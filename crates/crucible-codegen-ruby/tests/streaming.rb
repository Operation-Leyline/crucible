# frozen_string_literal: true
require 'stream_client'
require 'json'
require 'socket'
require 'stringio'
R = StreamSdk::Runtime
VECTORS = JSON.parse(File.read(ENV.fetch('CHECKSUM_VECTORS')))
def check(value, message)
  raise message unless value
end
def rejects(type = ArgumentError)
  yield
rescue type => error
  error
else
  raise "Expected #{type}"
end
def drain(body)
  data = +''.b
  while (chunk = body.read(7))
    data << chunk
  end
  data
end
# Only #read: neither seekable nor sized, so it is one-shot with unknown length.
class OneShot
  def initialize(data) = @io = StringIO.new(data)
  def read(*args) = @io.read(*args)
end
# Produces bytes on demand, so a large body never exists in memory.
class Generated
  def initialize(size) = @left = size
  def read(length = nil, buffer = nil)
    return nil if @left.zero?
    count = [length || 65_536, @left].min
    @left -= count
    chunk = 'x' * count
    buffer ? buffer.replace(chunk) : chunk
  end
end
class Fake
  attr_reader :requests, :bodies
  def initialize(&handler)
    @handler, @requests, @bodies = handler, [], []
  end
  def send_request(request, read_body:, scope:, sink: nil)
    @requests << request
    @bodies << (request.body.is_a?(R::StreamBody) ? drain(request.body) : request.body)
    @handler.call(@requests.size, request, sink)
  end
end
def sdk(transport, **config) = StreamSdk::StreamingClient.new(R::ClientConfig.new(endpoint: 'https://example.invalid', transport: transport, base_delay: 0, **config))
retrying = ->(count, _request, _sink) { R::HttpResponse.new(count == 1 ? 503 : 200) }

data = 'streamed bytes'.b
fake = Fake.new(&retrying)
sdk(fake).upload(StreamSdk::UploadInput.new(body: data))
check(fake.bodies == [data, data] && fake.requests.map { |r| r.headers['content-length'] } == [data.bytesize.to_s] * 2, 'String streams replay with a known length')
io = StringIO.new('skip' + data)
io.read(4)
fake = Fake.new(&retrying)
sdk(fake).upload(StreamSdk::UploadInput.new(body: io))
check(fake.bodies == [data, data], 'Seekable IO replays from its starting position')
opened = 0
stream = R::ByteStream.new(length: data.bytesize) { opened += 1; StringIO.new(data) }
fake = Fake.new(&retrying)
sdk(fake).upload(StreamSdk::UploadInput.new(body: stream))
check(fake.bodies == [data, data] && opened == 2, 'Factory streams open once per attempt')
fake = Fake.new(&retrying)
check(rejects(R::ServiceError) { sdk(fake).upload(StreamSdk::UploadInput.new(body: OneShot.new(data))) }.metadata.status == 503, 'One-shot failure surfaces')
check(fake.requests.size == 1 && fake.requests[0].headers['transfer-encoding'] == 'chunked' && fake.bodies == [data], 'One-shot streams are never replayed and are chunked')

fake = Fake.new { R::HttpResponse.new(200) }
rejects { sdk(fake).sized(StreamSdk::SizedInput.new(body: OneShot.new('abc'))) }
rejects { sdk(fake).sized(StreamSdk::SizedInput.new(body: 'abcd')) }
check(fake.requests.empty?, 'requiresLength and length bounds are checked before dispatch')
sdk(fake).sized(StreamSdk::SizedInput.new(body: R::ByteStream.new(OneShot.new('abc'), length: 3)))
check(fake.bodies == ['abc'] && fake.requests[0].headers['content-length'] == '3', 'Declared length satisfies requiresLength')
rejects { sdk(fake).sized(StreamSdk::SizedInput.new(body: R::ByteStream.new(OneShot.new('abcd'), length: 3))) }

payload = VECTORS.fetch('inputUtf8').b
fake = Fake.new { R::HttpResponse.new(200) }
sdk(fake).store(StreamSdk::StoreInput.new(body: payload))
check(fake.requests.last.headers['x-amz-checksum-crc32'] == VECTORS.fetch('base64').fetch('CRC32') && fake.bodies.last == payload, 'Checksum precedes a replayable stream')
rejects { sdk(fake).store(StreamSdk::StoreInput.new(body: OneShot.new(payload))) }
sdk(fake).store(StreamSdk::StoreInput.new(body: OneShot.new(payload), checksum: 'supplied'))
check(fake.requests.last.headers['x-amz-checksum-crc32'] == 'supplied', 'Caller checksum lets a one-shot stream through')
sdk(fake, request_checksum_calculation: 'when_required').store(StreamSdk::StoreInput.new(body: OneShot.new(payload)))
check(!fake.requests.last.headers.key?('x-amz-checksum-crc32'), 'Opted-out checksum')
VECTORS.fetch('base64').each do |algorithm, expected|
  hasher = R::Checksums::Hasher.new(algorithm)
  payload.each_char { |char| hasher.update(char) }
  check(hasher.base64 == expected, 'Incremental ' + algorithm)
end
signed = R::Authentication.sign(R::HttpRequest.new(url: 'https://example.invalid/', method: 'PUT', body: R::ByteStream.new(payload)),
  R::AwsCredentials.new(access_key_id: 'key', secret_access_key: 'secret'), service: 'example', region: 'local')
check(signed.headers['x-amz-content-sha256'] == 'UNSIGNED-PAYLOAD', 'Streams sign as UNSIGNED-PAYLOAD')

chunked = Fake.new do |count, _request, sink|
  next R::HttpResponse.new(503) if count == 1
  ['ab', 'cd', 'e'].each { |chunk| sink.call(chunk) }
  R::HttpResponse.new(200, headers: {'x-count' => '3', 'x-amz-checksum-crc32' => R::Checksums.digest('CRC32', 'abcde')})
end
received = []
output = sdk(chunked).download { |chunk| received << chunk }
check(received == %w[ab cd e] && output.count == 3 && output.body.read == '', 'Block receives chunks after a retry')
target = StringIO.new
check(sdk(Fake.new { |_c, _r, sink| sink.call('xyz'); R::HttpResponse.new(200) }).download(response_target: target).body.equal?(target) && target.string == 'xyz', 'Response target')
check(sdk(Fake.new { |_c, _r, sink| sink.call('xyz'); R::HttpResponse.new(200) }).download.body.read == 'xyz', 'Default buffer')
validated = []
output = sdk(chunked, checksum_validated: ->(op, algorithm) { validated << [op, algorithm] }).fetch(StreamSdk::FetchInput.new) { |_chunk| nil }
check(validated == [['Fetch', 'CRC32']] && chunked.requests.last.headers['x-validation'] == 'ENABLED', 'Incremental response checksum')
corrupt = Fake.new { |_c, _r, sink| sink.call('abcde'); R::HttpResponse.new(200, headers: {'x-amz-checksum-crc32' => 'AAAA'}) }
rejects(R::ChecksumError) { sdk(corrupt).fetch(StreamSdk::FetchInput.new) { |_chunk| nil } }
broken = Fake.new do |count, _request, sink|
  sink.call('partial')
  raise R::NetworkError, 'reset' if count == 1
  R::HttpResponse.new(200)
end
rejects(R::NetworkError) { sdk(broken).download { |_chunk| nil } }
check(broken.requests.size == 1, 'Delivered response bytes are never retried')
rejects { sdk(Fake.new { R::HttpResponse.new(200) }).upload(StreamSdk::UploadInput.new(body: 42)) }

# Real HTTP on loopback: chunked and sized uploads, streamed downloads, cancellation, and bounded memory.
SIZE = Integer(ENV.fetch('STREAM_BYTES', (256 * 1024 * 1024).to_s))
WARM_SIZE = [SIZE / 4, 32 * 1024 * 1024].min
server = TCPServer.new('127.0.0.1', 0)
uploads = Queue.new
download_sizes = Queue.new
Thread.new do
  loop do
    socket = server.accept
    Thread.new(socket) do |client|
      while (line = client.gets)
        method, path = line.split(' ')
        headers = {}
        while (header = client.gets) && header != "\r\n"
          name, value = header.split(':', 2)
          headers[name.downcase] = value.strip
        end
        received = 0
        if headers['transfer-encoding'] == 'chunked'
          while (size = client.gets.to_i(16)).positive?
            received += IO.copy_stream(client, IO::NULL, size)
            client.read(2)
          end
          client.read(2)
        elsif (length = headers['content-length'])
          received = IO.copy_stream(client, IO::NULL, Integer(length))
        end
        uploads << [method, path, received, headers['transfer-encoding']] if method == 'PUT'
        case path
        when '/download'
          size = download_sizes.pop
          client.write("HTTP/1.1 200 OK\r\nContent-Length: #{size}\r\nx-count: 1\r\n\r\n")
          block = 'y' * 65_536
          left = size
          while left.positive?
            client.write(left >= block.bytesize ? block : block[0, left])
            left -= [left, block.bytesize].min
          end
        when '/fetch'
          client.write("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
          loop do
            client.write("4\r\nslow\r\n")
            sleep 0.05
          end
        else client.write("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
        end
      end
    rescue IOError, SystemCallError
      nil
    ensure
      client.close
    end
  end
end
def peak_rss
  if File.readable?('/proc/self/status')
    File.read('/proc/self/status')[/VmHWM:\s+(\d+)/, 1].to_i * 1024
  else
    Integer(IO.popen(['powershell', '-NoProfile', '-Command', "(Get-Process -Id #{Process.pid}).PeakWorkingSet64"], &:read).strip)
  end
end
client = StreamSdk::StreamingClient.new(R::ClientConfig.new(endpoint: "http://127.0.0.1:#{server.addr[1]}", timeout: 120))
client.upload(StreamSdk::UploadInput.new(body: 'warm'))
uploads.pop
# Ruby retains heap arenas and native allocator pages after collections. Warm
# each transfer path before measuring its high-water growth with a larger body.
client.upload(StreamSdk::UploadInput.new(body: Generated.new(WARM_SIZE)))
check(uploads.pop == ['PUT', '/upload', WARM_SIZE, 'chunked'], 'Warm chunked upload')
client.upload(StreamSdk::UploadInput.new(body: R::ByteStream.new(length: WARM_SIZE) { Generated.new(WARM_SIZE) }))
check(uploads.pop == ['PUT', '/upload', WARM_SIZE, nil], 'Warm sized upload')
download_sizes << WARM_SIZE
warm_total = 0
warm_output = client.download { |chunk| warm_total += chunk.bytesize }
check(warm_total == WARM_SIZE && warm_output.count == 1, 'Warm streamed download')
GC.start
before = peak_rss
client.upload(StreamSdk::UploadInput.new(body: Generated.new(SIZE)))
check(uploads.pop == ['PUT', '/upload', SIZE, 'chunked'], 'Chunked upload of unknown length')
client.upload(StreamSdk::UploadInput.new(body: R::ByteStream.new(length: SIZE) { Generated.new(SIZE) }))
check(uploads.pop == ['PUT', '/upload', SIZE, nil], 'Sized upload')
total = 0
largest = 0
download_sizes << SIZE
output = client.download do |chunk|
  total += chunk.bytesize
  largest = [largest, chunk.bytesize].max
end
check(total == SIZE && output.count == 1 && largest <= 1024 * 1024, 'Streamed download')
growth = peak_rss - before
check(growth < 96 * 1024 * 1024, "Bounded memory: peak grew #{growth} bytes for #{SIZE} bytes each way")
start = R::CallScope.now
token = R::CancellationToken.new
chunks = 0
rejects(R::CancelledError) { client.fetch(StreamSdk::FetchInput.new, cancellation: token) { |_chunk| token.cancel if (chunks += 1) == 2 } }
check(chunks == 2 && R::CallScope.now - start < 2, 'Cancellation stops a streamed response')
client.close
deadline = StreamSdk::StreamingClient.new(R::ClientConfig.new(endpoint: "http://127.0.0.1:#{server.addr[1]}", timeout: 0.3))
start = R::CallScope.now
rejects(R::DeadlineError) { deadline.fetch(StreamSdk::FetchInput.new) { |_chunk| nil } }
check(R::CallScope.now - start < 2, 'Deadline stops a stalled streamed response')
deadline.close
puts "Ruby streaming blobs passed (#{SIZE} bytes each way, peak memory growth #{growth} bytes)"

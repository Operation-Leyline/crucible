# frozen_string_literal: true
require 'checksum_client'
require 'json'
R = ChecksumSdk::Runtime
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
VECTORS = JSON.parse(File.read(ENV.fetch('CHECKSUM_VECTORS')))
DATA = VECTORS.fetch('inputUtf8').b
VECTORS.fetch('base64').each do |algorithm, expected|
  check(R::Checksums.digest(algorithm, DATA) == expected, 'Published vector ' + algorithm)
end
check(R::Checksums.digest('MD5', '') == VECTORS.fetch('emptyMd5'), 'Empty MD5')
rejects { R::Checksums.digest('XXHASH64', DATA) }

class Server
  attr_accessor :mode
  attr_reader :requests
  def initialize
    @mode, @requests = 'normal', []
  end
  def send_request(request, read_body:, scope:)
    @requests << request
    algorithm = request.headers['x-algorithm']
    unless %w[override skip].include?(@mode)
      name = algorithm ? 'x-amz-checksum-' + algorithm.downcase : 'content-md5'
      expected = request.body.nil? || request.body.empty? ? VECTORS.fetch('emptyMd5') : VECTORS.fetch('base64').fetch(algorithm || 'MD5')
      raise "Request checksum #{name}: #{request.headers[name].inspect}" unless request.headers[name] == expected
    end
    raise 'Validated responses must be read' unless read_body || @mode == 'skip'
    selected = algorithm || 'CRC32'
    headers = {'x-amz-checksum-xxhash3' => 'unmodeled'}
    headers['x-amz-checksum-' + selected.downcase] = @mode == 'bad' ? 'AAAA' : VECTORS.fetch('base64').fetch(selected) unless @mode == 'unmodeled'
    headers['x-amz-checksum-sha256'] = 'AAAA' if @mode == 'one'
    R::HttpResponse.new(200, headers: headers, body: DATA)
  end
end
server = Server.new
validated = []
client = ChecksumSdk::ChecksumsClient.new(R::ClientConfig.new(endpoint: 'https://example.invalid', transport: server,
  checksum_validated: ->(operation, algorithm) { validated << [operation, algorithm] }))
client.legacy(ChecksumSdk::Input.new(data: DATA))
client.required(ChecksumSdk::Input.new(data: DATA))
client.legacy(ChecksumSdk::Input.new(data: ''.b))
input = ChecksumSdk::Input.new(data: DATA)
check(client.flexible(input).data == DATA, 'Flexible checksum round trip')
check(input.algorithm.equal?(R::UNSET) && input.validation.equal?(R::UNSET), 'Defaults never mutate caller input')
check(server.requests.last.headers['x-validation'] == 'ENABLED' && server.requests.last.headers['x-algorithm'] == 'CRC32', 'Modeled defaults are sent')
check(validated.last == ['Flexible', 'CRC32'], 'Validation callback')
VECTORS.fetch('base64').each_key do |algorithm|
  client.flexible(ChecksumSdk::Input.new(data: DATA, algorithm: ChecksumSdk::Algorithm.new(algorithm)))
  check(validated.last == ['Flexible', algorithm], 'Validated ' + algorithm)
end
server.mode = 'bad'
check(rejects(R::ChecksumError) { client.flexible(input) }.algorithm == 'CRC32', 'Corrupt response rejected')
server.mode = 'one'
client.flexible(input)
server.mode = 'unmodeled'
count = validated.size
client.flexible(input)
check(validated.size == count, 'Unmodeled checksum is not validated')
server.mode = 'override'
client.flexible(ChecksumSdk::Input.new(data: DATA, checksum: 'supplied'))
check(server.requests.last.headers['x-amz-checksum-crc32'] == 'supplied', 'Caller checksum header is preserved')
client.legacy(ChecksumSdk::Input.new(data: DATA, md5: 'supplied'))
check(server.requests.last.headers['content-md5'] == 'supplied', 'Caller MD5 header is preserved')
server.mode = 'normal'
rejects { client.flexible(ChecksumSdk::Input.new(data: DATA, algorithm: ChecksumSdk::Algorithm::XXHASH64)) }
check(server.requests.last.headers['x-algorithm'] != 'XXHASH64', 'Unsupported algorithm is never dispatched')
server.mode = 'skip'
optout = ChecksumSdk::ChecksumsClient.new(R::ClientConfig.new(endpoint: 'https://example.invalid', transport: server,
  request_checksum_calculation: 'when_required', response_checksum_validation: 'WHEN_REQUIRED'))
optout.flexible(ChecksumSdk::Input.new(data: DATA))
check(!server.requests.last.headers.key?('x-algorithm') && !server.requests.last.headers.key?('x-validation') && !server.requests.last.headers.key?('x-amz-checksum-crc32'), 'Opt-out policies')
rejects { R::ClientConfig.new(request_checksum_calculation: 'sometimes') }
server.mode = 'normal'
hooked = ChecksumSdk::ChecksumsClient.new(R::ClientConfig.new(endpoint: 'https://example.invalid', transport: server,
  middleware: [->(request) { request.with(body: DATA) }]))
hooked.legacy(ChecksumSdk::Input.new(data: ''.b))
check(server.requests.last.body == DATA, 'Checksum covers bytes after middleware')
puts 'Ruby checksum vectors, defaults, validation, overrides, and policies passed'

# frozen_string_literal: true
# A payload's Content-Type is the modelled member if set, then the payload's
# media type, then a client-wide default: exactly one value either way.
require 'payload_client'
require 'stringio'
R = PayloadSdk::Runtime
def check(value, message)
  raise message unless value
end
class Fake
  attr_reader :requests, :bodies
  def initialize
    @requests, @bodies = [], []
  end
  def send_request(request, read_body:, scope:, sink: nil)
    @requests << request
    if request.body.is_a?(R::StreamBody)
      data = +''.b
      while (chunk = request.body.read(2))
        data << chunk
      end
      @bodies << data
    else
      @bodies << request.body
    end
    R::HttpResponse.new(200)
  end
end
[false, true].each do |streaming|
  [nil, 'image/png'].each do |content_type|
    [nil, 'image/jpeg'].each do |configured|
      label = "#{streaming ? 'streaming' : 'buffered'}/#{content_type || '-'}/#{configured || '-'}"
      fake = Fake.new
      headers = configured ? {'Content-Type' => configured} : {}
      client = PayloadSdk::PayloadBindingsClient.new(R::ClientConfig.new(endpoint: 'https://example.invalid', transport: fake, headers: headers))
      fields = content_type ? {content_type: content_type} : {}
      if streaming
        client.upload_stream(PayloadSdk::StreamInput.new(data: R::ByteStream.new(length: 3) { StringIO.new('PNG') }, **fields))
      else
        client.upload(PayloadSdk::UploadInput.new(data: 'PNG', **fields))
      end
      check(fake.requests.size == 1 && fake.bodies[0] == 'PNG'.b, "#{label} body #{fake.bodies.inspect}")
      values = fake.requests[0].headers.select { |name, _| name.to_s.downcase == 'content-type' }.values
      check(values == [content_type || 'application/octet-stream'], "#{label} content type #{values.inspect}")
    end
  end
end
puts 'Ruby payload Content-Type precedence passed'

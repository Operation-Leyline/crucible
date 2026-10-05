# frozen_string_literal: true
# Native adapter for tools/protocol_conformance.py: one Smithy HTTP vector on stdin, one
# snapshot on stdout. Usage: ruby -I <lib> protocol_adapter.rb <package> <Module>
require 'json'
require 'uri'
require 'base64'
require ARGV.fetch(0)
SDK = Object.const_get(ARGV.fetch(1))
R = SDK::Runtime
CODEC = SDK.codec
class Captured < StandardError
  attr_reader :request
  def initialize(request)
    @request = request
    super('captured')
  end
end
class Capture
  def send_request(request, read_body:, scope:) = raise(Captured.new(request))
  def close; end
end

def shape(target) = CODEC.shape(target)

# Smithy test parameters to the codec's wire values: epoch timestamps and text blobs.
def wire(target, node, format)
  return node if node.nil?
  info = shape(target)
  case info.fetch('kind')
  when 'structure', 'union'
    node.to_h { |key, value| [key, info.fetch('members').key?(key) ? wire(info['members'][key]['target'], value, info['members'][key]['format']) : value] }
  when 'list' then node.map { |item| wire(info['member']['target'], item, info['member']['format']) }
  when 'map' then node.transform_values { |item| wire(info['value']['target'], item, info['value']['format']) }
  when 'timestamp' then R::Timestamp.from_epoch(node).wire(format || info['format'])
  when 'blob' then Base64.strict_encode64(node)
  else node
  end
end

# The inverse of wire, leaving out absent members.
def params(target, value, format)
  info = shape(target)
  case info.fetch('kind')
  when 'structure', 'union'
    info.fetch('members').filter_map { |key, member| [key, params(member['target'], value[key], member['format'])] unless value[key].nil? }.to_h
  when 'list' then value.map { |item| params(info['member']['target'], item, info['member']['format']) }
  when 'map' then value.transform_values { |item| params(info['value']['target'], item, info['value']['format']) }
  when 'timestamp' then R::Timestamp.read(value, format || info['format']).epoch
  when 'blob' then Base64.strict_decode64(value).force_encoding(Encoding::UTF_8)
  else value
  end
end

def run(vector)
  if vector['vendorParams'] || vector['authScheme']
    return {'unsupported' => 'Ruby adapter needs vendor/auth fixture configuration'}
  end
  test = vector.fetch('case')
  name = vector.fetch('operation').split('#', 2).last
  op = CODEC.schema.fetch('operations').fetch(name)
  target = vector['shape']
  config = R::ClientConfig.new(endpoint: 'https://' + (test['host'] || 'example.test'), transport: Capture.new, max_attempts: 1)
  runtime = R::ClientRuntime.new(config, CODEC, endpoint: '', digest: false)
  if test.fetch('kind') == 'request'
    input = target ? CODEC.decode(wire(target, vector.fetch('params'), nil), CODEC.context(target)) : nil
    begin
      runtime.call(name, input)
    rescue Captured => captured
      request = captured.request
      uri = URI.parse(request.url)
      return {'method' => request.method, 'uri' => uri.path, 'host' => uri.host,
              'queryParams' => uri.query ? uri.query.split('&') : [], 'headers' => request.headers,
              'body' => request.body.to_s.dup.force_encoding(Encoding::UTF_8)}
    end
    raise 'native client did not send a request'
  end
  message = test.fetch('message')
  response = R::HttpResponse.new(test.fetch('code'), headers: message.fetch('headers'), body: message['body'] || '')
  if (200...300).cover?(response.status)
    values = runtime.send(:success, name, op, response)
    output = op['output']
    return {'params' => output ? params(output, CODEC.encode(CODEC.decode(values, CODEC.context(output)), CODEC.context(output)), nil) : {}, 'error' => false}
  end
  error = runtime.send(:failure, op, response)
  modeled = error.class != R::ServiceError && target
  {'params' => modeled ? params(target, CODEC.encode(error, CODEC.context(target)), nil) : nil, 'error' => true}
end

print R::ExactJson.encode(run(R::ExactJson.decode($stdin.read)))

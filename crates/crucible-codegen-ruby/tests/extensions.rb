# frozen_string_literal: true
require 'dynamic_client'
require 'types_client'

class DynamicTransport
  def initialize(response) = @response = response
  def send_request(request, read_body:, scope:)
    raise 'method' unless request.method == 'POST'
    raise 'greedy/query' unless request.url == 'http://127.0.0.1/files/folder/a%20b?empty=&tag=hello%20world&tag=%2F'
    raise 'prefix' unless request.headers['x-meta-trace'] == 'abc'
    raise 'body' unless Dynamic::Runtime::ExactJson.decode(request.body) == {'wire-value' => 'hello'}
    raise 'read' unless read_body
    @response
  end
end
input = Dynamic::Input.new(key: 'folder/a b', query: {'tag' => ['hello world', '/'], 'empty' => ['']}, metadata_value: {'trace' => 'abc'}, value: 'hello')
response = Dynamic::Runtime::HttpResponse.new(206, headers: {'X-Meta-Trace' => 'abc'}, body: '{"wire-value":"result","value":"ignored"}')
client = Dynamic::DynamicClient.new(Dynamic::Runtime::ClientConfig.new(endpoint: 'http://127.0.0.1', transport: DynamicTransport.new(response)))
output = client.get(input)
raise 'output bindings' unless output.status == 206 && output.metadata_value == {'trace' => 'abc'} && output.value == 'result'
response = Dynamic::Runtime::HttpResponse.new(422, headers: {'x-crucible-error' => 'Problem', 'x-meta-reason' => 'invalid'}, body: '{}')
client = Dynamic::DynamicClient.new(Dynamic::Runtime::ClientConfig.new(endpoint: 'http://127.0.0.1', transport: DynamicTransport.new(response)))
begin
  client.get(input)
  raise 'expected modeled error'
rescue Dynamic::Problem => error
  raise 'error bindings' unless error.status == 422 && error.details == {'reason' => 'invalid'}
end
['a/../b', './a', 'a/.'].each do |key|
  begin
    client.get(Dynamic::Input.new(key: key))
    raise 'accepted dot segment'
  rescue ArgumentError
  end
end
codec = Types.codec
node = Types::Runtime::ExactJson.decode('{"mode":123,"names":["a","b"],"sparseNames":[null,"x"],"sparseMap":{"absent":null,"future":123}}')
output = codec.decode(node, codec.context('example.types#Output'))
raise 'integer' unless output.integer == 12345678901234567890123456789012345678901234567890
raise 'decimal' unless output.decimal == BigDecimal('12345678901234567890.12345678901234567890123456789')
raise 'sparse' unless output.sparse_names == [nil, 'x'] && output.sparse_map.key?('absent') && output.sparse_map['absent'].nil?
raise 'future enum' unless output.mode.value == 123 && output.sparse_map['future'].value == 123
encoded = codec.encode(output, codec.context('example.types#Output'))
raise 'roundtrip' unless codec.equivalent(encoded['sparseMap'], node['sparseMap']) && encoded['sparseNames'] == node['sparseNames']
[['example.types#Names', ['a', 'a']], ['example.types#Names', [nil]], ['example.types#Mode', 2147483648]].each do |target, value|
  begin
    codec.decode(value, codec.context(target))
    raise 'accepted invalid value'
  rescue ArgumentError
  end
end
puts 'Ruby dynamic bindings and extended types passed'
{'Integers' => '[18446744073709551616,18446744073709551617]', 'Decimals' => '[1.00000000000000000001,1.00000000000000000002]'}.each do |shape, json|
  codec.decode(Types::Runtime::ExactJson.decode(json), codec.context('example.types#' + shape))
  begin
    codec.decode(Types::Runtime::ExactJson.decode('[1,1.0]'), codec.context('example.types#' + shape))
    raise 'accepted numerically equal duplicate'
  rescue ArgumentError
  end
end

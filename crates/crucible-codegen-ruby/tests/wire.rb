# frozen_string_literal: true
require 'wire_client'
class WireTransport
  def initialize(failure) = @failure = failure
  def send_request(request, read_body:, scope:)
    raise 'read' unless read_body
    if request.url.end_with?('/document')
      raise 'document payload' unless request.headers['content-type'] == 'application/json' && WireSdk::Runtime::ExactJson.decode(request.body) == {'name' => 'record'}
      return WireSdk::Runtime::HttpResponse.new(200, body: request.body)
    end
    raise 'string payload' unless request.headers['content-type'] == 'text/plain' && request.body == 'héllo'.b
    raise 'media headers' unless request.headers['x-meta'] == 'eyJhIjoxfQ==, aGVsbG8='
    @failure ? WireSdk::Runtime::HttpResponse.new(422, headers: {'x-crucible-error' => 'TextError'}, body: 'invalid text') : WireSdk::Runtime::HttpResponse.new(200, headers: {'x-meta' => 'eyJhIjoxfQ==, aGVsbG8='}, body: request.body)
  end
end
input = WireSdk::TextInput.new(text: 'héllo', meta: ['{"a":1}', 'hello'])
client = WireSdk::WireClient.new(WireSdk::Runtime::ClientConfig.new(endpoint: 'http://127.0.0.1', transport: WireTransport.new(false)))
output = client.text(input)
raise 'output' unless output.text == input.text && output.meta == input.meta
raise 'document' unless client.document(WireSdk::DocumentInput.new(document: WireSdk::Record.new(name: 'record'))).document.name == 'record'
client = WireSdk::WireClient.new(WireSdk::Runtime::ClientConfig.new(endpoint: 'http://127.0.0.1', transport: WireTransport.new(true)))
begin
  client.text(input)
  raise 'expected TextError'
rescue WireSdk::TextError => error
  raise 'error payload' unless error.details == 'invalid text' && error.metadata.status == 422
end
puts 'Ruby payloads and media headers passed'

codec = WireSdk::Runtime::WireCodec.new({'shapes' => {
  'decimal' => {'kind' => 'bigDecimal'},
  'decimals' => {'kind' => 'list', 'set' => true, 'member' => {'target' => 'decimal'}}
}}, {})
number = WireSdk::Runtime::JsonNumber
set = {'set' => true}
duplicate = lambda do |values, info = set|
  begin
    codec.collection(values, info)
  rescue ArgumentError => error
    raise unless error.message == 'Duplicate set member'
  else
    raise 'Expected duplicate set member'
  end
end
duplicate.call([number.new('1e0'), 1.0])
duplicate.call([1, number.new('1.000')])
duplicate.call([-0.0, number.new('0')])
duplicate.call([{'a' => [1, {'inside' => number.new('2.0')}], 'b' => true},
                {'b' => true, 'a' => [number.new('1e0'), {'inside' => 2.0}]}])
distinct = [[1, 2], [2, 1], 1, '1', {'a' => 1}, {'b' => 1}, {1 => 2}, {1.0 => 2}]
raise 'Distinct set values' unless codec.collection(distinct, set).equal?(distinct)
raise 'Ordinary list duplicates' unless codec.collection([1, 1], {}).size == 2
duplicate.call([nil, nil], {'set' => true, 'sparse' => true})
begin
  codec.collection([nil], {'set' => true})
  raise 'Expected dense null failure'
rescue ArgumentError => error
  raise unless error.message == 'Null in dense list'
end

context = codec.context('decimals')
begin
  codec.encode([BigDecimal('1.0'), BigDecimal('1')], context)
  raise 'Expected encoded decimal duplicate'
rescue ArgumentError => error
  raise unless error.message == 'Duplicate set member'
end
begin
  codec.decode([number.new('1.0'), number.new('1')], context)
  raise 'Expected decoded decimal duplicate'
rescue ArgumentError => error
  raise unless error.message == 'Duplicate set member'
end
decimals = codec.decode([number.new('1.00000000000000000001'), number.new('1.00000000000000000002')], context)
raise 'Exact distinct decimals' unless decimals[0] != decimals[1] && codec.encode(decimals, context).size == 2

class CountingSetCodec < WireSdk::Runtime::WireCodec
  attr_reader :comparisons, :canonicalizations
  def initialize
    super({'shapes' => {}}, {})
    @comparisons = 0
    @canonicalizations = 0
  end
  def equivalent(left, right)
    @comparisons += 1
    super
  end
  def canonical(value)
    @canonicalizations += 1
    super
  end
end
class NoPrefixArray < Array
  def take(*) = raise('Set duplicate detection scanned a prefix')
end
large = NoPrefixArray.new((0...10_000).map { |index| {'id' => index, 'nested' => [index, false]} })
counting = CountingSetCodec.new
raise 'Large distinct set' unless counting.collection(large, {'set' => true}).equal?(large)
raise 'Set duplicate detection compared pairs' unless counting.comparisons.zero?
raise 'Set duplicate detection repeatedly normalized members' unless counting.canonicalizations <= large.size * 10
puts 'Ruby set equivalence and linear duplicate detection passed'

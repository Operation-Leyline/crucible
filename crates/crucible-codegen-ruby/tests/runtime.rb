# frozen_string_literal: true
require 'weather_client'
require 'json'
include Weather::Runtime
include Weather

def check(value, message)
  raise message unless value
end
def rejects(type = ArgumentError)
  begin
    yield
  rescue type => error
    return error
  end
  raise "Expected #{type}"
end
native_input = Weather::CreateCityInput.new(name: 'Oslo', location: nil, expected_revision: 7)
check(native_input.name == 'Oslo' && native_input.location.nil? && native_input.client_token.equal?(UNSET), 'Direct package models and native presence')
native_page = Weather::ListCitiesOutput.new(cities: [Weather::CitySummary.new(id: 'c', name: 'Oslo', population: 7)])
check(native_page.cities.instance_of?(Array) && native_page.cities.map(&:name) == ['Oslo'], 'Native response collections and readers')
check(!Weather.const_defined?(:Models, false), 'No artificial model container')
class Fake
  attr_reader :requests, :closed
  def initialize(&handler)
    @handler, @requests, @closed = handler, [], false
  end
  def send_request(request, read_body:, scope:)
    @requests << request
    @handler.call(@requests.size, request)
  end
  def close = @closed = true
end
def sdk(fake, **config)
  Weather::WeatherClient.new(ClientConfig.new(endpoint: 'https://example.invalid', transport: fake, base_delay: 0, **config))
end

codec = Weather.codec
fake = Fake.new do |_, request|
  check(!request.headers.key?('authorization'), 'Public operation has no bearer header')
  HttpResponse.new(204, headers: {}, body: '')
end
sdk(fake, token: -> { raise 'Public operation requested credentials' }).public_ping
fake = Fake.new do |_, request|
  check(request.headers['x-amz-content-sha256'] == Digest::SHA256.hexdigest(''), 'Empty write digest')
  HttpResponse.new(200, headers: {}, body: '{}')
end
%w[POST PUT PATCH GET DELETE].each do |method|
  transport = Fake.new do |_, request|
    check(request.body.nil? && request.method == method, 'Bodiless request')
    if %w[GET DELETE].include?(method)
      check(!request.headers.key?('x-amz-content-sha256'), 'Bodiless read has no digest')
    else
      check(request.headers['x-amz-content-sha256'] == Digest::SHA256.hexdigest(''), 'Empty write digest')
    end
    HttpResponse.new(200, headers: {}, body: '{}')
  end
  sdk(transport, middleware: [->(request) { request.with(method: method) }]).retry_safe
  check(transport.requests.size == 1, 'One bodiless request')
end
sdk(fake).retry_safe
['.', '..'].each do |label|
  fake = Fake.new { raise 'Dot label reached transport' }
  rejects { sdk(fake).get_city(GetCityInput.new(city_id: label)) }
  check(fake.requests.empty?, 'Dot label rejected before dispatch')
end
rejects { HttpRequest.new(url: 'https://example.invalid', method: "GET\r\nX: y") }
rejects { HttpRequest.new(url: 'https://example.invalid', method: 'GET', headers: {"x\r\nInjected" => 'y'}) }
rejects { HttpRequest.new(url: 'https://example.invalid', method: 'GET', headers: {'x' => "y\r\nz"}) }
value = ExactJson.decode('{"long":9223372036854775807,"tiny":1.000000000000000000001,"nul\u0000key":{},"list":[]}')
check(value['long'] == 2**63 - 1 && ExactJson.encode(value).include?('1.000000000000000000001'), 'Exact JSON numbers')
check(ExactJson.decode(ExactJson.encode(value)).fetch("nul\0key") == {}, 'Object identity')
rejects { JsonNumber.new('9223372036854775808').integer }
rejects { JsonNumber.new('1.01').integer }
rejects { ExactJson.decode('NaN') }
rejects { ExactJson.decode('[1,]') }
rejects { ExactJson.decode('"\udc00"') }
rejects { ExactJson.decode("\"\xff\"".b) }
rejects { JsonValue.new("\xff".b) }
rejects { ExactJson.encode(Float::INFINITY) }
rejects { ExactJson.encode({1 => 'bad'}) }
[-(2**63), -1, 0, 2**63 - 1].each do |second|
  timestamp = Timestamp.new(second, 999_999_999)
  copy = Timestamp.from_epoch(timestamp.epoch)
  check(copy.seconds == second && copy.nanoseconds == 999_999_999, 'Exact epoch')
end
check(Timestamp.from_epoch(JsonNumber.new('-0.000000001')).date_time == '1969-12-31T23:59:59.999999999Z', 'Negative epoch')
check(Timestamp.from_time(Timestamp.new(100, 123456789).to_time).nanoseconds == 123456789, 'Native Time precision')
rejects { Timestamp.parse('2026-02-30T00:00:00Z') }
rejects { Timestamp.parse('1500-02-29T00:00:00Z') }
check(Timestamp.parse('1582-10-10T00:00:00Z').date_time == '1582-10-10T00:00:00Z', 'Gregorian calendar')
rejects { Timestamp.parse('2026-01-01T24:00:00Z') }
rejects { Timestamp.from_epoch(JsonNumber.new('0.0000000001')) }
rejects { Timestamp.from_http_date('Fri, 01 Jan 1970 00:00:00 GMT') }
input = CreateCityInput.new(name: 'ruby', location: Location::Unknown.new('future', JsonValue.parse('{"long":9223372036854775807}')))
wire = codec.encode(input, codec.context('example.weather#CreateCityInput'))
check(!wire.key?('clientToken') && wire['location']['future']['long'] == 2**63 - 1, 'Native model wire')
copy = codec.decode(wire, codec.context('example.weather#CreateCityInput'))
check(copy.location.is_a?(Location::Unknown) && copy.name == 'ruby', 'Native model round trip')
null_input = CreateCityInput.new(name: nil)
check(codec.encode(null_input, codec.context('example.weather#CreateCityInput')) == {'name' => nil}, 'Null versus absent')
rejects { codec.encode(null_input, codec.context('example.weather#GetCityInput')) }
schema = ExactJson.decode('{"shapes":{"doc":{"kind":"document"},"long":{"kind":"long"},"str":{"kind":"string"},"list":{"kind":"list","member":{"target":"str"}},"parent":{"kind":"structure","members":{"doc":{"target":"doc","default":null},"count":{"target":"long","default":9223372036854775807},"optional":{"target":"long","default":2}}}}}')
simple = WireCodec.new(schema, {})
context = simple.context('parent')
check(context.read('doc', {'doc' => nil}).value.nil?, 'Required document null')
rejects { context.read('doc', {}) }
check(context.read('count', {}) == 2**63 - 1, 'Exact default')
rejects { context.read('count', {'count' => nil}) }
check(context.read('optional', {}, optional: true) == 2 && context.read('optional', {'optional' => nil}, optional: true).nil?, 'Default/null')
::JSON.parse(::File.read(ENV.fetch('HEADER_CASES'))).each do |row|
  member = {'target' => 'list'}
  check(simple.header(member, row.fetch('wire')) == row.fetch('values'), 'Header fixture')
  check(simple.header(member, simple.bound(row.fetch('values'))) == row.fetch('values'), 'Header round trip')
end
::JSON.parse(::File.read(ENV.fetch('RETRY_CASES'))).each do |row|
  fake = Fake.new do |attempt, _request|
    body = row['error'] ? {'__type' => row['error']} : {}
    HttpResponse.new(attempt <= row['failures'] ? row['status'] : 200, headers: {'x-amzn-errortype' => 'Throttled'}, body: ::JSON.generate(body))
  end
  client = sdk(fake, max_attempts: row['maxAttempts'])
  success = false
  begin
    row['safe'] ? client.retry_safe : client.retry_unsafe
    success = true
  rescue ServiceError
    nil
  end
  check(success == row['success'] && fake.requests.size == row['attempts'], 'Retry ' + row['name'])
  client.close
  check(!fake.closed, 'Caller transport ownership')
end
fake = Fake.new do |_attempt, request|
  check(request.url.include?('a%2Fb%20%2A%3F%23%C3%A9'), 'Label escaping')
  check(request.url.include?('at=2026-10-01T00%3A00%3A00Z'), 'Query timestamp')
  HttpResponse.new(200, headers: {'x-request-id' => 'bound'}, body: '{"city":{"id":"x","name":"n"}}')
end
output = sdk(fake).get_city(GetCityInput.new(city_id: 'a/b *?#é', at: Timestamp.parse('2026-10-01T00:00:00Z')))
check(output.request_id == 'bound', 'Bound response header')
token = 0
fake = Fake.new do |attempt, request|
  check(request.headers['authorization'] == "Bearer #{attempt}", 'Refreshed credential')
  check(request.headers['x-amz-content-sha256'] == ::Digest::SHA256.hexdigest(request.body), 'Final digest')
  HttpResponse.new(attempt == 1 ? 503 : 201, body: attempt == 1 ? '{"__type":"Future"}' : '{"city":{"id":"x","name":"n"}}')
end
token_calls = 0
client = sdk(fake, token: -> { (token += 1).to_s }, idempotency_token: -> { token_calls += 1; 'deterministic-idempotency-token' },
  middleware: [->(request) { request.with(body: request.body + ' ') }])
input = CreateCityInput.new(name: 'ruby')
client.create_city(input)
check(fake.requests.size == 2 && fake.requests[0].body == fake.requests[1].body && input.client_token.equal?(UNSET), 'Stable bytes and caller ownership')
check(token_calls == 1 && fake.requests[0].body.include?('deterministic-idempotency-token'), 'Configured idempotency token generated once and reused on retry')
fake = Fake.new { HttpResponse.new(429, headers: {'x-request-id' => 'header'}, body: '{"__type":"ThrottlingError","message":42}') }
error = rejects(ServiceError) { sdk(fake, max_attempts: 2).ping }
check(error.metadata.status == 429 && error.metadata.throttling && error.metadata.request_id == 'header' && fake.requests.size == 2, 'Malformed error retains metadata')
fake = Fake.new { HttpResponse.new(429, headers: {'x-request-id' => 'unicode'}, body: '{"__type":"ThrottlingError","message":"\udc00"}') }
error = rejects(ServiceError) { sdk(fake, max_attempts: 2).ping }
check(error.metadata.status == 429 && error.metadata.throttling && error.metadata.request_id == 'unicode' && fake.requests.size == 2, 'Malformed Unicode error retains wire metadata')
fake = Fake.new { HttpResponse.new(200, body: '{bad') }
rejects { sdk(fake).get_city(GetCityInput.new(city_id: 'x')) }
check(fake.requests.size == 1, 'Malformed success never retries')
fake = Fake.new { HttpResponse.new(200, body: '{"cities":[],"nextToken":"same"}') }
pages = sdk(fake).list_cities_pages(ListCitiesInput.new)
check(fake.requests.empty?, 'Lazy pagination')
pages.next
pages.next
rejects { pages.next }
check(fake.requests.size == 2, 'Pagination token cycle')
fake = Fake.new { HttpResponse.new(503, headers: {'retry-after' => '2147484'}) }
rejects(DeadlineError) { sdk(fake, timeout: 0.05).retry_safe }
check(fake.requests.size == 1, 'Retry deadline')
cancel = CancellationToken.new
cancel.cancel
fake = Fake.new { HttpResponse.new(200) }
rejects(CancelledError) { sdk(fake).ping(cancellation: cancel) }
check(fake.requests.empty?, 'Precancelled call')
polls = 0
latched = CancellationToken.new { (polls += 1) == 1 }
rejects(CancelledError) { latched.check }
rejects(CancelledError) { latched.check }
check(polls == 1, 'Cancellation stays latched')

client = Weather::WeatherClient.new(ClientConfig.new(endpoint: ENV.fetch('WEATHER_URL'), token: -> { 'weather-token' }))
city = client.create_city(CreateCityInput.new(name: 'Ruby city')).city
check(client.get_city(GetCityInput.new(city_id: city.id)).city.name == 'Ruby city', 'Real create and read')
photo = "\x00\x80\xff".b
client.put_city_photo(PutCityPhotoInput.new(city_id: city.id, photo: photo))
check(client.get_city_photo(GetCityPhotoInput.new(city_id: city.id)).photo == photo, 'Real raw payload')
check(client.list_cities_pages(ListCitiesInput.new(max_results: 1)).to_a.size > 0, 'Real pagination')
rejects(ConflictError) { client.create_city(CreateCityInput.new(name: 'Ruby city')) }
client.ping
client.close
fixture = ENV.fetch('FIXTURE_URL') + '/native/'
client = Weather::WeatherClient.new(ClientConfig.new(endpoint: fixture + 'bad-length', base_delay: 0))
check(client.get_city_photo(GetCityPhotoInput.new(city_id: 'x')).photo == 'complete', 'Malformed Content-Length retries')
client.close
client = Weather::WeatherClient.new(ClientConfig.new(endpoint: fixture + 'not-modified', max_attempts: 1))
check(rejects(ServiceError) { client.get_city(GetCityInput.new(city_id: 'x')) }.metadata.status == 304, 'Bodyless 304 retains wire status')
client.close
client = Weather::WeatherClient.new(ClientConfig.new(endpoint: fixture + 'partial', base_delay: 0))
check(client.get_city_photo(GetCityPhotoInput.new(city_id: 'x')).photo == 'complete', 'Partial body retry')
client.close
client = Weather::WeatherClient.new(ClientConfig.new(endpoint: fixture + 'empty-header', headers: {'x-empty' => ''}))
client.ping
client.close
client = Weather::WeatherClient.new(ClientConfig.new(endpoint: fixture + 'stall', timeout: 0.15))
start = CallScope.now
rejects(DeadlineError) { client.get_city(GetCityInput.new(city_id: 'x')) }
check(CallScope.now - start < 1.5, 'Stalled body deadline')
client.close
client = Weather::WeatherClient.new(ClientConfig.new(endpoint: fixture + 'stall', timeout: 5))
start = CallScope.now
token = CancellationToken.new { CallScope.now - start >= 0.1 }
rejects(CancelledError) { client.get_city(GetCityInput.new(city_id: 'x'), cancellation: token) }
check(CallScope.now - start < 1.5, 'Stalled body cancellation')
client.close
client = Weather::WeatherClient.new(ClientConfig.new(endpoint: fixture + 'discard', timeout: 0.5))
client.ping
client.close
client = Weather::WeatherClient.new(ClientConfig.new(endpoint: fixture + 'redirect', max_attempts: 1))
check(rejects(ServiceError) { client.ping }.metadata.status == 302, 'Redirect refusal')
client.close
token = 0
client = Weather::WeatherClient.new(ClientConfig.new(endpoint: fixture + 'echo', token: -> { (token += 1).to_s }, base_delay: 0))
client.create_city(CreateCityInput.new(name: 'x'))
check(token == 2, 'Real refreshed retry credential')
client.close
token = 0
client = Weather::WeatherClient.new(ClientConfig.new(endpoint: fixture + 'cache', token: -> { (token += 1).to_s }))
check(client.get_city(GetCityInput.new(city_id: 'c')).city.name == 'Bearer 1', 'First credential')
check(client.get_city(GetCityInput.new(city_id: 'c')).city.name == 'Bearer 2', 'No cached credential response')
client.close
puts 'Ruby exact codecs, shared fixtures, native models, and real HTTP passed'

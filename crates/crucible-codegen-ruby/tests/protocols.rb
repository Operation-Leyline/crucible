# frozen_string_literal: true
require 'json'
require 'uri'
def check(value, message)
  raise message unless value
end
class Handler
  def initialize(&block) = @block = block
  attr_reader :count
  def send_request(request, read_body:, scope:)
    @count = (@count || 0) + 1
    @block.call(request)
  end
end
[['restjson_client', 'RestJson', nil], ['json10_client', 'Json10', '1.0'], ['json11_client', 'Json11', '1.1']].each do |package, name, version|
  require package
  sdk = Object.const_get(name)
  r = sdk::Runtime
  instant = r::Timestamp.new(-1, 750_000_000)
  date = 'Wed, 31 Dec 1969 23:59:59 GMT'
  transport = Handler.new do |request|
    check(request.method == 'POST', 'AWS JSON method')
    body = r::ExactJson.decode(request.body)
    check(body['when'].text == '-0.25' && body['iso'] == '1969-12-31T23:59:59.75Z', "#{name} body timestamps")
    check(!body.key?('nullable') && body.key?('document') && body['document'].nil?, "#{name} structure null omitted, document null kept")
    check(body['floatValue'] == 'NaN' && body['doubleValue'] == 'Infinity', "#{name} non-finite floats")
    check(body[version ? 'renamed' : 'wire-name'] == 'hello', "#{name} member names")
    if version
      check(URI.parse(request.url).path == '/' && request.headers['x-amz-target'] == 'JsonService.Echo', "#{name} RPC target")
      check(request.headers['content-type'] == 'application/x-amz-json-' + version, "#{name} RPC content type")
      check(body['date'].text == '-0.25' && body['dates'].map(&:text) == ['-0.25', '-0.25'] && body['query'].text == '-0.25', "#{name} RPC body timestamps")
    else
      uri = URI.parse(request.url)
      check(uri.path == '/echo' && uri.query == 'time=1969-12-31T23%3A59%3A59.75Z&times=1969-12-31T23%3A59%3A59.75Z', 'REST query timestamps')
      check(request.headers['x-date'] == date && request.headers['x-dates'] == "#{date}, #{date}", 'REST header HTTP dates')
      check(!body.key?('date') && !body.key?('query') && request.headers['content-type'] == 'application/json', 'REST bindings leave the body')
    end
    body['choice'] = {'text' => 'ok', '__type' => 'ignored'}
    body['nullable'] = nil
    r::HttpResponse.new(200, headers: version ? {} : {'x-date' => date, 'x-dates' => "#{date}, #{date}"}, body: r::ExactJson.encode(body))
  end
  client = sdk::JsonServiceClient.new(r::ClientConfig.new(endpoint: 'https://example.invalid', transport: transport))
  output = client.echo(sdk::ValuesInput.new(renamed: 'hello', when_value: instant, iso: instant, date: instant, dates: [instant, instant], query: instant,
    queries: version ? r::UNSET : [instant], choice: sdk::Choice::TextVariant.new('choice'), float_value: Float::NAN, double_value: Float::INFINITY,
    nullable: nil, document: nil))
  check(output.choice.is_a?(sdk::Choice::TextVariant) && output.choice.value == 'ok', "#{name} union type discriminator ignored")
  check(output.nullable.equal?(r::UNSET) && output.document.nil?, "#{name} response nulls")
  check(output.float_value.nan? && output.double_value == Float::INFINITY, "#{name} response non-finite floats")
  check(output.when_value.seconds == -1 && output.when_value.nanoseconds == 750_000_000 && output.iso.nanoseconds == 750_000_000, "#{name} response timestamps")
  check(output.date.seconds == -1 && output.dates.map(&:seconds) == [-1, -1], 'REST response header HTTP dates') unless version
  check(transport.count == 1, "#{name} one request")

  [[{'x-amzn-errortype' => 'example.json#Problem:http://internal/'}, {'__type' => 'Other', 'Message' => 'bad'}],
   [{}, {'code' => 'Problem', 'Message' => 'bad'}],
   [{}, {'__type' => 'example.json#Problem', 'message' => 'bad'}]].each do |headers, body|
    transport = Handler.new { r::HttpResponse.new(409, headers: headers.merge('x-amzn-requestid' => 'request-42'), body: JSON.generate(body)) }
    client = sdk::JsonServiceClient.new(r::ClientConfig.new(endpoint: 'https://example.invalid', transport: transport))
    begin
      client.echo(sdk::ValuesInput.new)
      raise 'Expected modeled AWS error'
    rescue sdk::ProblemFault => error
      check(error.metadata.name == 'Problem' && error.metadata.request_id == 'request-42' && error.message.include?('bad'), "#{name} AWS error discriminator #{body}")
    end
  end
  transport = Handler.new do |request|
    if version
      check(request.body == '{}' && request.headers['x-amz-target'] == 'JsonService.Empty', "#{name} empty RPC body")
    else
      check(request.body.nil? && URI.parse(request.url).path == '/empty', 'REST empty operation')
    end
    r::HttpResponse.new(version ? 200 : 204)
  end
  sdk::JsonServiceClient.new(r::ClientConfig.new(endpoint: 'https://example.invalid', transport: transport)).empty
end
puts 'Ruby restJson1, awsJson1_0, and awsJson1_1 protocols passed'

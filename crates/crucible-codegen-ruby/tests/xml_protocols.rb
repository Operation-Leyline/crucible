# frozen_string_literal: true
# Independent consumer of the generated restXml, awsQuery and ec2Query gems.
require 'socket'
require 'uri'
require 'xml_client'
require 'nowrap_client'
require 'query_client'
require 'ec2_client'
def check(value, message)
  raise message unless value
end
def form(body) = body.split('&').to_h { |pair| pair.split('=', 2).map { |part| URI.decode_www_form_component(part) } }
class Handler
  def initialize(&block) = @block = block
  attr_reader :count
  def send_request(request, read_body:, scope:)
    @count = (@count || 0) + 1
    @block.call(request)
  end
  def close; end
end
def client(sdk, name, &block)
  r = sdk::Runtime
  sdk.const_get(name).new(r::ClientConfig.new(endpoint: 'https://example.invalid', transport: Handler.new(&block), max_attempts: 1))
end

# A loopback server, so one exchange of each protocol crosses real HTTP.
class Loopback
  attr_reader :url, :requests
  def initialize(responses)
    @server, @responses, @requests = TCPServer.new('127.0.0.1', 0), responses, []
    @url = "http://127.0.0.1:#{@server.addr[1]}"
    @thread = Thread.new do
      @responses.each do |status, headers, body|
        socket = @server.accept
        head = +''
        head << socket.readpartial(1) until head.end_with?("\r\n\r\n")
        length = head[/content-length: *(\d+)/i, 1].to_i
        @requests << [head, length.zero? ? '' : socket.read(length)]
        socket.write("HTTP/1.1 #{status} X\r\nContent-Length: #{body.bytesize}\r\nConnection: close\r\n" +
          headers.map { |key, value| "#{key}: #{value}\r\n" }.join + "\r\n" + body)
        socket.close
      end
    end
  end
  def join
    @thread.join(10)
    @server.close
  end
end

r = RestXml::Runtime
time = r::Timestamp.new(1_398_796_238)
xml = client(RestXml, :RestXmlClient) do |request|
  check(request.method == 'PUT' && URI.parse(request.url).path == '/XmlLists' && request.headers['content-type'] == 'application/xml', 'restXml route')
  check(request.body == '<XmlListsRequest><stringList><member>a &amp; b</member></stringList>' \
    '<timestampList><member>2014-04-29T18:30:38Z</member></timestampList><enumList><member>Foo</member></enumList>' \
    '<nestedStringList><member><member>x</member></member></nestedStringList><renamed><item>r</item></renamed>' \
    '<flattenedList>hi</flattenedList><flattenedList>bye</flattenedList>' \
    '<flattenedListWithMemberNamespace xmlns="https://xml-member.example.com">n</flattenedListWithMemberNamespace>' \
    '<myStructureList><item><value>1</value><other>2</other></item></myStructureList></XmlListsRequest>', "restXml list body #{request.body}")
  r::HttpResponse.new(200, body: '<XmlListsResponse/>')
end
lists = xml.xml_lists(RestXml::XmlListsRequest.new(string_list: ['a & b'], timestamp_list: [time], enum_list: [RestXml::FooEnum::FOO],
  nested_string_list: [['x']], renamed_list_members: ['r'], flattened_list: %w[hi bye], flattened_list_with_member_namespace: ['n'],
  structure_list: [RestXml::StructureListMember.new(a: '1', b: '2')]))
check(lists.string_list.equal?(r::UNSET), 'absent list stays absent')

xml = client(RestXml, :RestXmlClient) do |request|
  check(request.headers['x-foo'] == 'Foo', 'header binding stays a header')
  check(request.body == '<SimpleScalarPropertiesRequest><stringValue>&#xD;' + "\n" + '</stringValue><trueBooleanValue>true</trueBooleanValue>' \
    '<floatValue>NaN</floatValue><DoubleDribble>-Infinity</DoubleDribble></SimpleScalarPropertiesRequest>', "scalar body #{request.body}")
  r::HttpResponse.new(200, headers: {'X-Foo' => 'Bar'}, body: '<?xml version="1.0" encoding="UTF-8"?>' + "\n" \
    '<SimpleScalarPropertiesResponse><stringValue>a&#xD;&#10;<![CDATA[<b>]]></stringValue><byteValue> 7 </byteValue>' \
    '<floatValue>Infinity</floatValue><DoubleDribble>2.5</DoubleDribble></SimpleScalarPropertiesResponse>')
end
scalars = xml.simple_scalar_properties(RestXml::SimpleScalarPropertiesRequest.new(foo: 'Foo', string_value: "\r\n", true_boolean_value: true,
  float_value: Float::NAN, double_value: -Float::INFINITY))
check(scalars.foo == 'Bar' && scalars.string_value == "a\r\n<b>" && scalars.byte_value == 7, 'scalar response')
check(scalars.float_value == Float::INFINITY && scalars.double_value == 2.5, 'non-finite response floats')

xml = client(RestXml, :RestXmlClient) do |request|
  check(request.body == '<Hola><name>Phreddy</name></Hola>', "payload member name #{request.body}")
  r::HttpResponse.new(200, body: '<Anything><name>Back</name></Anything>')
end
check(xml.http_payload_with_member_xml_name(RestXml::HttpPayloadWithMemberXmlNameRequest.new(nested: RestXml::PayloadWithXmlName.new(name: 'Phreddy'))).nested.name == 'Back', 'payload response')

xml = client(RestXml, :RestXmlClient) do |request|
  check(request.body == '<XmlAttributesRequest test="&lt;&quot;&amp;&#xA;"><foo>x</foo></XmlAttributesRequest>', "attribute escaping #{request.body}")
  r::HttpResponse.new(200, body: '<XmlAttributesResponse test="a&amp;b"><foo>x</foo></XmlAttributesResponse>')
end
check(xml.xml_attributes(RestXml::XmlAttributesRequest.new(foo: 'x', attr: "<\"&\n")).attr == 'a&b', 'attribute response')

xml = client(RestXml, :RestXmlClient) { r::HttpResponse.new(200, body: '<r><myMap><key>k</key><value>Baz</value></myMap><myMap><key>j</key><value>0</value></myMap></r>') }
flat = xml.flattened_xml_map(RestXml::FlattenedXmlMapRequest.new)
check(flat.my_map == {'k' => RestXml::FooEnum::BAZ, 'j' => RestXml::FooEnum::ZERO}, 'flattened map response')
xml = client(RestXml, :RestXmlClient) do
  r::HttpResponse.new(200, body: '<x:XmlTimestampsResponse xmlns:x="urn:x"><x:epochSeconds>1.5</x:epochSeconds>' \
    '<x:httpDate>Tue, 29 Apr 2014 18:30:38 GMT</x:httpDate><x:normal>2014-04-29T18:30:38.123Z</x:normal></x:XmlTimestampsResponse>')
end
times = xml.xml_timestamps(RestXml::XmlTimestampsRequest.new)
check(times.epoch_seconds.seconds == 1 && times.epoch_seconds.nanoseconds == 500_000_000, 'epoch seconds response')
check(times.http_date.seconds == 1_398_796_238 && times.normal.nanoseconds == 123_000_000, 'text timestamps')

errors = [
  [400, {}, '<ErrorResponse><Error><Type>Sender</Type><Code>InvalidGreeting</Code><Message>Hi</Message></Error><RequestId>foo-id</RequestId></ErrorResponse>',
   ->(error) { error.is_a?(RestXml::InvalidGreeting) && error.message_value == 'Hi' && error.metadata.message == 'Hi' && error.metadata.request_id == 'foo-id' && error.metadata.status == 400 }],
  [403, {'X-Header' => 'Header', 'x-amz-request-id' => 'header-id'},
   '<ErrorResponse><Error><Code>ComplexError</Code><TopLevel>Top</TopLevel><Nested><Foo>bar</Foo></Nested></Error><RequestId>body-id</RequestId></ErrorResponse>',
   ->(error) { error.is_a?(RestXml::ComplexError) && error.header == 'Header' && error.top_level == 'Top' && error.nested.foo == 'bar' && error.metadata.request_id == 'header-id' }],
  [503, {}, '<ErrorResponse><Error><Code>SlowDown</Code><Message>later</Message></Error></ErrorResponse>',
   ->(error) { error.instance_of?(r::ServiceError) && error.metadata.name == 'SlowDown' && error.metadata.message == 'later' }],
]
errors.each do |status, headers, body, expected|
  xml = client(RestXml, :RestXmlClient) { r::HttpResponse.new(status, headers: headers, body: body) }
  begin
    xml.greeting_with_errors
    raise 'Expected restXml error'
  rescue r::ServiceError => error
    check(expected.call(error), "restXml error #{body}")
  end
end
unwrapped = client(NoWrap, :RestXmlNoErrorWrappingClient) do
  NoWrap::Runtime::HttpResponse.new(400, body: '<Error><Type>Sender</Type><Code>InvalidGreeting</Code><Message>Hi</Message><RequestId>foo-id</RequestId></Error>')
end
begin
  unwrapped.greeting_with_errors
  raise 'Expected unwrapped error'
rescue NoWrap::InvalidGreeting => error
  check(error.message_value == 'Hi' && error.metadata.request_id == 'foo-id', 'unwrapped error')
end

# External entities and entity expansion are refused before any are resolved.
['<?xml version="1.0"?><!DOCTYPE r [<!ENTITY x SYSTEM "file:///etc/passwd">]><SimpleScalarPropertiesResponse><stringValue>&x;</stringValue></SimpleScalarPropertiesResponse>',
 '<!DOCTYPE r [<!ENTITY a "aaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;">]><SimpleScalarPropertiesResponse><stringValue>&b;</stringValue></SimpleScalarPropertiesResponse>',
 '<SimpleScalarPropertiesResponse><stringValue>&undeclared;</stringValue></SimpleScalarPropertiesResponse>',
 '<SimpleScalarPropertiesResponse><stringValue>open</stringValue>',
 '<SimpleScalarPropertiesResponse test="&undeclared;"/>',
 '<a/><b/>',
 '<r>' + '<a>' * 300 + '</a>' * 300 + '</r>'].each do |body|
  xml = client(RestXml, :RestXmlClient) { r::HttpResponse.new(200, body: body) }
  begin
    xml.simple_scalar_properties(RestXml::SimpleScalarPropertiesRequest.new)
    raise "Hostile XML accepted: #{body}"
  rescue ::ArgumentError
    # Refused.
  end
end
begin
  client(RestXml, :RestXmlClient) { raise 'not sent' }.simple_scalar_properties(RestXml::SimpleScalarPropertiesRequest.new(string_value: "bad \u0001"))
  raise 'Unrepresentable text accepted'
rescue ::ArgumentError
  # Refused.
end

q = Query::Runtime
query = client(Query, :AwsQueryClient) do |request|
  check(request.method == 'POST' && URI.parse(request.url).path == '/' && request.headers['content-type'] == 'application/x-www-form-urlencoded', 'awsQuery route')
  check(request.body.start_with?('Action=QueryMaps&Version=2020-01-08&') && request.body.include?('v%20w'), "awsQuery form #{request.body}")
  check(form(request.body) == {'Action' => 'QueryMaps', 'Version' => '2020-01-08', 'MapArg.entry.1.key' => 'k', 'MapArg.entry.1.value' => 'v w',
    'Hi.1.K' => 'a', 'Hi.1.V' => 'A', 'MapOfLists.entry.1.key' => 'l', 'MapOfLists.entry.1.value.member.1' => 'x'}, "awsQuery maps #{request.body}")
  q::HttpResponse.new(200, body: '<QueryMapsResponse xmlns="https://example.com/"><QueryMapsResult/></QueryMapsResponse>')
end
query.query_maps(Query::QueryMapsInput.new(map_arg: {'k' => 'v w'}, flattened_map_with_xml_name: {'a' => 'A'}, map_of_lists: {'l' => ['x']}))
query = client(Query, :AwsQueryClient) do |request|
  check(form(request.body) == {'Action' => 'QueryLists', 'Version' => '2020-01-08', 'ListArg' => '', 'ComplexListArg.member.1.hi' => 'h',
    'FlattenedListArg.1' => 'f', 'ListArgWithXmlNameMember.item.1' => 'i'}, "awsQuery lists #{request.body}")
  q::HttpResponse.new(200)
end
query.query_lists(Query::QueryListsInput.new(list_arg: [], complex_list_arg: [Query::GreetingStruct.new(hi: 'h')], flattened_list_arg: ['f'], list_arg_with_xml_name_member: ['i']))
query = client(Query, :AwsQueryClient) do |request|
  check(request.body == 'Action=QueryTimestamps&Version=2020-01-08&normalFormat=2015-01-25T08%3A00%3A00.5Z&epochMember=1422172800.5', "timestamps #{request.body}")
  q::HttpResponse.new(200)
end
query.query_timestamps(Query::QueryTimestampsInput.new(normal_format: q::Timestamp.new(1_422_172_800, 500_000_000), epoch_member: q::Timestamp.new(1_422_172_800, 500_000_000)))
query = client(Query, :AwsQueryClient) do
  q::HttpResponse.new(402, body: '<ErrorResponse><Error><Type>Sender</Type><Code>Customized</Code><Message>Hi</Message></Error><RequestId>foo-id</RequestId></ErrorResponse>')
end
begin
  query.greeting_with_errors
  raise 'Expected custom code error'
rescue Query::CustomCodeError => error
  check(error.metadata.name == 'Customized' && error.message_value == 'Hi' && error.metadata.request_id == 'foo-id' && error.metadata.status == 402, 'awsQueryError code')
end

e = Ec2::Runtime
ec2 = client(Ec2, :AwsEc2Client) do |request|
  check(form(request.body) == {'Action' => 'SimpleInputParams', 'Version' => '2020-01-08', 'LowerName' => 'l', 'UsesXmlName' => 'u',
    'HasQueryAndXmlName' => 'q', 'Qux' => 'dmFsdWU=', 'Boo' => 'NaN'}, "ec2 names #{request.body}")
  e::HttpResponse.new(200)
end
ec2.simple_input_params(Ec2::SimpleInputParamsInput.new(lower_name: 'l', uses_xml_name: 'u', has_query_and_xml_name: 'q', qux: 'value'.b, boo: Float::NAN))
ec2 = client(Ec2, :AwsEc2Client) do |request|
  check(form(request.body) == {'Action' => 'QueryLists', 'Version' => '2020-01-08', 'ComplexListArg.1.Hi' => 'h', 'Hi.1' => 'a', 'Hi.2' => 'b',
    'NestedWithList.ListArg.1' => 'n'}, "ec2 lists #{request.body}")
  e::HttpResponse.new(200)
end
ec2.query_lists(Ec2::QueryListsInput.new(list_arg: [], complex_list_arg: [Ec2::GreetingStruct.new(hi: 'h')], list_arg_with_xml_name: %w[a b],
  nested_with_list: Ec2::NestedStructWithList.new(list_arg: ['n'])))

# One exchange per protocol over loopback HTTP, through the default Net::HTTP transport.
server = Loopback.new([
  [200, {'X-Foo' => 'Bar', 'Content-Type' => 'application/xml'}, '<SimpleScalarPropertiesResponse><stringValue>over http</stringValue></SimpleScalarPropertiesResponse>'],
  [200, {'Content-Type' => 'text/xml'}, '<SimpleScalarXmlPropertiesResponse xmlns="https://example.com/"><SimpleScalarXmlPropertiesResult>' \
    '<stringValue>s</stringValue><emptyStringValue/><integerValue>3</integerValue></SimpleScalarXmlPropertiesResult>' \
    '<ResponseMetadata><RequestId>r</RequestId></ResponseMetadata></SimpleScalarXmlPropertiesResponse>'],
  [400, {}, '<Response><Errors><Error><Code>InvalidGreeting</Code><Message>Hi</Message></Error></Errors><RequestID>foo-id</RequestID></Response>'],
  [200, {}, '<XmlListsResponse xmlns="https://example.com/"><stringList><member>a</member></stringList>' \
    '<flattenedList>x</flattenedList><flattenedList>y</flattenedList><requestId>r</requestId></XmlListsResponse>'],
])
real = RestXml::RestXmlClient.new(r::ClientConfig.new(endpoint: server.url, max_attempts: 1))
check(real.simple_scalar_properties(RestXml::SimpleScalarPropertiesRequest.new(foo: 'Foo', integer_value: 3)).string_value == 'over http', 'restXml over HTTP')
real_query = Query::AwsQueryClient.new(q::ClientConfig.new(endpoint: server.url, max_attempts: 1))
scalars = real_query.simple_scalar_xml_properties
check(scalars.string_value == 's' && scalars.empty_string_value == '' && scalars.integer_value == 3 && scalars.long_value.equal?(q::UNSET), 'awsQuery result wrapper over HTTP')
real_ec2 = Ec2::AwsEc2Client.new(e::ClientConfig.new(endpoint: server.url, max_attempts: 1))
begin
  real_ec2.greeting_with_errors
  raise 'Expected ec2 error'
rescue Ec2::InvalidGreeting => error
  check(error.message_value == 'Hi' && error.metadata.request_id == 'foo-id' && error.metadata.name == 'InvalidGreeting', 'ec2 error envelope over HTTP')
end
ec2_lists = real_ec2.xml_lists
check(ec2_lists.string_list == ['a'] && ec2_lists.flattened_list == %w[x y], 'ec2 response without wrapper over HTTP')
server.join
head, body = server.requests[0]
check(head.start_with?('PUT /SimpleScalarProperties ') && head =~ /x-foo: Foo/i && head =~ %r{content-type: application/xml}i, 'restXml request over HTTP')
check(body == '<SimpleScalarPropertiesRequest><integerValue>3</integerValue></SimpleScalarPropertiesRequest>', "restXml body over HTTP #{body}")
head, body = server.requests[1]
check(head.start_with?('POST / ') && head =~ %r{content-type: application/x-www-form-urlencoded}i && body == 'Action=SimpleScalarXmlProperties&Version=2020-01-08', 'awsQuery request over HTTP')
check(server.requests[2][1] == 'Action=GreetingWithErrors&Version=2020-01-08', 'ec2Query request over HTTP')
puts 'Ruby restXml, awsQuery, and ec2Query protocols passed'

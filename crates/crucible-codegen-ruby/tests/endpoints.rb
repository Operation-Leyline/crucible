# frozen_string_literal: true
require 'behavior_client'
R = Behavior::Runtime
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
class Capture
  attr_reader :requests
  def initialize = @requests = []
  def send_request(request, read_body:, scope:)
    @requests << request
    R::HttpResponse.new(200, body: '{"wire-status":"READY"}')
  end
end
capture = Capture.new
credentials = -> { R::AwsCredentials.new(access_key_id: 'key', secret_access_key: 'secret') }
config = {transport: capture, region: 'local', credentials: credentials}
client = Behavior::ServiceClient.new(R::ClientConfig.new(**config))
output = client.read(Behavior::Input.new(account: 'customer'))
request = capture.requests.last
check(output.status == 'READY' && URI.parse(request.url).host == 'customer.local.example.test', 'Resolved endpoint with host prefix')
check(request.headers['x-region'] == 'local', 'Endpoint headers')
check(request.headers['authorization'].include?('/local/example/aws4_request') && request.headers['x-amz-content-sha256'] == 'UNSIGNED-PAYLOAD', 'Endpoint signing properties')
['a.b', '-bad', 'x/y', 'x@evil', '', 'x' * 64].each do |label|
  error = rejects(R::EndpointError) { client.read(Behavior::Input.new(account: label)) }
  check(error.message.include?('host label'), 'Host label diagnostic')
end
check(capture.requests.size == 1, 'Invalid host labels are never dispatched')
check(client.wait_until_ready(Behavior::Input.new(account: 'customer'), timeout: 1).status == 'READY', 'Waiter uses resolved endpoint')
client.public
check(URI.parse(capture.requests.last.url).host == 'local.example.test' && !capture.requests.last.headers.key?('authorization'), 'Anonymous operation without host prefix')
client = Behavior::ServiceClient.new(R::ClientConfig.new(**config, disable_host_prefix: true))
client.read(Behavior::Input.new(account: 'ignored'))
check(URI.parse(capture.requests.last.url).host == 'local.example.test', 'Host prefix can be disabled')
client = Behavior::ServiceClient.new(R::ClientConfig.new(**config, region: 'missing'))
check(rejects(R::EndpointError) { client.read(Behavior::Input.new(account: 'customer')) }.message.include?('Unknown region missing'), 'Endpoint error rule')
client = Behavior::ServiceClient.new(R::ClientConfig.new(**config, region: nil))
check(rejects(R::EndpointError) { client.read(Behavior::Input.new(account: 'customer')) }.message.include?('missing'), 'Required endpoint parameter')
client = Behavior::ServiceClient.new(R::ClientConfig.new(**config, headers: {'authorization' => 'AWS4-HMAC-SHA256 supplied'}, credentials: -> { raise 'Provider despite supplied auth' }))
client.read(Behavior::Input.new(account: 'customer'))
check(capture.requests.last.headers['authorization'] == 'AWS4-HMAC-SHA256 supplied', 'Caller-supplied authorization bypasses the provider')

ref = ->(name) { {'kind' => 'reference', 'name' => name} }
text = ->(value) { {'kind' => 'string', 'parts' => [{'kind' => 'literal', 'value' => value}]} }
rules = {'parameters' => {'Name' => {'kind' => 'string', 'required' => true}, 'Flag' => {'kind' => 'boolean', 'required' => true}},
         'rules' => [{'conditions' => [{'function' => 'isSet', 'arguments' => [ref.('Flag')], 'assign' => nil}],
                      'result' => {'kind' => 'endpoint', 'url' => text.('https://example.test/base'), 'headers' => {'name' => [ref.('Name')]}, 'properties' => {'flag' => ref.('Flag')}}}]}
sources = {'Name' => [{'kind' => 'inputMember', 'member' => 'name'}, {'kind' => 'client', 'name' => 'name'}, {'kind' => 'default', 'value' => 'default'}],
           'Flag' => [{'kind' => 'builtIn', 'name' => 'Flag'}]}
result = R::Endpoints.resolve(rules, sources, {'name' => 'input'}, {'name' => 'client'}, {'Flag' => false})
check(result.headers['name'] == ['input'] && result.properties['flag'] == false, 'Input parameter precedence and false booleans')
check(R::Endpoints.resolve(rules, sources, {}, {'name' => 'client'}, {'Flag' => false}).headers['name'] == ['client'], 'Client parameter')
check(R::Endpoints.resolve(rules, sources, {}, {}, {'Flag' => false}).headers['name'] == ['default'], 'Default parameter')
check(rejects(R::EndpointError) { R::Endpoints.resolve(rules, sources, {}, {}, {}) }.message.include?('missing'), 'Missing required parameter')
check(rejects(R::EndpointError) { R::Endpoints.resolve(rules, sources, {}, {}, {'Flag' => 0}) }.message.include?('wrong type'), 'Typed parameter')
tree = {'parameters' => {}, 'rules' => [{'conditions' => [], 'result' => {'kind' => 'tree', 'rules' => [
  {'conditions' => [{'function' => 'parseUrl', 'arguments' => [text.('https://[::1]:8443/p')], 'assign' => 'url'}],
   'result' => {'kind' => 'endpoint', 'url' => {'kind' => 'string', 'parts' => [{'kind' => 'literal', 'value' => 'https://'}, {'kind' => 'reference', 'name' => 'url', 'path' => [{'kind' => 'member', 'name' => 'authority'}]}]}, 'headers' => {}, 'properties' => {}}}]}}]}
check(R::Endpoints.resolve(tree, {}, {}, {}, {}).url == 'https://[::1]:8443', 'Tree rules, assignment, and attribute templates')

check(R::Endpoints.parse_url('https://example.test:443/a')['authority'] == 'example.test', 'Default port authority')
check(R::Endpoints.parse_url('https://[::1]:8443')['authority'] == '[::1]:8443' && R::Endpoints.parse_url('https://[::1]')['isIp'], 'IP authority')
check(R::Endpoints.parse_url('http://example.test/a')['normalizedPath'] == '/a/', 'Normalized path')
['file:///path', 'https://user:password@example.test', 'https://example.test?q=x', 'https://example.test#x', 'https://example.test:99999', "https://evil\n.example", 'https://bad_host.test'].each do |value|
  check(R::Endpoints.parse_url(value).nil?, 'Invalid endpoint accepted: ' + value.inspect)
end
check(R::Endpoints.function('substring', ['abcdef', 1, 3, true]) == 'de', 'Reverse substring')
check(R::Endpoints.function('substring', ['é', 0, 1, false]).nil? && R::Endpoints.function('substring', ['a', 0, 2, false]).nil?, 'Substring bounds')
check(R::Endpoints.function('uriEncode', ['a/b +é']) == 'a%2Fb%20%2B%C3%A9', 'URI encoding')
check(R::Endpoints.function('getAttr', [{'items' => ['a', 'b']}, 'items[1]']) == 'b', 'getAttr path')
check(R::Endpoints.function('isSet', [false]) && !R::Endpoints.function('isSet', [nil]), 'isSet')
check(!R::Endpoints.function('isValidHostLabel', ['x.y', false]) && R::Endpoints.function('isValidHostLabel', ['x.y', true]), 'Host labels')
check(R::Endpoints.function('booleanEquals', [true, true]) && !R::Endpoints.function('stringEquals', ['a', nil]), 'Typed equality')
rejects(R::EndpointError) { R::Endpoints.function('not', ['true']) }
rejects(R::EndpointError) { R::Endpoints.host_prefix('https://example.test', {'parts' => [{'kind' => 'literal', 'value' => 'x' * 250 + '.'}]}, {}) }
puts 'Ruby endpoint rules, host prefixes, and endpoint auth properties passed'

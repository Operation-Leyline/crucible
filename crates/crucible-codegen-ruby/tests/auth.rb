# frozen_string_literal: true
require 'auth_client'
require 'tempfile'
R = AuthSdk::Runtime
class Capture
  attr_reader :request
  def send_request(request, read_body:, scope:)
    @request = request
    R::HttpResponse.new(204)
  end
end
def check(value)
  raise 'Auth check failed' unless value
end
capture = Capture.new
client = AuthSdk::AuthClient.new(R::ClientConfig.new(endpoint: 'http://127.0.0.1',transport: capture,basic_auth: -> { R::BasicCredentials.new(username: 'user',password: 'páss') },api_key: -> { raise 'Wrong provider' }))
client.read; check(capture.request.headers['authorization'] == 'Basic ' + Base64.strict_encode64('user:páss'))
client.public; check(!capture.request.headers.key?('authorization'))
client = AuthSdk::AuthClient.new(R::ClientConfig.new(endpoint: 'http://127.0.0.1',transport: capture,api_key: -> { 'secret' }))
client.read; check(capture.request.headers['x-api-key'] == 'ApiKey secret')
client = AuthSdk::AuthClient.new(R::ClientConfig.new(endpoint: 'http://127.0.0.1',transport: capture)); client.optional
begin; client.read; raise 'Expected missing identity'; rescue ArgumentError; end
client = AuthSdk::AuthClient.new(R::ClientConfig.new(endpoint: 'http://127.0.0.1',transport: capture,headers: {'authorization'=>'Basic preset'},basic_auth: -> { raise 'Provider despite explicit auth' })); client.read
identity = R::AwsCredentials.new(access_key_id: 'AKIAIOSFODNN7EXAMPLE',secret_access_key: 'wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY')
request = R::HttpRequest.new(url: 'https://examplebucket.s3.amazonaws.com/test.txt',method: 'GET',headers: {'range'=>'bytes=0-9'})
signed = R::Authentication.sign(request,identity,service: 's3',region: 'us-east-1',now: Time.utc(2013,5,24))
check(signed.headers['authorization'].end_with?('Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41'))
client = AuthSdk::AuthClient.new(R::ClientConfig.new(endpoint: 'https://example.invalid',transport: capture,credentials: -> { identity },region: 'us-east-1',middleware: [->(r) { r.with(headers: {'x-custom'=>'signed'}) }]))
client.read; check(capture.request.headers['authorization'].include?('x-custom'))
now = Time.at(1000); calls = 0
cache = R::CachedCredentials.new(-> { calls += 1; R::AwsCredentials.new(access_key_id: 'access',secret_access_key: 'secret',expires_at: now + 10) },clock: -> { now })
threads = 8.times.map { Thread.new { cache.call } }; threads.each(&:value); check(calls == 1)
now += 6; cache.call; check(calls == 2)
Tempfile.create('crucible-profile') do |file|
  file.write("[test]\naws_access_key_id=access\naws_secret_access_key=secret\ncredential_process=must-never-run\n"); file.flush
  check(R::Credentials.profile(profile: 'test',path: file.path).access_key_id == 'access')
end
check(R::Credentials.chain(-> { nil },-> { identity }).call.equal?(identity))
check(!identity.inspect.include?('AKIA'))
puts 'Ruby authentication, public SigV4 oracle, and shared credential refresh passed'

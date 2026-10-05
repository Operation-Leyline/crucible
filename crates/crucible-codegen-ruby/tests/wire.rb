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
    raise 'media headers' unless request.headers['x-meta'] == 'eyJhIjoxfQ==, aGVsbG8=' && request.headers['x-extra-trace'] == 'aGVsbG8='
    @failure ? WireSdk::Runtime::HttpResponse.new(422, headers: {'x-crucible-error' => 'TextError'}, body: 'invalid text') : WireSdk::Runtime::HttpResponse.new(200, headers: {'x-meta' => 'eyJhIjoxfQ==, aGVsbG8=', 'x-extra-trace' => 'aGVsbG8='}, body: request.body)
  end
end
input = WireSdk::TextInput.new(text: 'héllo', meta: ['{"a":1}', 'hello'], extras: {'trace' => 'hello'})
client = WireSdk::WireClient.new(WireSdk::Runtime::ClientConfig.new(endpoint: 'http://127.0.0.1', transport: WireTransport.new(false)))
output = client.text(input)
raise 'output' unless output.text == input.text && output.meta == input.meta && output.extras == input.extras
raise 'document' unless client.document(WireSdk::DocumentInput.new(document: WireSdk::Record.new(name: 'record'))).document.name == 'record'
client = WireSdk::WireClient.new(WireSdk::Runtime::ClientConfig.new(endpoint: 'http://127.0.0.1', transport: WireTransport.new(true)))
begin
  client.text(input)
  raise 'expected TextError'
rescue WireSdk::TextError => error
  raise 'error payload' unless error.details == 'invalid text' && error.metadata.status == 422
end
puts 'Ruby payloads and media headers passed'

# frozen_string_literal: true
# Run against a gem generated from tests/models/client-docs-vault
# with module Vault: ruby -I <gem>/lib describe.rb
require 'pp'
require 'vault_client'

MARKER = '*** Sensitive Data Redacted ***'
SECRETS = %w[hunter2 4321 old-one old-two map-secret union-secret].freeze

def check(condition, message)
  raise message unless condition
end

secret = Vault::Secret.new(
  name: 'db', password: 'hunter2', pin: '4321', history: %w[old-one old-two],
  by_name: { 'k' => 'map-secret' }, aliases: %w[alias-a alias-b], labels: { 'env' => 'prod' },
  kind: Vault::Kind::LOGIN, level: Vault::Level::HIGH, credential: Vault::Credential::PasswordVariant.new('union-secret'),
  created_at: Vault::Runtime::Timestamp.new(0), payload: "\x01\x02\x03".b
)

text = secret.to_s
check(text.start_with?('Secret(name=db, '), "type and first member: #{text}")
check(text.include?('aliases=[alias-a, alias-b]'), "lists recurse: #{text}")
check(text.include?('labels={env=prod}'), "maps recurse: #{text}")
check(text.include?('kind=Kind(login)'), "enums: #{text}")
check(text.include?('level=Level(2)'), "int enums: #{text}")
check(text.include?('created_at=1970-01-01T00:00:00Z'), "timestamps: #{text}")
check(text.include?('payload=<3 bytes>'), "blobs show their length: #{text}")
check(!text.include?('revision'), "absent members are omitted: #{text}")
check(secret.inspect == text, 'inspect and to_s agree')

# Every inspection path hides sensitive values, not just to_s.
outputs = [text, secret.inspect, "#{secret}", [secret].inspect, { a: secret }.inspect, secret.pretty_inspect]
outputs.each do |output|
  SECRETS.each { |leaked| check(!output.include?(leaked), "#{leaked} leaked in: #{output}") }
end
%w[password pin history by_name].each { |member| check(text.include?("#{member}=#{MARKER}"), "#{member} redacted: #{text}") }
check(text.include?("credential=Credential(password=#{MARKER})"), "union member: #{text}")

check(Vault::Credential::PinVariant.new('9999').to_s == "Credential(pin=#{MARKER})", 'sensitive union member')
check(Vault::Credential::UsernameVariant.new('bob').to_s == 'Credential(username=bob)', 'plain union member')
unknown = Vault::Credential::Unknown.new('future', Vault::Runtime::JsonValue.parse('"token-123"'))
check(unknown.to_s == 'Credential(unknown_variant=future)', 'unknown variant omits its value')
check(Vault::Secret.new(name: 'x').to_s == 'Secret(name=x)', 'minimal model')

error = Vault::AccessDenied.new(
  message_value: 'denied', reason: 'secret-reason',
  metadata: Vault::Runtime::ErrorMetadata.new(name: 'AccessDenied', message: 'denied', status: 403, request_id: 'r-1', headers: { 'authorization' => 'x' })
)
described = error.inspect
check(described.include?("reason=#{MARKER}") && !described.include?('secret-reason'), "error members redact: #{described}")
check(described.include?('metadata=ErrorMetadata(name=AccessDenied, status=403, request_id=r-1)'), "error metadata: #{described}")
check(!described.include?('authorization'), 'headers are omitted')
check(!error.pretty_inspect.include?('authorization') && !error.pretty_inspect.include?('secret-reason'), 'pp hides headers and sensitive members')
check(error.message == 'denied', 'an error keeps its message')
puts 'Ruby string representations passed'

# frozen_string_literal: true
require 'weather_client'
client = Weather::Client.new(Weather::Runtime::ClientConfig.new(endpoint: 'https://example.invalid', timeout: 2, max_attempts: 1))
begin
  client.ping
  raise 'Proxy CONNECT was mistaken for an API success'
rescue Weather::Runtime::NetworkError
  puts 'Ruby proxy CONNECT isolation passed'
ensure
  client.close
end

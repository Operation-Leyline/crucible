# frozen_string_literal: true
require 'socket'
require 'waiter_json_client'

def check(value, message)
  raise message unless value
end

class IdentityLoopback
  attr_reader :url, :calls
  def initialize(replies)
    @replies, @calls = replies, 0
    @server = TCPServer.new('127.0.0.1', 0)
    @url = "http://127.0.0.1:#{@server.addr[1]}"
    @thread = Thread.new do
      loop do
        socket = @server.accept
        begin
          head = +''
          head << socket.readpartial(1) until head.end_with?("\r\n\r\n")
          length = head[/content-length: *(\d+)/i, 1].to_i
          socket.read(length) if length.positive?
          reply = @replies.fetch(@calls)
          @calls += 1
          status, content_type, body = reply
          socket.write("HTTP/1.1 #{status} X\r\nContent-Type: #{content_type}\r\nContent-Length: #{body.bytesize}\r\nConnection: close\r\n\r\n" + body)
        ensure
          socket.close
        end
      end
    rescue IOError, Errno::EBADF => error
      @failure = error unless @server.closed?
    rescue StandardError => error
      @failure = error
    end
  end

  def close
    @server.close
    @thread.join(5)
    check(!@thread.alive?, 'loopback thread did not stop')
    raise @failure if @failure
  end
end

def exchange(sdk, client_name, replies, expected_calls)
  server = IdentityLoopback.new(replies)
  client = sdk.const_get(client_name).new(sdk::Runtime::ClientConfig.new(endpoint: server.url, max_attempts: 1, timeout: 5))
  yield client
  check(server.calls == expected_calls, "expected #{expected_calls} requests, received #{server.calls}")
ensure
  client&.close
  server&.close
end

def rejects(type)
  yield
rescue type => error
  error
else
  raise "Expected #{type}"
end

options = {timeout: 5, min_delay: 0, max_delay: 0}
options = {timeout: 5, min_delay: 0, max_delay: 0}
renamed = [400, 'application/json', '{"__type":"OriginalError","value":7}']
exchange(WaiterJson, :JsonIdentityClient, [renamed], 1) do |client|
  error = rejects(WaiterJson::Runtime::ServiceError) { client.json_read }
  check(error.is_a?(WaiterJson::RenamedError) && error.metadata.name == 'OriginalError' && error.value == 7, 'renamed public class preserves original error name')
end
%i[wait_until_by_absolute wait_until_by_relative].each do |waiter|
  exchange(WaiterJson, :JsonIdentityClient, [renamed, [200, 'application/json', '']], 2) do |client|
    client.public_send(waiter, **options)
  end
end
puts 'Ruby generated waiter error identity passed over HTTP'

# frozen_string_literal: true
require 'error_docs_client'
require 'json'
require 'socket'

cases = JSON.parse(File.read(ARGV.fetch(0))).fetch('cases')
server = TCPServer.new('127.0.0.1', 0)
serving = Thread.new do
  cases.each do |test|
    socket = server.accept
    begin
      request = +''
      while (line = socket.gets) && line != "\r\n"
        request << line
        raise 'Request header bound' if request.bytesize > 65_536
      end
      raise 'Expected native GET' unless request.start_with?('GET /inspect HTTP/1.1')
      headers = "HTTP/1.1 #{test.fetch('status')} Fixture\r\nConnection: close\r\nContent-Length: #{test.fetch('body').bytesize}\r\nx-request-id: documentation-consumer\r\n"
      headers << "x-crucible-error: #{test['identity']}\r\n" unless test['identity'].nil?
      headers << "X-Error-Reason: #{test['header']}\r\n" unless test['header'].nil?
      socket.write(headers + "\r\n" + test.fetch('body'))
    ensure
      socket.close
    end
  end
end
client = ErrorDocs::TroubleshootingClient.new(ErrorDocs::Runtime::ClientConfig.new(
  endpoint: "http://127.0.0.1:#{server.addr[1]}", max_attempts: 1, timeout: 5))
last = nil
begin
  cases.each do |test|
    begin
      client.inspect_value
      raise 'Expected modeled service failure'
    rescue ErrorDocs::Runtime::ServiceError => error
      typed = error.class != ErrorDocs::Runtime::TroubleshootingException
      unless error.doc_url == test['docUrl'] && error.metadata.doc_url == test['docUrl'] && typed == test['typed'] &&
          error.metadata.status == test['status'] && error.metadata.request_id == 'documentation-consumer'
        raise "#{test.fetch('name')}: URL=#{error.doc_url.inspect}, type=#{error.class}"
      end
      raise 'Static URL must be immutable' if error.doc_url && !error.doc_url.frozen?
      last = error
      if %w[document-empty document-whitespace document-null].include?(test['name'])
        absent = test['name'] != 'document-null'
        raise "#{test['name']}: document presence" unless error.data.equal?(ErrorDocs::Runtime::UNSET) == absent && (absent || error.data.nil?)
      end
    end
  end
  client.close
  raise 'URL lifetime' unless last.doc_url == cases.last['docUrl']
  serving.value
ensure
  client.close
  if serving.alive?
    serving.kill
    serving.join
  end
  server.close
end
puts "Ruby error documentation: #{cases.size} installed gem/native HTTP cases passed"

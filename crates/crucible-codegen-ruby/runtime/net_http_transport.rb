# frozen_string_literal: true
require 'net/http'
require 'uri'
module CrucibleGenerated
  module Runtime
    class NetHttpTransport
      def initialize
        @mutex, @http, @origin, @closed = ::Mutex.new, nil, nil, false
      end
      def close
        @mutex.synchronize do
          @closed = true
          disconnect
        end
      end
      def send_request(request, read_body:, scope:)
        scope.check
        scope.wait(0.01) until @mutex.try_lock
        worker = nil
        begin
          raise IOError, 'Transport is closed' if @closed
          # Only owned network work runs here. Killing it never interrupts caller hooks.
          worker = ::Thread.new do
            ::Thread.current.report_on_exception = false
            completed = false
            begin
              response = perform(request, read_body, scope.remaining)
              completed = true
              response
            ensure
              disconnect unless completed
            end
          end
          until worker.join([0.025, scope.remaining].min)
            scope.check
          end
          scope.check
          worker.value
        ensure
          if worker&.alive?
            worker.kill
            worker.join
          end
          @mutex.unlock
        end
      end
      private
      def disconnect
        @http.finish if @http&.started?
      rescue IOError, SystemCallError
        nil
      ensure
        @http = @origin = nil
      end
      def perform(request, read_body, timeout)
        uri = ::URI.parse(request.url)
        origin = [uri.scheme, uri.host, uri.port]
        if @origin != origin
          disconnect
          @http = ::Net::HTTP.new(uri.hostname, uri.port)
          @http.use_ssl = uri.scheme == 'https'
          @http.max_retries = 0
          @origin = origin
        end
        @http.open_timeout = @http.read_timeout = @http.write_timeout = timeout
        @http.start unless @http.started?
        headers = request.headers.merge('accept-encoding' => 'identity')
        message = ::Net::HTTPGenericRequest.new(request.method, !request.body.nil?, request.method != 'HEAD', uri.request_uri, headers)
        message.body = request.body unless request.body.nil?
        discarded = false
        result = catch(:irrelevant_body) do
          decoded = nil
          @http.request(message) do |response|
            status, headers = response.code.to_i, response.to_hash.transform_values { |values| values.join(', ') }
            if (200...300).cover?(status) && !read_body
              # Breaking out prevents Net::HTTP from draining a body the model cannot use.
              discarded = true
              throw :irrelevant_body, HttpResponse.new(status, headers: headers)
            end
            body = +''.b
            response.read_body { |chunk| body << chunk }
            length = response['content-length']
            if request.method != 'HEAD' && status >= 200 && ![204, 304].include?(status) && length && !response['transfer-encoding'] && body.bytesize != ::Kernel.Integer(length, 10)
              raise EOFError, 'Truncated response body'
            end
            decoded = HttpResponse.new(status, headers: headers, body: body)
          end
          decoded
        end
        disconnect if discarded
        result
      rescue ::Net::OpenTimeout, ::Net::ReadTimeout, ::Net::WriteTimeout => error
        raise DeadlineError, error.message
      rescue ::IOError, ::EOFError, ::SystemCallError, ::SocketError, ::OpenSSL::SSL::SSLError, ::Net::HTTPBadResponse, ::Net::ProtocolError => error
        raise NetworkError, error.message
      end
    end
  end
end

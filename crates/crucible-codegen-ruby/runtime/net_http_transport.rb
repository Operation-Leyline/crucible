# frozen_string_literal: true
require 'net/http'
require 'uri'
module CrucibleGenerated
  module Runtime
    # The default transport, built on `Net::HTTP`: keeps one connection open
    # between calls, never follows redirects, and honors the call deadline and
    # cancellation. Implement `send_request(request, read_body:, scope:, sink: nil)`
    # and `close` to plug in a different HTTP stack.
    class NetHttpTransport
      def initialize
        @mutex, @http, @origin, @closed = ::Mutex.new, nil, nil, false
      end
      # Closes the connection; later calls fail.
      def close
        @mutex.synchronize do
          @closed = true
          disconnect
        end
      end
      # A sink receives a successful body in chunks on the calling thread; a bounded
      # queue between the network worker and the sink keeps memory flat and applies backpressure.
      def send_request(request, read_body:, scope:, sink: nil)
        scope.check
        scope.wait(0.01) until @mutex.try_lock
        worker = nil
        begin
          raise ::IOError, 'Transport is closed' if @closed
          chunks = sink && ::SizedQueue.new(4)
          # Only owned network work runs here. Killing it never interrupts caller hooks.
          worker = ::Thread.new do
            ::Thread.current.report_on_exception = false
            completed = false
            begin
              response = perform(request, read_body, scope.remaining, chunks)
              completed = true
              response
            ensure
              disconnect unless completed
            end
          end
          loop do
            finished = worker.join(chunks ? 0 : [0.025, scope.remaining].min)
            if chunks
              chunk = chunks.pop(timeout: finished ? 0 : [0.025, scope.remaining].min)
              scope.check
              if chunk
                sink.call(chunk)
                next
              end
            else
              scope.check
            end
            break if finished
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
      rescue ::IOError, ::SystemCallError
        nil
      ensure
        @http = @origin = nil
      end
      def perform(request, read_body, timeout, chunks = nil)
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
        if request.body.is_a?(StreamBody)
          message.body_stream = request.body
        elsif !request.body.nil?
          message.body = request.body
        end
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
            length = response['content-length']
            raise ::EOFError, 'Invalid Content-Length' if length && !/\A[0-9]+\z/.match?(length)
            expected = length && ::Kernel.Integer(length, 10)
            body, received = +''.b, 0
            stream = chunks && (200...300).cover?(status)
            response.read_body do |chunk|
              received += chunk.bytesize
              stream ? chunks.push(chunk.b) : body << chunk
            end
            if request.method != 'HEAD' && status >= 200 && ![204, 304].include?(status) && expected && !response['transfer-encoding'] && received != expected
              raise ::EOFError, 'Truncated response body'
            end
            decoded = HttpResponse.new(status, headers: headers, body: body)
          end
          decoded
        end
        disconnect if discarded
        result
      rescue ::Net::OpenTimeout, ::Net::ReadTimeout, ::Net::WriteTimeout => error
        raise DeadlineError, error.message
      rescue ::IOError, ::EOFError, ::SystemCallError, ::SocketError, ::OpenSSL::SSL::SSLError, ::Net::HTTPBadResponse, ::Net::HTTPHeaderSyntaxError, ::Net::ProtocolError => error
        raise NetworkError, error.message
      end
    end
  end
end

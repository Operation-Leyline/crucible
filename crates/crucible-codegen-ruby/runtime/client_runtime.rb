# frozen_string_literal: true
require 'securerandom'
require 'digest'
module CrucibleGenerated
  module Runtime
    class ClientRuntime
      def initialize(config, codec, endpoint:, digest:)
        @config, @codec, @digest = config, codec, digest
        @endpoint = (config.endpoint || endpoint).sub(%r{/+\z}, '')
        uri = self.class.url(@endpoint)
        raise ::ArgumentError, 'Endpoint cannot contain query or fragment' if uri.query || uri.fragment
        @transport, @closed = config.transport || NetHttpTransport.new, false
      end
      def close
        @transport.close unless @closed || @config.transport
        @closed = true
      end
      def call(name, input, cancellation: nil)
        scope = CallScope.new(@config.timeout, cancellation)
        scope.check
        op = operation(name)
        values = op['input'] ? @codec.encode(input, @codec.context(op['input'])) : {}
        result = send_request(op, prepare(op, values), scope)
        output = op['output'] ? @codec.decode(result, @codec.context(op['output'])) : Unit.new
        scope.check
        output
      end
      def pages(name, input, cancellation: nil)
        op = operation(name)
        initial = @codec.encode(input, @codec.context(op.fetch('input')))
        ::Enumerator.new do |yielder|
          values = initial.dup
          pagination = op.fetch('pagination')
          token_name = pagination.fetch('input')
          seen = {}
          seen[values[token_name]] = true if values[token_name] && values[token_name] != ''
          loop do
            scope = CallScope.new(@config.timeout, cancellation)
            scope.check
            result = send_request(op, prepare(op, values), scope)
            output = @codec.decode(result, @codec.context(op.fetch('output')))
            scope.check
            yielder << output
            token = pagination.fetch('output').reduce(result) { |value, part| value.is_a?(::Hash) ? value[part] : nil }
            break if token.nil? || token == ''
            token = WireCodec.string(token)
            raise ::ArgumentError, 'Pagination token cycle' if seen[token]
            seen[token] = true
            values[token_name] = token
          end
        end
      end
      def self.url(text)
        uri = ::URI.parse(text)
        unless %w[https http].include?(uri.scheme) && uri.host && !uri.host.empty? && !uri.userinfo && !/[\x00-\x20\x7f]/.match?(text)
          raise ::ArgumentError, 'Absolute HTTP(S) URL without credentials required'
        end
        uri
      rescue ::URI::InvalidURIError => error
        raise ::ArgumentError, error.message
      end
      private
      def operation(name) = @codec.schema.fetch('operations').fetch(name)
      def escape(text)
        WireCodec.string(text).b.bytes.map { |byte| (65..90).cover?(byte) || (97..122).cover?(byte) || (48..57).cover?(byte) || [45,46,95,126].include?(byte) ? byte.chr : '%%%02X' % byte }.join
      end
      def prepare(op, input)
        values = input.dup
        token = op['token']
        values[token] = ::SecureRandom.uuid if token && values[token].nil?
        bindings = op.fetch('request')
        path = op.fetch('uri').dup
        bindings.fetch('labels').each do |name|
          text = @codec.bound(values.fetch(name))
          raise ::ArgumentError, 'Empty label: ' + name if text.empty?
          path = path.gsub('{' + name + '}', escape(text))
        end
        query = []
        bindings.fetch('query').each do |member, name|
          next if values[member].nil?
          (values[member].is_a?(::Array) ? values[member] : [values[member]]).each { |item| query << escape(name) + '=' + escape(@codec.bound(item)) }
        end
        path += (path.include?('?') ? '&' : '?') + query.join('&') unless query.empty?
        headers = @config.headers.to_h { |key, value| [key.downcase, value] }
        bindings.fetch('headers').each { |member, name| headers[name.downcase] = @codec.bound(values[member]) unless values[member].nil? }
        body = nil
        unless bindings.fetch('body').empty?
          object = bindings.fetch('body').filter_map { |member| [member, values[member]] if values.key?(member) }.to_h
          body, headers['content-type'] = ExactJson.encode(object), 'application/json'
        end
        payload = bindings['payload']
        if payload && values[payload[0]]
          bytes = ::Base64.strict_decode64(values[payload[0]])
          body, headers['content-type'] = bytes, payload[1] unless bytes.empty?
        end
        HttpRequest.new(url: @endpoint + path, method: op.fetch('method'), headers: headers, body: body)
      end
      def send_request(op, prepared, scope)
        raise ::IOError, 'Client is closed' if @closed
        safe = op.fetch('safe')
        (1..@config.max_attempts).each do |attempt|
          scope.check
          request = prepared
          if @codec.schema.fetch('bearer') && @config.token
            token = @config.token.call
            scope.check
            request = request.with(headers: request.headers.merge('authorization' => 'Bearer ' + WireCodec.string(token))) unless token.nil?
          end
          @config.middleware.each do |middleware|
            scope.check
            request = middleware.call(request)
            raise ::ArgumentError, 'Middleware must return HttpRequest' unless request.is_a?(HttpRequest)
          end
          scope.check
          uri = self.class.url(request.url)
          if request.headers.key?('authorization') && uri.scheme != 'https' && !%w[localhost 127.0.0.1 ::1].include?(uri.hostname.downcase)
            raise ::ArgumentError, 'Credentials require HTTPS or loopback HTTP'
          end
          if @digest
            headers = request.headers.reject { |name, _| name == 'x-amz-content-sha256' }
            headers['x-amz-content-sha256'] = ::Digest::SHA256.hexdigest(request.body) if request.body && !request.body.empty?
            request = request.with(headers: headers)
          end
          bindings = op.fetch('response')
          begin
            response = @transport.send_request(request, read_body: !bindings.fetch('body').empty? || !bindings['payload'].nil?, scope: scope)
          rescue NetworkError
            scope.check
            raise unless safe && attempt < @config.max_attempts
            scope.wait(retry_delay(attempt, nil))
            next
          end
          scope.check
          return success(op, response) if (200...300).cover?(response.status)
          error = failure(op, response)
          eligible = error.metadata.throttling || (safe && (error.metadata.retryable || [500,502,503,504].include?(response.status)))
          raise error unless eligible && attempt < @config.max_attempts
          scope.wait(retry_delay(attempt, response.headers['retry-after']))
        end
        raise 'No request attempt'
      end
      def success(op, response)
        bindings = op.fetch('response')
        payload = bindings['payload']
        body = if payload
          {payload[0] => ::Base64.strict_encode64(response.body)}
        elsif bindings.fetch('body').empty? || response.body.empty?
          {}
        else WireCodec.object(ExactJson.decode(response.body))
        end
        op['output'] ? apply_headers(op['output'], bindings.fetch('headers'), response, body) : body
      end
      def apply_headers(target, bindings, response, body)
        object = body.dup
        members = @codec.shape(target).fetch('members')
        bindings.each do |member, name|
          object[member] = @codec.header(members.fetch(member), response.headers[name.downcase]) if response.headers.key?(name.downcase)
        end
        object
      end
      def failure(op, response)
        begin
          body = WireCodec.object(ExactJson.decode(response.body))
        rescue ::ArgumentError
          body = {}
        end
        name = body['__type'].is_a?(::String) ? body['__type'] : nil
        matches = op.fetch('errors').select do |target|
          shape = @codec.shape(target)
          name ? shape.fetch('name') == name : shape.fetch('error').fetch('status') == response.status
        end
        target = matches.size == 1 ? matches.first : nil
        shape = target && @codec.shape(target)
        traits = shape && shape.fetch('error')
        metadata = ErrorMetadata.new(name: name || shape&.fetch('name') || 'UnknownError',
          message: body['message'].is_a?(::String) ? body['message'] : 'HTTP ' + response.status.to_s,
          status: response.status, request_id: response.headers['x-request-id'] || (body['requestId'] if body['requestId'].is_a?(::String)),
          headers: response.headers, retryable: traits&.fetch('retryable', false) == true, throttling: traits&.fetch('throttling', false) == true)
        if target
          begin
            object = apply_headers(target, traits.fetch('headers'), response, body)
            return @codec.error(target, object, metadata)
          rescue ::ArgumentError, ::TypeError
            # Keep wire metadata when modeled members cannot be decoded.
          end
        end
        ServiceError.new(metadata)
      end
      def retry_delay(attempt, header)
        if header
          begin
            number = ::Kernel.Float(header)
            return number if number.finite? && number >= 0
          rescue ::ArgumentError
            begin
              return [0.0, Timestamp.from_http_date(header).seconds - ::Time.now.to_f].max
            rescue ::ArgumentError
              # Invalid Retry-After uses bounded jitter.
            end
          end
        end
        [@config.max_delay, @config.base_delay * 2**[attempt - 1, 30].min].min * ::Random.rand
      end
    end
  end
end

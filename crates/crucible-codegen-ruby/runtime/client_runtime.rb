# frozen_string_literal: true
require 'securerandom'
module CrucibleGenerated
  module Runtime
    class ClientRuntime
      def initialize(config, codec, endpoint:)
        @config, @codec = config, codec
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
        result = exchange(op, values, scope)
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
            result = exchange(op, values, scope)
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
        WireCodec.string(text).b.bytes.map { |byte| (65..90).cover?(byte) || (97..122).cover?(byte) || (48..57).cover?(byte) || [45, 46, 95, 126].include?(byte) ? byte.chr : format('%%%02X', byte) }.join
      end
      def exchange(op, values, scope) = send_request(op, prepare(op, values), scope)
      # Binding-specific timestamp formats re-encode values the codec wrote in the member's document format.
      def retime(value, context, from, to)
        return nil if value.nil?
        shape = context.shape
        case shape.fetch('kind')
        when 'list' then value.map { |item| retime(item, context.child(shape.fetch('member')), from, to) }
        when 'timestamp'
          source = from || context.format || shape['format']
          target = to || context.format || shape['format']
          Timestamp.read(value, source).wire(target)
        else value
        end
      end
      def bound_member(op, bindings, member, value, header: false)
        format = bindings.fetch('timestampFormats', {})[member]
        if format
          context = @codec.context(op.fetch('input')).member(member)
          value = retime(value, context, nil, format)
          # HTTP dates contain commas, so a list of them is never quoted.
          return value.join(', ') if header && format == 'http-date' && value.is_a?(::Array)
        end
        @codec.bound(header ? media(value, bindings, member, true) : value)
      end
      def prepare(op, input)
        values = input.dup
        token = op['token']
        values[token] = WireCodec.string(@config.idempotency_token.call) if token && values[token].nil?
        bindings = op.fetch('request')
        path = op.fetch('uri').dup
        bindings.fetch('labels').each do |member|
          text = bound_member(op, bindings, member, values.fetch(member))
          raise ::ArgumentError, 'Empty label: ' + member if text.empty?
          greedy = op.fetch('greedyLabels', []).include?(member)
          parts = greedy ? text.split('/', -1) : [text]
          raise ::ArgumentError, 'Dot-segment HTTP label: ' + member if parts.any? { |part| ['.', '..'].include?(part) }
          path = path.gsub('{' + member + (greedy ? '+' : '') + '}', parts.map { |part| escape(part) }.join('/'))
        end
        query = []
        bindings.fetch('query').each do |member, wire|
          next if values[member].nil?
          format = bindings.fetch('timestampFormats', {})[member]
          value = format ? retime(values[member], @codec.context(op.fetch('input')).member(member), nil, format) : values[member]
          (value.is_a?(::Array) ? value : [value]).each { |item| query << escape(wire) + '=' + escape(@codec.bound(item)) }
        end
        path += (path.include?('?') ? '&' : '?') + query.join('&') unless query.empty?
        headers = @config.headers.to_h { |key, value| [key.downcase, value] }
        # A Content-Type bound to an input member outranks the payload's media
        # type, which outranks client header defaults.
        modelled_content_type = false
        bindings.fetch('headers').each do |member, wire|
          next if values[member].nil?
          headers[wire.downcase] = bound_member(op, bindings, member, values[member], header: true)
          modelled_content_type = true if wire.downcase == 'content-type'
        end
        body = nil
        if !bindings.fetch('body').empty?
          object = bindings.fetch('body').filter_map { |member| [member, values[member]] if values.key?(member) }.to_h
          body, headers['content-type'] = ExactJson.encode(@codec.document(op.fetch('input'), object, encode: true)), 'application/json'
        end
        payload = bindings['payload']
        if payload && values[payload[0]]
          value = values[payload[0]]
          body = case bindings.fetch('payloadKind', 'blob')
          when 'string' then WireCodec.string(value).encode(::Encoding::UTF_8)
          when 'document'
            ExactJson.encode(@codec.document(payload_target(op.fetch('input'), payload), value, encode: true))
          else ::Base64.strict_decode64(value)
          end
          headers['content-type'] = payload[1] unless modelled_content_type
        end
        # The release this client was generated for (docs/api-versions-clients.md).
        api_version = @codec.schema['apiVersion']
        headers['crucible-version'] = api_version if api_version.is_a?(String)
        HttpRequest.new(url: @endpoint + path, method: op.fetch('method'), headers: headers, body: body)
      end
      def send_request(op, prepared, scope)
        raise ::IOError, 'Client is closed' if @closed
        safe = op.fetch('safe')
        bindings = op.fetch('response')
        read_body = !bindings.fetch('body').empty? || !bindings['payload'].nil?
        attempts = @config.max_attempts
        (1..attempts).each do |attempt|
          scope.check
          request = prepared
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
          request = Authentication.apply(request, op, @config, scope)
          begin
            response = @transport.send_request(request, read_body: read_body, scope: scope)
          rescue NetworkError
            scope.check
            raise unless safe && attempt < attempts
            scope.wait(retry_delay(attempt, nil))
            next
          end
          scope.check
          return success(op, response) if (200...300).cover?(response.status)
          error = failure(op, response)
          eligible = error.metadata.throttling || (safe && (error.metadata.retryable || [500,502,503,504].include?(response.status)))
          raise error unless eligible && attempt < attempts
          scope.wait(retry_delay(attempt, response.headers['retry-after']))
        end
        raise 'No request attempt'
      end
      def success(op, response)
        bindings = op.fetch('response')
        payload = bindings['payload']
        body = if payload
          {payload[0] => payload_value(op.fetch('output'), bindings, response.body)}
        elsif bindings.fetch('body').empty? || response.body.empty?
          {}
        else WireCodec.object(@codec.document(op.fetch('output'), ExactJson.decode(response.body), encode: false))
        end
        op['output'] ? apply_headers(op['output'], bindings, response, body) : body
      end
      def header_value(target, member, text, format)
        description = @codec.shape(target).fetch('members').fetch(member)
        context = @codec.context(target).member(member)
        if context.shape.fetch('kind') == 'list'
          parts = text.split(',').map(&:strip)
          parts = parts.each_slice(2).map { |pair| pair.join(', ') } if format == 'http-date'
          parts = [] if text.strip.empty?
          return retime(parts.map { |part| format == 'epoch-seconds' ? JsonNumber.new(part) : part }, context, format, nil)
        end
        retime(@codec.header(description.merge('format' => format), text), context, format, nil)
      end
      def apply_headers(target, bindings, response, body)
        object = body.dup
        members = @codec.shape(target).fetch('members')
        formats = bindings.fetch('timestampFormats', {})
        bindings.fetch('headers').each do |member, wire|
          text = response.headers[wire.downcase]
          next if text.nil?
          object[member] = if formats[member]
            header_value(target, member, text, formats[member])
          else media(@codec.header(members.fetch(member), text), bindings, member, false)
          end
        end
        object
      end
      def failure(op, response)
        begin
          body = WireCodec.object(ExactJson.decode(response.body))
        rescue ::ArgumentError
          body = {}
        end
        text = ->(value) { value.is_a?(::String) ? value : nil }
        name = response.headers['x-crucible-error'] || text.(body['__type'])
        request_id = response.headers['x-request-id'] || text.(body['requestId'])
        message = text.(body['message'])
        matches = op.fetch('errors').select do |target|
          shape = @codec.shape(target)
          name ? (shape.fetch('error')['wireCode'] || shape.fetch('name')) == name : shape.fetch('error').fetch('status') == response.status
        end
        target = matches.size == 1 ? matches.first : nil
        shape = target && @codec.shape(target)
        traits = shape && shape.fetch('error')
        metadata = ErrorMetadata.new(name: name || shape&.fetch('name') || 'UnknownError',
          message: message || 'HTTP ' + response.status.to_s,
          status: response.status, request_id: request_id,
          headers: response.headers, retryable: traits&.fetch('retryable', false) == true, throttling: traits&.fetch('throttling', false) == true)
        if target
          begin
            payload = traits['payload']
            body = if payload then {payload[0] => payload_value(target, traits, response.body)}
            else @codec.document(target, body, encode: false)
            end
            object = apply_headers(target, traits, response, body)
            return @codec.error(target, object, metadata)
          rescue ::ArgumentError, ::TypeError
            # Keep wire metadata when modeled members cannot be decoded.
          end
        end
        ServiceException.new(metadata)
      end
      def payload_target(target, payload) = @codec.shape(target).fetch('members').fetch(payload[0]).fetch('target')
      def payload_value(target, bindings, body)
        case bindings.fetch('payloadKind', 'blob')
        when 'string' then WireCodec.string(body.dup.force_encoding(::Encoding::UTF_8))
        when 'document' then body.empty? ? nil : @codec.document(payload_target(target, bindings.fetch('payload')), ExactJson.decode(body), encode: false)
        else ::Base64.strict_encode64(body)
        end
      end
      def media(value, bindings, member, encode)
        return value unless bindings.fetch('headerEncodings', {})[member] == 'base64'
        return value.map { |item| media(item, bindings, member, encode) } if value.is_a?(::Array)
        text = WireCodec.string(value)
        return ::Base64.strict_encode64(text.encode(::Encoding::UTF_8)) if encode
        bytes = ::Base64.strict_decode64(text)
        raise ::ArgumentError, 'Invalid media-type header base64' unless ::Base64.strict_encode64(bytes) == text
        WireCodec.string(bytes.force_encoding(::Encoding::UTF_8))
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

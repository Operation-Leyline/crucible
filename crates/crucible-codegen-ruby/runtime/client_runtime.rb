# frozen_string_literal: true
require 'securerandom'
require 'digest'
module CrucibleGenerated
  module Runtime
    class ClientRuntime
      def initialize(config, codec, endpoint:, digest:)
        @config, @codec, @digest = config, codec, digest
        base = config.endpoint || endpoint
        if codec.schema.key?('endpointRules') && (base.nil? || base.empty?)
          @endpoint = nil
        else
          @endpoint = base.sub(%r{/+\z}, '')
          uri = self.class.url(@endpoint)
          raise ::ArgumentError, 'Endpoint cannot contain query or fragment' if uri.query || uri.fragment
        end
        @builtins = {'AWS::Region' => config.region, 'SDK::Endpoint' => @endpoint}.merge(config.endpoint_builtins)
        @transport, @closed = config.transport || NetHttpTransport.new, false
        # The XML family's codec, or nil for the JSON protocols.
        @xml = XmlProtocol.handles?(protocol) ? XmlProtocol.new(codec.schema) : nil
      end
      def close
        @transport.close unless @closed || @config.transport
        @closed = true
      end
      # A streamed response is written to response_target or yielded to the block in chunks.
      def call(name, input, cancellation: nil, response_target: nil, block: nil)
        scope = CallScope.new(@config.timeout, cancellation)
        scope.check
        op = operation(name)
        values = op['input'] ? @codec.encode(input, @codec.context(op['input'])) : {}
        execute(name, op, values, scope, response_target: response_target, block: block)
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
            result = exchange(name, op, values, scope)
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
      # Polls one operation until a modeled acceptor settles it. Each attempt gets the
      # smaller of the client timeout and the time left before the waiter deadline.
      def wait(operation_name, waiter_name, input, timeout:, min_delay: nil, max_delay: nil, cancellation: nil)
        raise ::ArgumentError, 'Waiter timeout must be finite and positive' unless timeout.is_a?(::Numeric) && timeout.finite? && timeout.positive?
        op = operation(operation_name)
        waiter = Waiter.new(waiter_name, op.fetch('waiters').fetch(waiter_name), min_delay: min_delay, max_delay: max_delay)
        values = op['input'] ? @codec.encode(input, @codec.context(op['input'])) : {}
        query_input = op['input'] ? query_value(values, @codec.context(op['input'])) : nil
        scope = CallScope.new(timeout, cancellation)
        attempts, response, error = 0, nil, nil
        expired = -> { WaiterTimeoutError.new(waiter_name, attempts, response: response, error: error, reason: 'deadline exceeded') }
        loop do
          cancellation&.check
          raise expired.call if scope.remaining <= 0
          attempts += 1
          begin
            response, error = execute(operation_name, op, values, CallScope.new([scope.remaining, @config.timeout].min, cancellation)), nil
          rescue CancelledError
            raise
          rescue ::StandardError => caught
            response, error = nil, caught
          end
          cancellation&.check
          raise expired.call if scope.remaining <= 0
          output = error.nil? && op['output'] ? query_value(@codec.encode(response, @codec.context(op['output'])), @codec.context(op['output'])) : nil
          state, reason = waiter.state(query_input, output, error)
          return response if state == 'success'
          raise WaiterError.new(waiter_name, attempts, response: response, error: error, reason: reason) if state == 'failure'
          begin
            scope.wait(waiter.delay(attempts, scope.remaining))
          rescue DeadlineError
            raise expired.call
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
      AWS_PROTOCOLS = %w[awsRestJson1 awsJson1_0 awsJson1_1].freeze
      def operation(name) = @codec.schema.fetch('operations').fetch(name)
      def protocol = @codec.schema.fetch('protocol', 'crucibleJson')
      def escape(text) = Endpoints.encode(text)
      def execute(name, op, values, scope, response_target: nil, block: nil)
        result = exchange(name, op, values, scope, response_target: response_target, block: block)
        output = op['output'] ? @codec.decode(result, @codec.context(op['output'])) : Unit.new
        scope.check
        output
      end
      def exchange(name, op, values, scope, response_target: nil, block: nil)
        request, checksum, endpoint = prepare(name, op, values)
        sink = if op.fetch('response')['streaming']
          member = op.fetch('response').fetch('payload')[0]
          limits = @codec.shape(op.fetch('output')).fetch('members').fetch(member).fetch('length', {})
          StreamSink.new(target: response_target, block: block, algorithms: checksum.response,
            minimum: limits['min'], maximum: limits['max'])
        end
        raise ::ArgumentError, 'Only streamed responses take a target or block' if sink.nil? && (response_target || block)
        send_request(name, op, request, checksum, endpoint, scope, sink)
      end
      # Query values use member names, exact numbers, and epoch-second timestamps.
      def query_value(value, context)
        return nil if value.nil?
        shape = context.shape
        case shape.fetch('kind')
        when 'structure', 'union'
          members = shape.fetch('members')
          WireCodec.object(value).to_h { |name, item| [name, members.key?(name) ? query_value(item, context.member(name)) : Query.normalize(item)] }
        when 'list' then WireCodec.items(value).map { |item| query_value(item, context.child(shape.fetch('member'))) }
        when 'map' then WireCodec.object(value).transform_values { |item| query_value(item, context.child(shape.fetch('value'))) }
        when 'timestamp' then @codec.decode(value, context).epoch.rational
        when 'blob' then shape['streaming'] ? nil : value
        else Query.normalize(value)
        end
      end
      def resolve_endpoint(op, query_input)
        endpoint = if @codec.schema.key?('endpointRules')
          Endpoints.resolve(@codec.schema.fetch('endpointRules'), op.fetch('endpointParameters', {}), query_input,
            @config.endpoint_parameters, @builtins)
        else Endpoints::ResolvedEndpoint.new(url: @endpoint, headers: {}, properties: {})
        end
        url = endpoint.url
        url = Endpoints.host_prefix(url, op['endpoint'], query_input) if op['endpoint'] && !@config.disable_host_prefix
        endpoint.url = url.sub(%r{/+\z}, '')
        endpoint
      end
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
      def prepare(name, op, input)
        values = input.dup
        query_input = op['input'] && (@codec.schema.key?('endpointRules') || op['endpoint']) ? query_value(values, @codec.context(op['input'])) : {}
        endpoint = resolve_endpoint(op, query_input)
        checksum = Checksums.prepare(op['checksum'], values, @config.request_checksum_calculation, @config.response_checksum_validation)
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
        if (member = bindings['queryParams']) && values[member]
          reserved = bindings.fetch('query').map(&:last)
          reserved += path.split('?', 2).last.split('&').map { |pair| ::URI::DEFAULT_PARSER.unescape(pair.split('=', 2).first) } if path.include?('?')
          WireCodec.object(values[member]).sort.each do |key, value|
            next if reserved.include?(key)
            (value.is_a?(::Array) ? value : [value]).each { |item| query << escape(key) + '=' + escape(@codec.bound(item)) unless item.nil? }
          end
        end
        path += (path.include?('?') ? '&' : '?') + query.join('&') unless query.empty?
        headers = @config.headers.to_h { |key, value| [key.downcase, value] }
        endpoint.headers.each { |key, items| headers[key.downcase] = items.join(', ') }
        bindings.fetch('headers').each { |member, wire| headers[wire.downcase] = bound_member(op, bindings, member, values[member], header: true) unless values[member].nil? }
        explicit = bindings.fetch('headers').map { |_, wire| wire.downcase }
        bindings.fetch('prefixHeaders', []).each do |member, prefix|
          next if values[member].nil?
          WireCodec.object(values[member]).sort.each do |key, value|
            header = (prefix + key).downcase
            headers[header] = @codec.bound(media(value, bindings, member, true)) unless explicit.include?(header)
          end
        end
        body = nil
        if @xml&.query?
          # awsQuery and ec2Query always send a form naming the action and API version.
          body, headers['content-type'] = @xml.form(name, op['input'], values), 'application/x-www-form-urlencoded'
        elsif @xml && !bindings.fetch('body').empty?
          body, headers['content-type'] = @xml.document(op.fetch('input'), bindings.fetch('body'), values), 'application/xml'
        elsif !bindings.fetch('body').empty?
          object = bindings.fetch('body').filter_map { |member| [member, values[member]] if values.key?(member) }.to_h
          body, headers['content-type'] = ExactJson.encode(@codec.document(op.fetch('input'), object, encode: true)), 'application/json'
        end
        payload = bindings['payload']
        if payload && values[payload[0]]
          value = values[payload[0]]
          body = case bindings.fetch('payloadKind', 'blob')
          when 'string' then WireCodec.string(value).encode(::Encoding::UTF_8)
          when 'document'
            @xml ? @xml.payload(op.fetch('input'), payload[0], value) : ExactJson.encode(@codec.document(payload_target(op.fetch('input'), payload), value, encode: true))
          else bindings['streaming'] ? stream_payload(op, bindings, payload, value) : ::Base64.strict_decode64(value)
          end
          headers['content-type'] = payload[1]
        end
        if %w[awsJson1_0 awsJson1_1].include?(protocol)
          # AWS JSON RPC always sends an object and names the operation in X-Amz-Target.
          body ||= '{}'
          headers['content-type'] = 'application/x-amz-json-' + (protocol == 'awsJson1_0' ? '1.0' : '1.1')
          headers['x-amz-target'] = @codec.schema.fetch('service').split('#').last + '.' + name
        end
        # The release this client was generated for (docs/api-versions-clients.md).
        api_version = @codec.schema['apiVersion']
        headers['crucible-version'] = api_version if api_version.is_a?(String)
        [HttpRequest.new(url: endpoint.url + path, method: op.fetch('method'), headers: headers, body: body), checksum, endpoint]
      end
      def stream_payload(op, bindings, payload, stream)
        raise ::ArgumentError, 'Streaming payload requires a known length' if bindings['requiresLength'] && stream.length.nil?
        limits = stream_limits(op)
        if stream.length && ((limits['min'] && stream.length < limits['min']) || (limits['max'] && stream.length > limits['max']))
          raise ::ArgumentError, 'Streaming payload length is outside its modeled bounds'
        end
        stream
      end
      def stream_limits(op) = @codec.shape(op.fetch('input')).fetch('members').fetch(op.fetch('request').fetch('payload')[0]).fetch('length', {})
      def content_sha256(body)
        return ::Digest::SHA256.hexdigest(body || '') unless body.is_a?(ByteStream)
        return 'UNSIGNED-PAYLOAD' unless body.replayable?
        digest = ::Digest::SHA256.new
        body.each_chunk { |chunk| digest.update(chunk) }
        digest.hexdigest
      end
      # Opens a streamed body for this attempt only, after hooks, checksums, and signing.
      def open_stream(op, request)
        stream = request.body
        return request unless stream.is_a?(ByteStream)
        limits = stream_limits(op)
        headers = request.headers.reject { |header, _| %w[content-length transfer-encoding].include?(header) }
        headers[stream.length ? 'content-length' : 'transfer-encoding'] = stream.length ? stream.length.to_s : 'chunked'
        body = StreamBody.new(stream.open, length: stream.length, minimum: limits['min'], maximum: limits['max'], owned: stream.owns_opened?)
        request.with(headers: headers, body: body)
      end
      def send_request(name, op, prepared, checksum, endpoint, scope, sink = nil)
        raise ::IOError, 'Client is closed' if @closed
        safe = op.fetch('safe')
        bindings = op.fetch('response')
        read_body = !bindings.fetch('body').empty? || !bindings['payload'].nil? || !checksum.response.empty?
        # A one-shot stream cannot be replayed, so it gets exactly one attempt.
        attempts = prepared.body.is_a?(ByteStream) && !prepared.body.replayable? ? 1 : @config.max_attempts
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
          request = Checksums.request(checksum, request)
          if @digest
            headers = request.headers.reject { |header, _| header == 'x-amz-content-sha256' }
            headers['x-amz-content-sha256'] = content_sha256(request.body) if request.body || %w[POST PUT PATCH].include?(request.method)
            request = request.with(headers: headers)
          end
          request = Authentication.apply(request, op, @codec.schema, @config, scope, properties: endpoint.properties)
          request = open_stream(op, request)
          begin
            response = sink ? @transport.send_request(request, read_body: true, scope: scope, sink: sink) : @transport.send_request(request, read_body: read_body, scope: scope)
          rescue NetworkError
            scope.check
            # Bytes already handed to the caller cannot be taken back.
            raise unless safe && attempt < attempts && (sink.nil? || sink.bytes.zero?)
            scope.wait(retry_delay(attempt, nil))
            next
          ensure
            request.body.close if request.body.is_a?(StreamBody)
          end
          scope.check
          if (200...300).cover?(response.status)
            sink&.finish
            algorithm = Checksums.response(checksum, response.headers, response.body, hashers: sink&.hashers)
            @config.checksum_validated&.call(name, algorithm) if algorithm
            return success(name, op, response, sink)
          end
          error = failure(op, response)
          eligible = error.metadata.throttling || (safe && (error.metadata.retryable || [500,502,503,504].include?(response.status)))
          raise error unless eligible && attempt < attempts
          scope.wait(retry_delay(attempt, response.headers['retry-after']))
        end
        raise 'No request attempt'
      end
      def success(name, op, response, sink = nil)
        bindings = op.fetch('response')
        payload = bindings['payload']
        body = if sink
          {payload[0] => sink.body}
        elsif @xml && (payload.nil? || bindings['payloadKind'] == 'document')
          @xml.output(name, op['output'], bindings.fetch('body'), payload&.first, response.body)
        elsif payload
          member = @codec.shape(op.fetch('output')).fetch('members').fetch(payload[0])
          absent_blob = @xml && response.body.empty? && member['optional'] &&
            @codec.shape(member.fetch('target'))['kind'] == 'blob'
          absent_blob ? {} : {payload[0] => payload_value(op.fetch('output'), bindings, response.body)}
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
        explicit = bindings.fetch('headers').map { |_, wire| wire.downcase }
        bindings.fetch('prefixHeaders', []).each do |member, prefix|
          prefix = prefix.downcase
          object[member] = response.headers.filter_map { |header, text| [header.delete_prefix(prefix), media(text, bindings, member, false)] if header.start_with?(prefix) && (prefix.empty? || !explicit.include?(header)) }.to_h
        end
        object[bindings['responseCode']] = response.status if bindings['responseCode']
        object
      end
      def failure(op, response)
        envelope = @xml && XmlProtocol.error(response.body)
        begin
          body = envelope ? {} : WireCodec.object(ExactJson.decode(response.body))
        rescue ::ArgumentError
          body = {}
        end
        text = ->(value) { value.is_a?(::String) ? value : nil }
        aws = AWS_PROTOCOLS.include?(protocol)
        if envelope
          name, message = envelope.code, envelope.message
          request_id = response.headers['x-amzn-requestid'] || response.headers['x-amz-request-id'] || envelope.request_id
        elsif aws
          # AWS JSON names errors in a header or body, possibly with a namespace and a ':' suffix.
          name = response.headers['x-amzn-errortype'] || text.(body['code']) || text.(body['__type'])
          name = name&.split(':', 2)&.first&.split('#')&.last
          request_id = response.headers['x-amzn-requestid'] || response.headers['x-amz-request-id'] || response.headers['x-request-id']
          message = text.(body['message']) || text.(body['Message'])
        else
          name = response.headers['x-crucible-error'] || text.(body['__type'])
          request_id = response.headers['x-request-id'] || text.(body['requestId'])
          message = text.(body['message'])
        end
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
            elsif envelope then envelope.error ? @xml.read_members(envelope.error, target, shape.fetch('members').keys) : {}
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

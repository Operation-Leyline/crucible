# frozen_string_literal: true
require 'openssl'
require 'base64'
require 'uri'
module CrucibleGenerated
  module Runtime
    module Authentication
      def self.secure(request)
        uri = ::URI.parse(request.url)
        raise ::ArgumentError, 'Credentials require HTTPS or loopback HTTP' unless uri.scheme == 'https' || %w[localhost 127.0.0.1 ::1].include?(uri.hostname.downcase)
        request
      end
      def self.apply(request, op, schema, config, scope)
        schemes = schema.fetch('authSchemes', {})
        ids = op.fetch('auth') { op.fetch('bearer', false) ? ['smithy.api#httpBearerAuth'] : [] }
        ids.each do |id|
          scheme = schemes.fetch(id, {'kind' => 'bearer'}); kind = scheme.fetch('kind')
          auth = request.headers['authorization']
          return secure(request) if %w[bearer basic].include?(kind) && auth&.downcase&.start_with?(kind + ' ')
          return secure(request) if kind == 'sigV4' && auth&.start_with?('AWS4-HMAC-SHA256 ')
          if kind == 'apiKey'
            return secure(request) if scheme['location'] == 'header' && request.headers.key?(scheme['name'].downcase)
            return secure(request) if scheme['location'] == 'query' && (::URI.parse(request.url).query || '').split('&').any? { |part| ::URI::DEFAULT_PARSER.unescape(part.split('=',2)[0]) == scheme['name'] }
          end
          provider = {'bearer' => config.token, 'basic' => config.basic_auth, 'apiKey' => config.api_key, 'sigV4' => config.credentials}[kind]
          next unless provider
          identity = provider.call; scope.check; next if identity.nil?
          case kind
          when 'bearer' then request = request.with(headers: request.headers.merge('authorization' => 'Bearer ' + WireCodec.string(identity)))
          when 'basic'
            raise ::ArgumentError, 'Expected BasicCredentials' unless identity.is_a?(BasicCredentials)
            request = request.with(headers: request.headers.merge('authorization' => 'Basic ' + ::Base64.strict_encode64((identity.username + ':' + identity.password).encode('UTF-8'))))
          when 'apiKey'
            identity = WireCodec.string(identity); raise ::ArgumentError, 'API key must be nonempty' if identity.empty?
            if scheme['location'] == 'query'
              uri = ::URI.parse(request.url)
              pairs = (uri.query || '').split('&').reject { |part| ::URI::DEFAULT_PARSER.unescape(part.split('=',2)[0]) == scheme['name'] }
              pairs << encode(scheme['name']) + '=' + encode(identity); uri.query = pairs.join('&'); request = request.with(url: uri.to_s)
            else
              request = request.with(headers: request.headers.merge(scheme['name'].downcase => (scheme['scheme'] ? scheme['scheme'] + ' ' : '') + identity))
            end
          when 'sigV4'
            raise ::ArgumentError, 'Expected AwsCredentials' unless identity.is_a?(AwsCredentials)
            request = sign(request, identity, service: scheme.fetch('signingName'), region: config.region, unsigned_payload: op.fetch('unsignedPayload', false))
          end
          return secure(request)
        end
        raise ::ArgumentError, 'No credentials for an enabled authentication scheme' if ids.any? { |id| id != 'smithy.api#httpBearerAuth' } && !op.fetch('optionalAuth', false)
        request
      end
      def self.encode(text) = text.encode('UTF-8').bytes.map { |byte| ((65..90).cover?(byte) || (97..122).cover?(byte) || (48..57).cover?(byte) || [45,46,95,126].include?(byte)) ? byte.chr : '%%%02X' % byte }.join
      def self.sign(request, credentials, service:, region:, now: ::Time.now, unsigned_payload: false)
        raise ::ArgumentError, 'AWS credentials have expired' if credentials.expires_at && credentials.expires_at <= now
        raise ::ArgumentError, 'Invalid SigV4 service or region' if service.nil? || region.nil? || service.empty? || region.empty? || /[\/\r\n]/.match?(service + region)
        now = now.utc; headers = request.headers.reject { |name,_| %w[authorization x-amz-security-token].include?(name) }
        headers['x-amz-date'] = now.strftime('%Y%m%dT%H%M%SZ')
        headers['x-amz-security-token'] = credentials.session_token if credentials.session_token
        headers['x-amz-content-sha256'] = unsigned_payload ? 'UNSIGNED-PAYLOAD' : ::Digest::SHA256.hexdigest(request.body || '')
        uri = ::URI.parse(request.url); headers['host'] = uri.host.downcase + (uri.port == uri.default_port ? '' : ':' + uri.port.to_s)
        path = uri.path.empty? ? '/' : uri.path
        unless service == 's3'
          parts = []
          path.split('/').each { |part| part == '..' ? parts.pop : parts.push(part) unless part.empty? || part == '.' }
          path = '/' + parts.join('/') + (path.end_with?('/') && !parts.empty? ? '/' : '')
          path = encode(path).gsub('%2F','/')
        end
        query = (uri.query || '').split('&').reject(&:empty?).map { |part| key,value = part.split('=',2); [encode(::URI::DEFAULT_PARSER.unescape(key)),encode(::URI::DEFAULT_PARSER.unescape(value || ''))] }.sort.map { |pair| pair.join('=') }.join('&')
        excluded = %w[authorization connection expect transfer-encoding user-agent x-amzn-trace-id accept-encoding content-length]
        canonical = headers.reject { |key,_| excluded.include?(key) }.sort.to_h
        signed = canonical.keys.join(';'); lines = canonical.map { |key,value| key + ':' + value.strip.gsub(/\s+/,' ') + "\n" }.join
        canonical_request = [request.method,path,query,lines,signed,headers['x-amz-content-sha256']].join("\n")
        date = now.strftime('%Y%m%d'); scope = "#{date}/#{region}/#{service}/aws4_request"
        text = "AWS4-HMAC-SHA256\n#{headers['x-amz-date']}\n#{scope}\n#{::Digest::SHA256.hexdigest(canonical_request)}"
        key = 'AWS4' + credentials.secret_access_key
        [date,region,service,'aws4_request'].each { |part| key = ::OpenSSL::HMAC.digest('SHA256',key,part) }
        headers['authorization'] = "AWS4-HMAC-SHA256 Credential=#{credentials.access_key_id}/#{scope}, SignedHeaders=#{signed}, Signature=#{::OpenSSL::HMAC.hexdigest('SHA256',key,text)}"
        request.with(headers: headers)
      end
    end
  end
end

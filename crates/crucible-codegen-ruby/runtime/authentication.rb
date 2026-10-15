# frozen_string_literal: true
require 'uri'
module CrucibleGenerated
  module Runtime
    module Authentication
      def self.secure(request)
        uri = ::URI.parse(request.url)
        raise ::ArgumentError, 'Credentials require HTTPS or loopback HTTP' unless uri.scheme == 'https' || %w[localhost 127.0.0.1 ::1].include?(uri.hostname.downcase)
        request
      end
      # Adds the bearer token to an operation that takes one. A token source that
      # returns nil, or an Authorization header the caller already set, leaves the
      # request as it is.
      def self.apply(request, op, config, scope)
        return request unless op.fetch('bearer', false)
        return secure(request) if request.headers['authorization']&.downcase&.start_with?('bearer ')
        return request unless config.token
        identity = config.token.call
        scope.check
        return request if identity.nil?
        secure(request.with(headers: request.headers.merge('authorization' => 'Bearer ' + WireCodec.string(identity))))
      end
    end
  end
end

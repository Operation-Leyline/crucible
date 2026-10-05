# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    class ClientConfig
      attr_reader :endpoint, :token, :transport, :timeout, :max_attempts, :base_delay, :max_delay, :headers, :middleware
      def initialize(endpoint: nil, token: nil, transport: nil, timeout: 30.0, max_attempts: 3, base_delay: 0.1, max_delay: 10.0, headers: {}, middleware: [])
        [timeout, base_delay, max_delay].each do |value|
          raise ::ArgumentError, 'Invalid duration' unless value.is_a?(::Numeric) && value.finite? && value >= 0
        end
        raise ::ArgumentError, 'Invalid call limits' unless timeout > 0 && max_attempts.is_a?(::Integer) && max_attempts >= 1
        @endpoint, @token, @transport = endpoint&.dup&.freeze, token, transport
        @timeout, @max_attempts, @base_delay, @max_delay = timeout, max_attempts, base_delay, max_delay
        @headers = headers.to_h { |key, value| [key.dup.freeze, value.dup.freeze] }.freeze
        @middleware = middleware.dup.freeze
        freeze
      end
    end
  end
end

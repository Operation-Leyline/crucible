# frozen_string_literal: true
require 'securerandom'
module CrucibleGenerated
  module Runtime
    class ClientConfig
      attr_reader :endpoint, :token, :transport, :timeout, :max_attempts, :base_delay, :max_delay, :headers, :middleware, :basic_auth, :api_key, :credentials, :region,
        :endpoint_parameters, :endpoint_builtins, :disable_host_prefix, :request_checksum_calculation,
        :response_checksum_validation, :checksum_validated, :idempotency_token
      def initialize(endpoint: nil, token: nil, transport: nil, timeout: 30.0, max_attempts: 3, base_delay: 0.1, max_delay: 10.0, headers: {}, middleware: [], basic_auth: nil, api_key: nil, credentials: nil, region: nil,
                     endpoint_parameters: {}, endpoint_builtins: {}, disable_host_prefix: false,
                     request_checksum_calculation: 'when_supported', response_checksum_validation: 'when_supported', checksum_validated: nil,
                     idempotency_token: -> { ::SecureRandom.uuid })
        [timeout, base_delay, max_delay].each do |value|
          raise ::ArgumentError, 'Invalid duration' unless value.is_a?(::Numeric) && value.finite? && value >= 0
        end
        raise ::ArgumentError, 'Invalid call limits' unless timeout > 0 && max_attempts.is_a?(::Integer) && max_attempts >= 1
        raise ::ArgumentError, 'Idempotency token supplier must be callable' unless idempotency_token.respond_to?(:call)
        @basic_auth, @api_key, @credentials, @region = basic_auth, api_key, credentials, region&.dup&.freeze
        @endpoint, @token, @transport = endpoint&.dup&.freeze, token, transport
        @timeout, @max_attempts, @base_delay, @max_delay = timeout, max_attempts, base_delay, max_delay
        @headers = headers.to_h { |key, value| [key.dup.freeze, value.dup.freeze] }.freeze
        @middleware = middleware.dup.freeze
        @endpoint_parameters = endpoint_parameters.to_h { |key, value| [key.to_s.freeze, value.dup.freeze] }.freeze
        @endpoint_builtins = endpoint_builtins.to_h { |key, value| [key.to_s.freeze, value.dup.freeze] }.freeze
        @disable_host_prefix = disable_host_prefix == true
        @request_checksum_calculation = Checksums.policy(request_checksum_calculation)
        @response_checksum_validation = Checksums.policy(response_checksum_validation)
        @checksum_validated = checksum_validated
        @idempotency_token = idempotency_token
        freeze
      end
    end
  end
end

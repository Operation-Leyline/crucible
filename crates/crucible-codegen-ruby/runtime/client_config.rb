# frozen_string_literal: true
require 'securerandom'
module CrucibleGenerated
  module Runtime
    # Settings for a generated client, fixed when the client is created.
    #
    # Every setting is optional. Without credentials, only operations that need
    # none can succeed. A configuration is frozen and may be shared by any
    # number of clients.
    class ClientConfig
      # @return [String, nil] the service base URL, overriding the one generated into the client; absolute HTTP(S) without credentials, query or fragment, and required unless the model's endpoint rules resolve every request
      attr_reader :endpoint
      # @return [#call, nil] bearer token source, for services that use HTTP bearer authentication; returns a String or nil
      attr_reader :token
      # @return [Object, nil] sends the HTTP requests and responds to `send_request` and `close`; defaults to a {NetHttpTransport} that the client owns and closes with itself, while one you supply stays yours to close
      attr_reader :transport
      # @return [Float] seconds one call may take in total, covering every attempt and retry delay; exceeding it raises {DeadlineError}
      attr_reader :timeout
      # @return [Integer] the most times a request is sent, the first attempt included; 1 disables retries. Only read-only or idempotent operations are retried after a network failure or a 500, 502, 503, 504 or retryable response; throttling responses are retried for any operation
      attr_reader :max_attempts
      # @return [Float] ceiling in seconds of the first retry delay; delays use exponential backoff with full jitter, doubling per attempt up to {#max_delay}, and a Retry-After response header takes precedence
      attr_reader :base_delay
      # @return [Float] ceiling in seconds of any single backoff delay
      attr_reader :max_delay
      # @return [Hash{String => String}] headers added to every request
      attr_reader :headers
      # @return [Array<#call>] request rewriters, each given an {HttpRequest} and returning one, run in order on every attempt
      attr_reader :middleware
      # @return [#call, nil] HTTP Basic credential source, returning a {BasicCredentials} or nil
      attr_reader :basic_auth
      # @return [#call, nil] API key source, returning a String or nil
      attr_reader :api_key
      # @return [#call, nil] AWS credential source for Signature Version 4, returning {AwsCredentials} or nil; see {Credentials} and {CachedCredentials}
      attr_reader :credentials
      # @return [String, nil] the SigV4 signing region and the `AWS::Region` endpoint built-in
      attr_reader :region
      # @return [Hash{String => Object}] client context parameters for modeled endpoint rules
      attr_reader :endpoint_parameters
      # @return [Hash{String => Object}] overrides for endpoint rule built-ins such as `SDK::Endpoint`
      attr_reader :endpoint_builtins
      # @return [Boolean] whether modeled `hostPrefix` values are left off the host, for example when {#endpoint} points at a local test server
      attr_reader :disable_host_prefix
      # @return [String] `'when_supported'` (the default) to checksum request payloads whenever the service supports it, or `'when_required'`
      attr_reader :request_checksum_calculation
      # @return [String] `'when_supported'` (the default) to validate every response checksum the service sends, or `'when_required'`
      attr_reader :response_checksum_validation
      # @return [#call, nil] called with the operation name and algorithm after a response checksum validates
      attr_reader :checksum_validated
      # @return [#call] supplies the value for an idempotency token the caller omitted; a random UUID by default
      attr_reader :idempotency_token
      # @raise [ArgumentError] when a duration, the attempt limit, a checksum policy or an endpoint parameter is invalid
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

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
      # @return [String, nil] the service base URL, overriding the one generated into the client; absolute HTTP(S) without credentials, query or fragment
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
      # @return [#call] supplies the value for an idempotency token the caller omitted; a random UUID by default
      attr_reader :idempotency_token
      # @raise [ArgumentError] when a duration, the attempt limit or the idempotency token supplier is invalid
      def initialize(endpoint: nil, token: nil, transport: nil, timeout: 30.0, max_attempts: 3, base_delay: 0.1, max_delay: 10.0, headers: {}, middleware: [],
                     idempotency_token: -> { ::SecureRandom.uuid })
        [timeout, base_delay, max_delay].each do |value|
          raise ::ArgumentError, 'Invalid duration' unless value.is_a?(::Numeric) && value.finite? && value >= 0
        end
        raise ::ArgumentError, 'Invalid call limits' unless timeout > 0 && max_attempts.is_a?(::Integer) && max_attempts >= 1
        raise ::ArgumentError, 'Idempotency token supplier must be callable' unless idempotency_token.respond_to?(:call)
        @endpoint, @token, @transport = endpoint&.dup&.freeze, token, transport
        @timeout, @max_attempts, @base_delay, @max_delay = timeout, max_attempts, base_delay, max_delay
        @headers = headers.to_h { |key, value| [key.dup.freeze, value.dup.freeze] }.freeze
        @middleware = middleware.dup.freeze
        @idempotency_token = idempotency_token
        freeze
      end
    end
  end
end

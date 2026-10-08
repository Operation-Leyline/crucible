# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # AWS credentials for Signature Version 4. Secrets never appear in `inspect`.
    class AwsCredentials
      # @return [String] the access key ID
      attr_reader :access_key_id
      # @return [String] the secret access key
      attr_reader :secret_access_key
      # @return [String, nil] the session token of temporary credentials
      attr_reader :session_token
      # @return [Time, nil] when temporary credentials expire; signing with expired ones fails
      attr_reader :expires_at
      # @raise [ArgumentError] when the key ID or secret is empty
      def initialize(access_key_id:, secret_access_key:, session_token: nil, expires_at: nil)
        raise ::ArgumentError, 'AWS credentials must be nonempty' if access_key_id.empty? || secret_access_key.empty?
        raise ::ArgumentError, 'Credential expiry must be a Time' if expires_at && !expires_at.is_a?(::Time)
        @access_key_id, @secret_access_key = access_key_id.dup.freeze, secret_access_key.dup.freeze
        @session_token, @expires_at = session_token&.dup&.freeze, expires_at&.dup&.freeze
        freeze
      end
      def inspect = '#<AwsCredentials [redacted]>'
    end
  end
end

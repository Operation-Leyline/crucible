# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    class AwsCredentials
      attr_reader :access_key_id, :secret_access_key, :session_token, :expires_at
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

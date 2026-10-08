# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # HTTP Basic credentials. The password never appears in `inspect`.
    class BasicCredentials
      # @return [String] the user name; it may not contain a colon
      attr_reader :username
      # @return [String] the password, sent only in the `Authorization` header over HTTPS or loopback HTTP
      attr_reader :password
      # @raise [ArgumentError] when the user name contains a colon
      def initialize(username:, password:)
        raise ::ArgumentError, 'Basic username cannot contain a colon' if username.include?(':')
        @username, @password = username.dup.freeze, password.dup.freeze
        freeze
      end
      def inspect = '#<BasicCredentials [redacted]>'
    end
  end
end

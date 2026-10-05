# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    class BasicCredentials
      attr_reader :username, :password
      def initialize(username:, password:)
        raise ::ArgumentError, 'Basic username cannot contain a colon' if username.include?(':')
        @username, @password = username.dup.freeze, password.dup.freeze
        freeze
      end
      def inspect = '#<BasicCredentials [redacted]>'
    end
  end
end

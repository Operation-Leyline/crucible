# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # One HTTP response with lower-cased header names.
    class HttpResponse
      # @return [Integer] the status code
      attr_reader :status
      # @return [Hash{String => String}] the response headers, names in lower case
      attr_reader :headers
      # @return [String] the body bytes; empty when none was read
      attr_reader :body
      def initialize(status, headers: {}, body: ''.b)
        @status, @body = status, body.dup.b.freeze
        @headers = headers.to_h { |key, value| [key.downcase.freeze, value.dup.freeze] }.freeze
        freeze
      end
    end
  end
end

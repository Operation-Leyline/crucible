# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    class HttpResponse
      attr_reader :status, :headers, :body
      def initialize(status, headers: {}, body: ''.b)
        @status, @body = status, body.dup.b.freeze
        @headers = headers.to_h { |key, value| [key.downcase.freeze, value.dup.freeze] }.freeze
        freeze
      end
    end
  end
end

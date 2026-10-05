# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    class HttpRequest
      attr_reader :url, :method, :headers, :body
      def initialize(url:, method:, headers: {}, body: nil)
        token = /\A[!#$%&'*+.^_`|~0-9A-Za-z-]+\z/
        raise ::ArgumentError, 'Invalid HTTP method' unless method.is_a?(::String) && token.match?(method)
        headers.each do |name, value|
          unless name.is_a?(::String) && token.match?(name) && value.is_a?(::String) && !/[\r\n\x00]/.match?(value)
            raise ::ArgumentError, 'Invalid HTTP header'
          end
        end
        @url, @method = url.dup.freeze, method.dup.freeze
        @headers = headers.to_h { |key, value| [key.downcase.freeze, value.dup.freeze] }.freeze
        @body = body&.dup&.b&.freeze
        freeze
      end
      def with(headers: @headers, body: @body, url: @url, method: @method)
        self.class.new(url: url, method: method, headers: headers, body: body)
      end
    end
  end
end

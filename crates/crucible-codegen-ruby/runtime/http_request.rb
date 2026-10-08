# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # One outgoing HTTP request. Frozen: {#with} returns a changed copy.
    class HttpRequest
      # @return [String] the absolute URL
      attr_reader :url
      # @return [String] the HTTP method, upper case
      attr_reader :method
      # @return [Hash{String => String}] the request headers, names in lower case
      attr_reader :headers
      # @return [String, ByteStream, nil] the body bytes, a streaming body, or nil for none
      attr_reader :body
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
        # Streaming bodies stay by reference: the transport reads them as bytes flow.
        @body = body.is_a?(ByteStream) || body.is_a?(StreamBody) ? body : body&.dup&.b&.freeze
        freeze
      end
      # @return [HttpRequest] a copy with the given parts replaced
      def with(headers: @headers, body: @body, url: @url, method: @method)
        self.class.new(url: url, method: method, headers: headers, body: body)
      end
    end
  end
end

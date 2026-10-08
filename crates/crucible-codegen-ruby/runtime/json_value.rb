# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # An immutable JSON document that keeps every numeric token exactly.
    class JsonValue
      # @return [String] the document as JSON text
      attr_reader :json
      alias to_s json
      def initialize(value)
        @json = ExactJson.encode(value).freeze
        freeze
      end
      def self.parse(text) = new(ExactJson.decode(text))
      def value = ExactJson.decode(@json)
    end
  end
end

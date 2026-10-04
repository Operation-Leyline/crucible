# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    class JsonValue
      attr_reader :json
      def initialize(value)
        @json = ExactJson.encode(value).freeze
        freeze
      end
      def self.parse(text) = new(ExactJson.decode(text))
      def value = ExactJson.decode(@json)
    end
  end
end

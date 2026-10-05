# frozen_string_literal: true
require 'json'
module CrucibleGenerated
  module Runtime
    module ExactJson
      def self.decode(text)
        value = ::JSON.parse(text, decimal_class: JsonNumber, max_nesting: 255, create_additions: false, allow_nan: false)
        validate_strings(value)
        value
      rescue ::JSON::ParserError => error
        raise ::ArgumentError, error.message
      end
      def self.validate_strings(value)
        case value
        when ::String
          raise ::ArgumentError, 'Invalid JSON Unicode string' unless value.dup.force_encoding(::Encoding::UTF_8).valid_encoding?
        when ::Array then value.each { |item| validate_strings(item) }
        when ::Hash then value.each { |key, item| validate_strings(key); validate_strings(item) }
        end
      end
      def self.encode(value, depth = 0)
        raise ::ArgumentError, 'JSON nesting limit' if depth > 255
        case value
        when JsonNumber then value.text
        when ::Integer then value.to_s
        when ::Float
          raise ::ArgumentError, 'Non-finite float' unless value.finite?
          ::JSON.generate(value)
        when ::String
          validate_strings(value)
          ::JSON.generate(value)
        when true, false, nil then ::JSON.generate(value)
        when ::Array then '[' + value.map { |item| encode(item, depth + 1) }.join(',') + ']'
        when ::Hash
          raise ::ArgumentError, 'JSON object keys must be strings' unless value.keys.all? { |key| key.is_a?(::String) }
          '{' + value.keys.sort.map { |key| encode(key) + ':' + encode(value.fetch(key), depth + 1) }.join(',') + '}'
        else raise ::ArgumentError, 'Expected JSON value'
        end
      rescue ::JSON::GeneratorError => error
        raise ::ArgumentError, error.message
      end
    end
  end
end

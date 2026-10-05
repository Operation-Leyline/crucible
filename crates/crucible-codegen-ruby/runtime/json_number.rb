# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    class JsonNumber
      attr_reader :text
      def initialize(text)
        raise ::ArgumentError, 'Invalid JSON number' unless text.is_a?(::String) && /\A-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?\z/.match?(text)
        @text = text.dup.freeze
        freeze
      end
      def rational
        exponent = @text.split(/[eE]/, 2)[1]
        raise ::ArgumentError, 'Numeric exponent limit' if exponent && exponent.to_i.abs > 10_000
        ::Kernel.Rational(@text)
      end
      def integer(min = -(2**63), max = 2**63 - 1)
        value = rational
        raise ::ArgumentError, 'Integer out of range' unless value.denominator == 1 && value >= min && value <= max
        value.to_i
      end
      def floating
        value = ::Kernel.Float(@text)
        raise ::ArgumentError, 'Non-finite float' unless value.finite?
        value
      end
    end
  end
end

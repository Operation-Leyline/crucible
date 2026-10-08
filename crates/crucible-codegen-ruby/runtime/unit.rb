# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # Marks an optional member as omitted, as distinct from an explicit nil.
    UNSET = ::Object.new.freeze
    # The empty input or output of an operation that takes or returns nothing.
    class Unit
      def initialize = freeze
      def inspect = 'Unit()'
      alias to_s inspect
    end
  end
end

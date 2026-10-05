# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    class ServiceError < ::StandardError
      attr_reader :metadata
      # Waiters use the original modeled shape name; wire metadata stays intact.
      attr_accessor :modeled_error_name
      def initialize(metadata = ErrorMetadata.new)
        @metadata = metadata
        super(metadata.message)
      end
    end
  end
end

# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    class ServiceError < ::StandardError
      attr_reader :metadata
      def initialize(metadata = ErrorMetadata.new)
        @metadata = metadata
        super(metadata.message)
      end
    end
  end
end

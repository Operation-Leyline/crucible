# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # A response the service sent that is not a success.
    #
    # Rescue the service exception class to handle every modeled and unknown
    # service error, or a generated error class for one modeled error.
    # Transport failures are a {NetworkError} or {DeadlineError} instead.
    class ServiceError < ::StandardError
      # @return [ErrorMetadata] status, request ID and retry classification of the failed response
      attr_reader :metadata
      # Waiters use the original modeled shape name; wire metadata stays intact.
      attr_accessor :modeled_error_name
      def initialize(metadata = ErrorMetadata.new)
        @metadata = metadata
        super(metadata.message)
      end
    end
    # The base of every error this service returns, modeled or not. An error
    # response the model does not describe, such as one added by a newer
    # server, is raised as this class itself.
    class ServiceException < ServiceError; end
  end
end

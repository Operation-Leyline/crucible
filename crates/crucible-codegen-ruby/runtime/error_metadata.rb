# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    ErrorMetadata = ::Data.define(:name, :message, :status, :request_id, :headers, :retryable, :throttling) do
      def initialize(name: 'UnknownError', message: '', status: 0, request_id: nil, headers: {}, retryable: false, throttling: false)
        super
      end
    end
  end
end

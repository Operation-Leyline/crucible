# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # What the transport learned about a failed call, apart from the modeled body.
    #
    # `name` is the error's shape name as received, `message` the service's
    # human-readable message, `status` the HTTP status (0 without a response),
    # `request_id` the request ID header when sent, `headers` the lower-cased
    # response headers, `retryable` whether the retry policy treats the failure
    # as transient, and `throttling` whether the service asked the client to
    # slow down.
    ErrorMetadata = ::Data.define(:name, :message, :status, :request_id, :headers, :retryable, :throttling) do
      def initialize(name: 'UnknownError', message: '', status: 0, request_id: nil, headers: {}, retryable: false, throttling: false)
        super
      end

      # Omits `message` and `headers`: they may carry sensitive data.
      def inspect
        "ErrorMetadata(name=#{name}, status=#{status}" + (request_id ? ", request_id=#{request_id}" : '') +
          (retryable ? ', retryable=true' : '') + (throttling ? ', throttling=true' : '') + ')'
      end
      alias to_s inspect

      # pp prints a Data's members itself instead of calling `inspect`, which would list the headers.
      def pretty_print(printer) = printer.text(inspect)
    end
  end
end

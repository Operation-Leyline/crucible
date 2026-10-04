# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    class CancelledError < ::StandardError; end
    class DeadlineError < ::StandardError; end
    class NetworkError < ::StandardError; end
    class CancellationToken
      def initialize(&predicate)
        @mutex, @cancelled, @predicate = ::Mutex.new, false, predicate
      end
      def cancel = @mutex.synchronize { @cancelled = true }
      def check
        cancelled = @mutex.synchronize { @cancelled }
        cancel if !cancelled && @predicate&.call
        raise CancelledError, 'Request cancelled' if @mutex.synchronize { @cancelled }
      end
    end
  end
end

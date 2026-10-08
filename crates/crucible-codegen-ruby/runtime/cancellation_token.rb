# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # The call's {CancellationToken} was cancelled before it finished.
    class CancelledError < ::StandardError; end
    # The call outlasted {ClientConfig#timeout}.
    class DeadlineError < ::StandardError; end
    # The request never produced a response: DNS, connection, TLS or I/O failure.
    class NetworkError < ::StandardError; end
    # Caller-owned cancellation shared by any number of calls and threads.
    #
    # Pass it as `cancellation:` to an operation; cancelling it raises
    # {CancelledError} from the call.
    class CancellationToken
      # @yieldreturn [Boolean] an optional polling predicate lets a synchronous caller observe external cancellation
      def initialize(&predicate)
        @mutex, @cancelled, @predicate = ::Mutex.new, false, predicate
      end
      # Cancels every call that uses this token.
      def cancel = @mutex.synchronize { @cancelled = true }
      # @raise [CancelledError] when cancelled
      def check
        cancelled = @mutex.synchronize { @cancelled }
        cancel if !cancelled && @predicate&.call
        raise CancelledError, 'Request cancelled' if @mutex.synchronize { @cancelled }
      end
    end
  end
end

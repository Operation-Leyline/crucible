# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    class CallScope
      def initialize(timeout, cancellation)
        @deadline = self.class.now + timeout
        @cancellation = cancellation
      end
      def self.now = ::Process.clock_gettime(::Process::CLOCK_MONOTONIC)
      def remaining = [0.0, @deadline - self.class.now].max
      def check
        @cancellation&.check
        raise DeadlineError, 'Request deadline exceeded' if remaining <= 0
      end
      def wait(seconds)
        deadline = self.class.now + seconds
        loop do
          check
          left = deadline - self.class.now
          return if left <= 0
          ::Kernel.sleep([0.025, left, remaining].min)
        end
      end
    end
  end
end

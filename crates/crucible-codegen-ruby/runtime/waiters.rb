# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # A waiter reached a failure acceptor, an unmatched error, or its deadline.
    class WaiterError < ::StandardError
      attr_reader :waiter_name, :attempts, :response, :error, :reason
      def initialize(waiter_name, attempts, response: nil, error: nil, reason: nil)
        @waiter_name, @attempts, @response, @error, @reason = waiter_name, attempts, response, error, reason
        detail = reason.is_a?(::Array) ? reason.join(', ') : reason
        super("Waiter #{waiter_name} failed after #{attempts} attempts" + (detail ? ': ' + detail : ''))
      end
    end
    class WaiterTimeoutError < WaiterError; end
    # Modeled acceptors and Smithy's jittered exponential backoff.
    class Waiter
      attr_reader :name, :min_delay, :max_delay
      def initialize(name, description, min_delay: nil, max_delay: nil)
        @name, @acceptors = name, description.fetch('acceptors')
        @min_delay = min_delay.nil? ? description.fetch('minDelay') : min_delay
        @max_delay = max_delay.nil? ? description.fetch('maxDelay') : max_delay
        valid = [@min_delay, @max_delay].all? { |value| value.is_a?(::Numeric) && value.finite? && value >= 0 }
        raise ::ArgumentError, 'Waiter delays must be finite, nonnegative, and ordered' unless valid && @max_delay >= @min_delay
      end
      # Returns [state, reason]; output and input are query-domain values.
      def state(input, output, error)
        @acceptors.each do |acceptor|
          matcher = acceptor.fetch('matcher')
          next unless matches?(matcher, input, output, error)
          reason = nil
          if (message = acceptor['message'])
            value = Query.search(message.fetch('expression'), output)
            reason = value if value.is_a?(::String) || (value.is_a?(::Array) && value.all?(::String))
          end
          return [acceptor.fetch('state'), reason]
        end
        [error ? 'failure' : 'retry', nil]
      end
      def delay(attempt, remaining, random: ::Random)
        ceiling = [@max_delay, @min_delay * 2**[attempt - 1, 62].min].min
        delay = @min_delay + random.rand * (ceiling - @min_delay)
        return remaining if remaining <= @min_delay
        # Preserve a final call's budget when the next delay would consume it.
        remaining - delay <= @min_delay ? remaining - @min_delay : delay
      end
      private
      def matches?(matcher, input, output, error)
        case matcher.fetch('kind')
        when 'success' then error.nil? == matcher.fetch('expected')
        when 'errorType'
          expected = matcher.fetch('name')
          error.is_a?(ServiceError) && [expected, expected.split('#').last].include?(error.modeled_error_name || error.metadata.name)
        else
          return false if error
          source = matcher['kind'] == 'inputOutput' ? {'input' => input, 'output' => output} : output
          result = Query.evaluate(matcher.fetch('path').fetch('expression'), source)
          expected = matcher.fetch('expected')
          case matcher.fetch('comparator')
          when 'stringEquals' then result.is_a?(::String) && result == expected
          when 'booleanEquals' then (result == true || result == false) && result == (expected == 'true')
          when 'allStringEquals' then result.is_a?(::Array) && !result.empty? && result.all? { |item| item.is_a?(::String) && item == expected }
          when 'anyStringEquals' then result.is_a?(::Array) && result.any? { |item| item.is_a?(::String) && item == expected }
          else false
          end
        end
      end
    end
  end
end

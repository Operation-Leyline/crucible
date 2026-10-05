# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    class CachedCredentials
      def initialize(provider, refresh_before: 60, clock: -> { ::Time.now })
        raise ::ArgumentError, 'Invalid refresh interval' unless refresh_before.finite? && refresh_before >= 0
        @provider, @refresh_before, @clock = provider, refresh_before, clock
        @mutex, @value, @refresh_at = ::Mutex.new, nil, nil
      end
      def call
        @mutex.synchronize do
          now = @clock.call
          return @value if @value && (!@refresh_at || @refresh_at > now)
          value = @provider.call
          raise ::ArgumentError, 'Expected AwsCredentials' if value && !value.is_a?(AwsCredentials)
          if value&.expires_at
            remaining = value.expires_at - now
            raise ::ArgumentError, 'Credential provider returned expired credentials' if remaining <= 0
            @refresh_at = value.expires_at - [@refresh_before, remaining / 2].min
          else
            @refresh_at = nil
          end
          @value = value
        end
      end
      def inspect = '#<CachedCredentials [redacted]>'
    end
  end
end

# frozen_string_literal: true
require 'time'
require 'date'
module CrucibleGenerated
  module Runtime
    class Timestamp
      attr_reader :seconds, :nanoseconds
      def initialize(seconds, nanoseconds = 0)
        unless seconds.is_a?(::Integer) && seconds.between?(-(2**63), 2**63 - 1) && nanoseconds.is_a?(::Integer) && nanoseconds.between?(0, 999_999_999)
          raise ::ArgumentError, 'Timestamp out of range'
        end
        @seconds, @nanoseconds = seconds, nanoseconds
        freeze
      end
      def self.from_epoch(value)
        number = value.is_a?(JsonNumber) ? value.rational : ::Kernel.Rational(value)
        nanos = number * 1_000_000_000
        raise ::ArgumentError, 'Timestamp exceeds nanosecond precision' unless nanos.denominator == 1
        new(*nanos.to_i.divmod(1_000_000_000))
      end
      def self.parse(value)
        match = /\A(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,9}))?(Z|[+-]\d{2}:\d{2})\z/.match(value)
        raise ::ArgumentError, 'Invalid date-time' unless match
        year, month, day, hour, minute, second = match.captures.take(6).map(&:to_i)
        zone = match[8]
        unless year.between?(1, 9999) && ::Date.valid_date?(year, month, day, ::Date::GREGORIAN) && hour < 24 && minute < 60 && second < 60 && (zone == 'Z' || (zone[1,2].to_i < 24 && zone[4,2].to_i < 60))
          raise ::ArgumentError, 'Invalid date-time components'
        end
        offset = zone == 'Z' ? 0 : (zone[1,2].to_i * 3600 + zone[4,2].to_i * 60) * (zone[0] == '-' ? -1 : 1)
        result = new(::Time.utc(year, month, day, hour, minute, second).to_i - offset, (match[7] || '').ljust(9, '0').to_i)
        result.checked_time
        result
      end
      def self.from_http_date(text)
        time = ::Time.httpdate(text)
        raise ::ArgumentError, 'Invalid HTTP date' unless time.httpdate == text
        result = new(time.to_i)
        result.checked_time
        result
      end
      def self.read(value, format)
        case format
        when 'epoch-seconds' then from_epoch(value.is_a?(::String) ? JsonNumber.new(value) : value)
        when 'http-date' then from_http_date(WireCodec.string(value))
        else parse(WireCodec.string(value))
        end
      end
      def wire(format)
        case format
        when 'epoch-seconds' then epoch
        when 'http-date' then http_date
        else date_time
        end
      end
      def checked_time
        time = to_time
        raise ::ArgumentError, 'Text timestamp year out of range' unless time.year.between?(1, 9999)
        time
      end
      def to_time = ::Time.at(@seconds, @nanoseconds, :nanosecond).utc
      def self.from_time(time) = new(time.to_i, time.nsec)
      def date_time
        base = checked_time.strftime('%Y-%m-%dT%H:%M:%S')
        fraction = @nanoseconds.zero? ? '' : '.' + @nanoseconds.to_s.rjust(9, '0').sub(/0+\z/, '')
        base + fraction + 'Z'
      end
      def http_date = checked_time.httpdate
      def epoch
        total = @seconds * 1_000_000_000 + @nanoseconds
        whole, fraction = total.abs.divmod(1_000_000_000)
        text = (total.negative? ? '-' : '') + whole.to_s
        text += '.' + fraction.to_s.rjust(9, '0').sub(/0+\z/, '') unless fraction.zero?
        JsonNumber.new(text)
      end
    end
  end
end

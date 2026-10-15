# frozen_string_literal: true
require 'base64'
require 'bigdecimal'
module CrucibleGenerated
  module Runtime
    class WireCodec
      attr_reader :schema
      def initialize(schema, models)
        @schema, @models = schema, models
      end
      def shape(target) = @schema.fetch('shapes').fetch(target)
      def context(target) = WireContext.new(self, target)
      def document(target, value, encode:)
        return nil if value.nil?
        info = shape(target)
        case info['kind']
        when 'structure', 'union'
          object, result = self.class.object(value), {}
          members = info.fetch('members')
          members.each do |name, member|
            result[name] = document(member.fetch('target'), object[name], encode: encode) if object.key?(name)
          end
          if info['kind'] == 'union'
            object.each do |name, item|
              result[name] = item unless members.key?(name)
            end
          end
          result
        when 'list' then self.class.items(value).map { |item| document(info.fetch('member').fetch('target'), item, encode: encode) }
        when 'map' then self.class.object(value).transform_values { |item| document(info.fetch('value').fetch('target'), item, encode: encode) }
        else value
        end
      end
      def equivalent(left, right)
        if (left.is_a?(JsonNumber) || left.is_a?(::Numeric)) && (right.is_a?(JsonNumber) || right.is_a?(::Numeric))
          return ::Kernel.BigDecimal(number(left).text) == ::Kernel.BigDecimal(number(right).text)
        end
        if left.is_a?(::Array) && right.is_a?(::Array)
          return left.size == right.size && left.zip(right).all? { |a, b| equivalent(a, b) }
        end
        if left.is_a?(::Hash) && right.is_a?(::Hash)
          return left.size == right.size && left.all? { |key, item| right.key?(key) && equivalent(item, right[key]) }
        end
        left == right
      end
      def canonical(value)
        case value
        when JsonNumber, ::Numeric
          decimal = ::Kernel.BigDecimal(number(value).text)
          # BigDecimal considers signed zeros equal but gives them different hashes.
          [:number, decimal.zero? ? ::Kernel.BigDecimal('0') : decimal]
        when ::Array then [:array, value.map { |item| canonical(item) }]
        when ::Hash then [:hash, value.to_h { |key, item| [key, canonical(item)] }]
        else [:scalar, value]
        end
      end
      def collection(values, shape)
        raise ::ArgumentError, 'Null in dense list' if !shape['sparse'] && values.any?(&:nil?)
        if shape['set']
          seen = {}
          values.each do |value|
            key = canonical(value)
            raise ::ArgumentError, 'Duplicate set member' if seen.key?(key)
            seen[key] = true
          end
        end
        values
      end
      def error(target, value, metadata)
        @models.fetch(target).from_wire(value, context(target), metadata)
      end
      def self.object(value)
        raise ::ArgumentError, 'Expected object' unless value.is_a?(::Hash) && value.keys.all? { |key| key.is_a?(::String) }
        value
      end
      def self.string(value)
        raise ::ArgumentError, 'Expected UTF-8 string' unless value.is_a?(::String) && value.dup.force_encoding(::Encoding::UTF_8).valid_encoding?
        value.dup.force_encoding(::Encoding::UTF_8)
      end
      def self.items(value)
        raise ::ArgumentError, 'Expected list' unless value.is_a?(::Array)
        value
      end
      def number(value)
        return value if value.is_a?(JsonNumber)
        return JsonNumber.new(value.to_s) if value.is_a?(::Integer) || value.is_a?(::Float)
        raise ::ArgumentError, 'Expected number'
      end
      def decode(value, context)
        shape = context.shape
        kind = shape.fetch('kind')
        raise ::ArgumentError, 'Unexpected null' if value.nil? && kind != 'document'
        case kind
        when 'unit'
          raise ::ArgumentError, 'Expected unit' unless self.class.object(value).empty?
          Unit.new
        when 'string' then self.class.string(value)
        when 'boolean'
          raise ::ArgumentError, 'Expected boolean' unless value == true || value == false
          value
        when 'byte' then number(value).integer(-128, 127)
        when 'short' then number(value).integer(-32768, 32767)
        when 'integer' then number(value).integer(-(2**31), 2**31 - 1)
        when 'long' then number(value).integer
        when 'bigInteger'
          rational = number(value).rational
          raise ::ArgumentError, 'Expected integer' unless rational.denominator == 1
          rational.to_i
        when 'bigDecimal'
          decimal = ::Kernel.BigDecimal(number(value).text)
          raise ::ArgumentError, 'Decimal outside representable range' unless decimal.finite?
          decimal
        when 'float', 'double'
          result = number(value).floating
          result = [result].pack('f').unpack1('f') if kind == 'float'
          raise ::ArgumentError, 'Float out of range' unless result.finite?
          result
        when 'blob'
          text = self.class.string(value)
          result = ::Base64.strict_decode64(text)
          raise ::ArgumentError, 'Expected padded standard base64' unless ::Base64.strict_encode64(result) == text
          result
        when 'timestamp'
          case context.format || shape['format']
          when 'epoch-seconds' then Timestamp.from_epoch(number(value))
          when 'http-date' then Timestamp.from_http_date(self.class.string(value))
          else Timestamp.parse(self.class.string(value))
          end
        when 'document' then JsonValue.new(value)
        when 'list'
          child = context.child(shape.fetch('member'))
          collection(self.class.items(value), shape).map { |item| item.nil? && shape['sparse'] ? nil : decode(item, child) }
        when 'map'
          child, key = context.child(shape.fetch('value')), context.child(shape.fetch('key'))
          raise ::ArgumentError, 'Null in dense map' if !shape['sparse'] && self.class.object(value).values.any?(&:nil?)
          self.class.object(value).to_h { |name, item| [decode(name, key), item.nil? && shape['sparse'] ? nil : decode(item, child)] }
        else
          @models.fetch(context.target).from_wire(value, context)
        end
      end
      def encode(value, context)
        shape = context.shape
        kind = shape.fetch('kind')
        case kind
        when 'unit'
          raise ::ArgumentError, 'Expected Unit' unless value.is_a?(Unit)
          {}
        when 'structure', 'union', 'enum', 'intEnum'
          raise ::ArgumentError, 'Wrong model type' unless value.is_a?(@models.fetch(context.target))
          value.to_wire(context)
        when 'document'
          raise ::ArgumentError, 'Expected JsonValue' unless value.is_a?(JsonValue)
          value.value
        when 'timestamp'
          raise ::ArgumentError, 'Expected Timestamp' unless value.is_a?(Timestamp)
          case context.format || shape['format']
          when 'epoch-seconds' then value.epoch
          when 'http-date' then value.http_date
          else value.date_time
          end
        when 'blob'
          raise ::ArgumentError, 'Expected binary String' unless value.is_a?(::String)
          ::Base64.strict_encode64(value)
        when 'list'
          child = context.child(shape.fetch('member'))
          collection(self.class.items(value).map { |item| item.nil? && shape['sparse'] ? nil : encode(item, child) }, shape)
        when 'map'
          raise ::ArgumentError, 'Expected Hash' unless value.is_a?(::Hash)
          child, key = context.child(shape.fetch('value')), context.child(shape.fetch('key'))
          result = value.to_h { |name, item| [encode(name, key), item.nil? && shape['sparse'] ? nil : encode(item, child)] }
          raise ::ArgumentError, 'Null in dense map' if !shape['sparse'] && result.values.any?(&:nil?)
          result
        when 'bigDecimal'
          raise ::ArgumentError, 'Expected BigDecimal' unless value.is_a?(::BigDecimal) && value.finite?
          JsonNumber.new(value.to_s)
        when 'bigInteger'
          raise ::ArgumentError, 'Expected Integer' unless value.is_a?(::Integer)
          value
        when 'byte', 'short', 'integer', 'long'
          raise ::ArgumentError, 'Expected Integer' unless value.is_a?(::Integer)
          decode(value, context)
        when 'float', 'double'
          result = decode(value, context)
          if kind == 'float'
            # Ruby stores binary64: serialize the shortest decimal that round-trips as binary32.
            bits = [result].pack('g')
            (1..9).each do |precision|
              candidate = ::Kernel.Float(::Kernel.format("%.#{precision}g", result))
              return candidate if [candidate].pack('g') == bits
            end
          end
          result
        else decode(value, context)
        end
      end
      def header(member, text)
        shape = shape(member.fetch('target'))
        kind = shape.fetch('kind')
        if kind == 'list'
          return [] if text.strip.empty?
          item = shape.fetch('member')
          item_shape = self.shape(item.fetch('target'))
          http_date = item_shape['kind'] == 'timestamp' && (item['format'] || item_shape['format']) == 'http-date'
          parts, part, quoted, escaped = [], +'', false, false
          append = lambda do
            value = part.strip
            value = ExactJson.decode(value) if value.start_with?('"')
            parts << header(shape.fetch('member'), value)
            part = +''
          end
          text.each_char do |character|
            # The comma after an HTTP-date weekday belongs to the date, not the list.
            if character == ',' && !quoted && !(http_date && part.strip.match?(/\A[A-Za-z]{3}\z/))
              append.call
              next
            end
            part << character
            if escaped then escaped = false
            elsif character == '\\' && quoted then escaped = true
            elsif character == '"' then quoted = !quoted
            end
          end
          raise ::ArgumentError, 'Unterminated quoted header' if quoted
          append.call
          return parts
        end
        if %w[byte short integer long intEnum bigInteger bigDecimal float double boolean].include?(kind) || (kind == 'timestamp' && member['format'] == 'epoch-seconds')
          ExactJson.decode(text)
        else text
        end
      end
      def bound(value)
        case value
        when ::String then value
        when JsonNumber then value.text
        when ::Integer, ::Float, true, false then ExactJson.encode(value)
        when ::Array
          value.map do |item|
            text = bound(item)
            text.empty? || /[,"\\]/.match?(text) || text.strip != text ? ExactJson.encode(text) : text
          end.join(', ')
        else raise ::ArgumentError, 'Expected bound scalar'
        end
      end
    end
  end
end

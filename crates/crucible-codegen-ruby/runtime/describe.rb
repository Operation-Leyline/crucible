# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # Formatting shared by `inspect` and `to_s` of every generated model, which
    # print as `TypeName(member=value, ...)`.
    #
    # Members the service left unset are omitted, nested models and collections
    # recurse, blobs show only their length, and `@sensitive` members show
    # {REDACTED} instead of their value. Models define `inspect` rather than
    # relying on Ruby's default, which would print every instance variable.
    module Describe
      # What a string representation shows in place of a `@sensitive` value.
      REDACTED = '*** Sensitive Data Redacted ***'

      # Replaces a present sensitive value; an absent one stays absent so it is omitted.
      # @return [String, nil]
      def self.redact(value) = value.nil? || value.equal?(UNSET) ? nil : REDACTED

      # Replaces a present blob with its length, so a payload never floods a log line.
      def self.bytes(value) = value.is_a?(::String) ? "<#{value.bytesize} bytes>" : value

      # @param type [String] the model's name
      # @param members [Hash{Symbol => Object}] member values in declaration order; unset ones are omitted
      # @return [String]
      def self.format(type, members)
        parts = members.filter_map { |name, member| "#{name}=#{render(member)}" unless member.nil? || member.equal?(UNSET) }
        "#{type}(#{parts.join(', ')})"
      end

      # Renders one value the way {format} renders a member.
      # @return [String]
      def self.render(value)
        case value
        when nil then 'null'
        when ::String then value.encoding == ::Encoding::BINARY || !value.valid_encoding? ? "<#{value.bytesize} bytes>" : value
        when ::Array then "[#{value.map { |item| render(item) }.join(', ')}]"
        when ::Hash then "{#{value.map { |key, item| "#{render(key)}=#{render(item)}" }.join(', ')}}"
        else value.to_s
        end
      end
    end
  end
end

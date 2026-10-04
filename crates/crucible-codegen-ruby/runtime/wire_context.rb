# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    class WireContext
      attr_reader :codec, :target, :format
      def initialize(codec, target, format = nil, depth = 0)
        raise ArgumentError, 'Model nesting limit' if depth > 255
        @codec, @target, @format, @depth = codec, target, format, depth
      end
      def shape = @codec.shape(@target)
      def child(member) = self.class.new(@codec, member.fetch('target'), member['format'], @depth + 1)
      def member(name) = child(shape.fetch('members').fetch(name))
      def read(name, object, optional: false)
        description = shape.fetch('members').fetch(name)
        value = object.fetch(name, description['default'])
        unless object.key?(name) || !value.nil?
          return UNSET if optional
          raise ArgumentError, 'Missing required member: ' + name
        end
        return nil if optional && value.nil?
        @codec.decode(value, child(description))
      end
      def put(object, name, value, optional: false)
        if optional
          return if value.equal?(UNSET)
          if value.nil?
            object[name] = nil
            return
          end
        end
        object[name] = @codec.encode(value, member(name))
      end
    end
  end
end

# frozen_string_literal: true
require 'stringio'
module CrucibleGenerated
  module Runtime
    # A streaming blob request body. Strings, seekable IOs, and factory blocks replay
    # the same bytes on every attempt; any other IO is one-shot and never retried.
    class ByteStream
      CHUNK = 64 * 1024
      attr_reader :length
      def self.from(value)
        case value
        when ByteStream then value
        when ::String then new(value)
        else
          raise ::ArgumentError, 'Streaming blob requires a String, an IO responding to read, or a ByteStream' unless value.respond_to?(:read)
          new(value)
        end
      end
      # ByteStream.new(io_or_string, length: nil) or ByteStream.new(length: n) { fresh_io }
      def initialize(source = nil, length: nil, &factory)
        raise ::ArgumentError, 'ByteStream takes a source or a factory block, not both' if source.nil? == factory.nil?
        raise ::ArgumentError, 'Stream length must be a nonnegative Integer' unless length.nil? || (length.is_a?(::Integer) && length >= 0)
        @mutex, @opened, @factory = ::Mutex.new, false, factory
        if source.is_a?(::String)
          @data = source.b.freeze
          length ||= @data.bytesize
          raise ::ArgumentError, 'Declared length differs from the String' unless length == @data.bytesize
        elsif source
          raise ::ArgumentError, 'Stream source must respond to read' unless source.respond_to?(:read)
          @io = source
          @start = begin
            source.pos if source.respond_to?(:pos) && source.respond_to?(:seek)
          rescue ::IOError, ::SystemCallError
            nil
          end
          length ||= source.size - @start if @start && source.respond_to?(:size) && source.size.is_a?(::Integer)
        end
        @length = length
      end
      def replayable? = !@data.nil? || !@factory.nil? || !@start.nil?
      # Opens the bytes for one attempt; only a replayable stream opens twice.
      def open
        @mutex.synchronize do
          return ::StringIO.new(@data) if @data
          if @factory
            io = @factory.call
            raise ::ArgumentError, 'ByteStream factory must return an object responding to read' unless io.respond_to?(:read)
            return io
          end
          if @opened
            raise ::ArgumentError, 'One-shot stream was already consumed' unless @start
            @io.seek(@start)
          end
          @opened = true
          @io
        end
      end
      # Reads a replayable stream once in bounded chunks, for checksums that precede the body.
      def each_chunk
        raise ::ArgumentError, 'A checksum over a one-shot stream needs a caller-supplied header' unless replayable?
        io = open
        while (chunk = io.read(CHUNK))
          yield chunk.b
        end
      ensure
        io.close if @factory && io.respond_to?(:close)
      end
      def owns_opened? = !@factory.nil?
      def inspect = "#<ByteStream length=#{@length.inspect} replayable=#{replayable?}>"
    end
    # One attempt's request body, read by the transport. It enforces the declared
    # length and the member's length constraints while bytes flow.
    class StreamBody
      attr_reader :length, :bytes_read
      def initialize(io, length:, minimum: nil, maximum: nil, owned: false)
        @io, @length, @minimum, @maximum, @bytes_read, @owned = io, length, minimum, maximum, 0, owned
      end
      def read(size = nil, buffer = nil)
        chunk = @io.read(size || ByteStream::CHUNK)
        if chunk.nil? || chunk.empty?
          raise ::ArgumentError, 'Stream ended before its declared length' if (@length && @bytes_read != @length) || (@minimum && @bytes_read < @minimum)
          buffer&.clear
          return size.nil? ? +'' : nil
        end
        @bytes_read += chunk.bytesize
        raise ::ArgumentError, 'Stream exceeds its declared length' if (@length && @bytes_read > @length) || (@maximum && @bytes_read > @maximum)
        return chunk.b unless buffer
        buffer.replace(chunk.b)
      end
      # Closes a body the client opened from a factory; caller IOs stay open.
      def close
        @io.close if @owned && @io.respond_to?(:close)
      end
    end
    # Receives a streamed response body: written to the caller's IO, yielded to the
    # caller's block, or (by default) buffered. Checksums are computed as bytes pass.
    class StreamSink
      attr_reader :bytes, :hashers
      def initialize(target: nil, block: nil, algorithms: [])
        raise ::ArgumentError, 'Response target must respond to write' unless target.nil? || target.respond_to?(:write)
        @target, @block, @bytes = target, block, 0
        @buffer = ::StringIO.new(+''.b) if target.nil? && block.nil?
        @hashers = algorithms.to_h { |algorithm| [algorithm, Checksums::Hasher.new(algorithm)] }
      end
      def call(chunk)
        @hashers.each_value { |hasher| hasher.update(chunk) }
        @bytes += chunk.bytesize
        if @block then @block.call(chunk)
        else (@target || @buffer).write(chunk)
        end
      end
      # The output member: the caller's IO, an empty IO after a block, or the rewound buffer.
      def body
        return @target if @target
        return ::StringIO.new(+''.b) if @block
        @buffer.rewind
        @buffer
      end
    end
  end
end

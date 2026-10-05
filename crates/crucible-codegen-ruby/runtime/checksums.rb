# frozen_string_literal: true
require 'base64'
require 'digest'
require 'openssl'
require 'zlib'
module CrucibleGenerated
  module Runtime
    # A response checksum differs from the received payload.
    class ChecksumError < ::StandardError
      attr_reader :algorithm
      def initialize(algorithm)
        @algorithm = algorithm
        super("Response #{algorithm} checksum mismatch")
      end
    end
    # HTTP checksums over the final transmitted bytes. CRC32C and CRC64NVME are
    # pure Ruby because the standard library only provides CRC32.
    module Checksums
      SUPPORTED = %w[CRC32 CRC32C CRC64NVME MD5 SHA1 SHA256 SHA512].freeze
      POLICIES = %w[WHEN_SUPPORTED WHEN_REQUIRED].freeze
      Settings = ::Struct.new(:algorithm, :header, :response, keyword_init: true)
      NONE = Settings.new(response: []).freeze
      def self.table(polynomial)
        (0..255).map do |byte|
          8.times.reduce(byte) { |value, _| value.odd? ? (value >> 1) ^ polynomial : value >> 1 }
        end.freeze
      end
      CRC32C = table(0x82F63B78)
      CRC64NVME = table(0x9A6C9329AC4BC9B5)
      def self.policy(value)
        text = value.to_s.upcase
        raise ::ArgumentError, 'Checksum policy must be when_supported or when_required' unless POLICIES.include?(text)
        text.freeze
      end
      # An incremental checksum, so streamed bodies are never buffered to be hashed.
      class Hasher
        attr_reader :algorithm
        def initialize(algorithm)
          @algorithm = algorithm
          case algorithm
          when 'CRC32' then @crc = 0
          when 'CRC32C' then @crc, @table, @bits = 0xFFFFFFFF, CRC32C, 32
          when 'CRC64NVME' then @crc, @table, @bits = 0xFFFFFFFFFFFFFFFF, CRC64NVME, 64
          when 'MD5' then @digest = ::Digest::MD5.new
          when 'SHA1' then @digest = ::Digest::SHA1.new
          when 'SHA256' then @digest = ::Digest::SHA256.new
          when 'SHA512' then @digest = ::Digest::SHA512.new
          else raise ::ArgumentError, "Unsupported checksum algorithm: #{algorithm}"
          end
        end
        def update(data)
          data = data.to_s.b
          if @digest then @digest.update(data)
          elsif @table
            crc, table = @crc, @table
            data.each_byte { |byte| crc = table[(crc ^ byte) & 0xff] ^ (crc >> 8) }
            @crc = crc
          else @crc = ::Zlib.crc32(data, @crc)
          end
          self
        end
        def base64
          raw = if @digest then @digest.digest
          elsif @table then @bits == 32 ? [@crc ^ 0xFFFFFFFF].pack('N') : [@crc ^ 0xFFFFFFFFFFFFFFFF].pack('Q>')
          else [@crc].pack('N')
          end
          ::Base64.strict_encode64(raw)
        end
      end
      def self.digest(algorithm, data) = Hasher.new(algorithm).update(data).base64
      def self.stream_digest(algorithm, stream)
        hasher = Hasher.new(algorithm)
        stream.each_chunk { |chunk| hasher.update(chunk) }
        hasher.base64
      end
      # Fills modeled defaults into the encoded input copy and returns the settings.
      def self.prepare(description, values, request_policy, response_policy)
        return NONE if description.nil?
        return Settings.new(algorithm: 'MD5', header: 'content-md5', response: []) if description.fetch('kind') == 'md5'
        algorithm = header = nil
        if (member = description['requestAlgorithmMember'])
          algorithm = values[member]
          if algorithm.nil? && (description['requestRequired'] || request_policy == 'WHEN_SUPPORTED')
            algorithm = values[member] = 'CRC32'
          end
          if algorithm
            raise ::ArgumentError, "Unsupported checksum algorithm: #{algorithm}" unless SUPPORTED.include?(algorithm)
            header = 'x-amz-checksum-' + algorithm.downcase
          end
        elsif description['requestRequired']
          algorithm, header = 'MD5', 'content-md5'
        end
        validate = false
        if (mode = description['validationModeMember'])
          values[mode] = 'ENABLED' if values[mode].nil? && response_policy == 'WHEN_SUPPORTED'
          validate = values[mode] == 'ENABLED'
        end
        response = validate ? description.fetch('responseAlgorithms', []).select { |name| SUPPORTED.include?(name) } : []
        Settings.new(algorithm: algorithm, header: header, response: response.freeze).freeze
      end
      # A caller-supplied header is never replaced.
      def self.request(settings, request)
        return request if settings.header.nil? || request.headers.key?(settings.header)
        body = request.body
        value = body.is_a?(ByteStream) ? stream_digest(settings.algorithm, body) : digest(settings.algorithm, body || '')
        request.with(headers: request.headers.merge(settings.header => value))
      end
      # Returns the validated algorithm, or nil when no modeled checksum was present.
      # Hashers carry digests of a streamed body that was never held in memory.
      def self.response(settings, headers, body, hashers: nil)
        settings.response.each do |algorithm|
          expected = headers['x-amz-checksum-' + algorithm.downcase]
          next if expected.nil?
          actual = hashers ? hashers.fetch(algorithm).base64 : digest(algorithm, body)
          raise ChecksumError, algorithm unless expected.bytesize == actual.bytesize && ::OpenSSL.fixed_length_secure_compare(expected, actual)
          return algorithm
        end
        nil
      end
    end
  end
end

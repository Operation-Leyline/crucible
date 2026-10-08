import 'dart:convert';
import 'dart:typed_data';
import 'package:crypto/crypto.dart';

/// When a client computes request checksums or validates response checksums.
enum ChecksumPolicy {
  /// Whenever the operation and service support it.
  whenSupported,

  /// Only when the operation requires it.
  whenRequired
}

/// A response checksum differs from the received payload.
final class ChecksumException implements Exception {
  /// The mismatching algorithm, such as `CRC32`.
  final String algorithm;
  const ChecksumException(this.algorithm);
  @override
  String toString() => 'ChecksumException: response $algorithm checksum mismatch';
}

const supportedChecksums = ['CRC32', 'CRC32C', 'CRC64NVME', 'MD5', 'SHA1', 'SHA256', 'SHA512'];

// Tables are 32-bit words so the same arithmetic is exact in JavaScript.
List<int> _table32(int polynomial) => List.generate(256, (byte) {
      var value = byte;
      for (var i = 0; i < 8; i++) {
        value = value & 1 == 1 ? (value >>> 1) ^ polynomial : value >>> 1;
      }
      return value;
    });
final _crc32 = _table32(0xEDB88320);
final _crc32c = _table32(0x82F63B78);
final _crc64 = () {
  const high = 0x9A6C9329, low = 0xAC4BC9B5;
  final result = <(int, int)>[];
  for (var byte = 0; byte < 256; byte++) {
    var hi = 0, lo = byte;
    for (var i = 0; i < 8; i++) {
      final odd = lo & 1 == 1;
      lo = ((lo >>> 1) | ((hi & 1) << 31)) & 0xFFFFFFFF;
      hi = hi >>> 1;
      if (odd) {
        hi ^= high;
        lo ^= low;
      }
    }
    result.add((hi, lo));
  }
  return result;
}();

Uint8List _crc32Bytes(List<int> table, List<int> data) {
  var value = 0xFFFFFFFF;
  for (final byte in data) {
    value = (table[(value ^ byte) & 255] ^ (value >>> 8)) & 0xFFFFFFFF;
  }
  return Uint8List(4)..buffer.asByteData().setUint32(0, (value ^ 0xFFFFFFFF) & 0xFFFFFFFF);
}

Uint8List _crc64Bytes(List<int> data) {
  var hi = 0xFFFFFFFF, lo = 0xFFFFFFFF;
  for (final byte in data) {
    final (th, tl) = _crc64[(lo ^ byte) & 255];
    lo = ((lo >>> 8) | ((hi & 255) << 24)) & 0xFFFFFFFF;
    hi = hi >>> 8;
    hi ^= th;
    lo ^= tl;
  }
  return Uint8List(8)
    ..buffer.asByteData().setUint32(0, (hi ^ 0xFFFFFFFF) & 0xFFFFFFFF)
    ..buffer.asByteData().setUint32(4, (lo ^ 0xFFFFFFFF) & 0xFFFFFFFF);
}

/// Base64 of the big-endian checksum or digest, as AWS checksum headers carry it.
String checksumDigest(String algorithm, List<int> data) => base64Encode(switch (algorithm) {
      'CRC32' => _crc32Bytes(_crc32, data),
      'CRC32C' => _crc32Bytes(_crc32c, data),
      'CRC64NVME' => _crc64Bytes(data),
      'MD5' => md5.convert(data).bytes,
      'SHA1' => sha1.convert(data).bytes,
      'SHA256' => sha256.convert(data).bytes,
      'SHA512' => sha512.convert(data).bytes,
      _ => throw ArgumentError('Unsupported checksum algorithm: $algorithm'),
    });

final class ChecksumSettings {
  final String? algorithm;
  final String? header;
  final List<String> response;
  const ChecksumSettings({this.algorithm, this.header, this.response = const []});
}

/// Resolves the request algorithm and response validation, injecting modeled
/// defaults into the wire values so their header bindings are sent.
ChecksumSettings prepareChecksum(Map<String, Object?>? description, Map<String, Object?> values,
    ChecksumPolicy requestPolicy, ChecksumPolicy responsePolicy) {
  if (description == null) return const ChecksumSettings();
  if (description['kind'] == 'md5') return const ChecksumSettings(algorithm: 'MD5', header: 'content-md5');
  String? algorithm, header;
  if (description['requestAlgorithmMember'] case final String member) {
    algorithm = values[member] as String?;
    if (algorithm == null && (description['requestRequired'] == true || requestPolicy == ChecksumPolicy.whenSupported)) {
      algorithm = values[member] = 'CRC32';
    }
    if (algorithm != null) {
      if (!supportedChecksums.contains(algorithm)) throw ArgumentError('Unsupported checksum algorithm: $algorithm');
      header = 'x-amz-checksum-${algorithm.toLowerCase()}';
    }
  } else if (description['requestRequired'] == true) {
    algorithm = 'MD5';
    header = 'content-md5';
  }
  var validate = false;
  if (description['validationModeMember'] case final String member) {
    var mode = values[member] as String?;
    if (mode == null && responsePolicy == ChecksumPolicy.whenSupported) mode = values[member] = 'ENABLED';
    validate = mode == 'ENABLED';
  }
  return ChecksumSettings(
      algorithm: algorithm,
      header: header,
      response: validate
          ? [for (final name in description['responseAlgorithms'] as List? ?? const []) if (supportedChecksums.contains(name)) name as String]
          : const []);
}

/// Adds the request checksum unless the caller already supplied that header.
Map<String, String> checksumRequest(ChecksumSettings settings, Map<String, String> headers, Uint8List? body) {
  final name = settings.header;
  if (name == null || headers.containsKey(name)) return headers;
  return {...headers, name: checksumDigest(settings.algorithm!, body ?? Uint8List(0))};
}

/// Validates the first modeled checksum present; returns its algorithm.
String? checksumResponse(ChecksumSettings settings, Map<String, String> headers, Uint8List body) {
  for (final algorithm in settings.response) {
    final expected = headers['x-amz-checksum-${algorithm.toLowerCase()}'];
    if (expected == null) continue;
    final actual = checksumDigest(algorithm, body);
    // Constant-time comparison: the expected value is attacker-controlled.
    var difference = expected.length ^ actual.length;
    for (var i = 0; i < expected.length && i < actual.length; i++) {
      difference |= expected.codeUnitAt(i) ^ actual.codeUnitAt(i);
    }
    if (difference != 0) throw ChecksumException(algorithm);
    return algorithm;
  }
  return null;
}

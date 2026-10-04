import '../runtime/json.dart';
import '../runtime/timestamp.dart';
import 'json_test.dart' show check, rejects;

void main() {
  for (final text in [
    '-9223372036854775808',
    '9223372036854775807.999999999',
    '-0.000000001',
    '0',
    '1.000000001',
  ]) {
    check(
      Timestamp.fromEpoch(JsonNumber(text)).toEpoch().text == text,
      'Exact epoch $text',
    );
  }
  for (final text in [
    '0001-01-01T00:00:00Z',
    '9999-12-31T23:59:59.999999999Z',
    '1969-12-31T23:59:59.999999999Z',
    '2024-02-29T12:00:00Z',
  ]) {
    check(
      Timestamp.parse(text).toIso8601String() == text,
      'ISO round trip $text',
    );
  }
  check(
    Timestamp.parse('1970-01-01T01:00:00+01:00').seconds == BigInt.zero,
    'Timezone offset',
  );
  check(
    Timestamp.parse('1969-12-31T23:00:00-01:00').seconds == BigInt.zero,
    'Negative timezone offset',
  );
  check(
    Timestamp.parseHttp('Thu, 01 Jan 1970 00:00:00 GMT').seconds == BigInt.zero,
    'HTTP timestamp',
  );
  check(
    Timestamp(BigInt.zero).toHttpDate() == 'Thu, 01 Jan 1970 00:00:00 GMT',
    'HTTP rendering',
  );
  for (final text in [
    '2023-02-29T00:00:00Z',
    '2024-01-01T24:00:00Z',
    '2024-01-01T00:00:00.0000000001Z',
    '2024-01-01T00:00:00+24:00',
    '0000-01-01T00:00:00Z',
  ]) {
    rejects(() => Timestamp.parse(text));
  }
  rejects(() => Timestamp.fromEpoch(JsonNumber('-9223372036854775808.1')));
  rejects(() => Timestamp.fromEpoch(JsonNumber('9223372036854775808')));
  rejects(() => Timestamp.parseHttp('Wed, 01 Jan 1970 00:00:00 GMT'));
  check(
    Timestamp.fromDateTime(DateTime.fromMillisecondsSinceEpoch(-1, isUtc: true))
            .toEpoch()
            .text ==
        '-0.001',
    'Negative DateTime normalization',
  );
  print('Exact timestamp qualification passed');
}

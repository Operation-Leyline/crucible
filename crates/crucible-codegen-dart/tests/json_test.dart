import '../runtime/json.dart';

void check(bool condition, String message) {
  if (!condition) throw StateError(message);
}

void rejects(void Function() action) {
  try {
    action();
  } on FormatException {
    return;
  }
  throw StateError('Expected FormatException');
}

void main() {
  final min = BigInt.parse('-9223372036854775808');
  final max = BigInt.parse('9223372036854775807');
  for (final text in [
    '-9223372036854775808',
    '9223372036854775807',
    '9007199254740993',
  ]) {
    final value = exactJsonDecode(text) as JsonNumber;
    check(
      value.integer(min: min, max: max) == BigInt.parse(text),
      'Exact long $text',
    );
    check(exactJsonEncode(value) == text, 'Round trip $text');
  }
  for (final text in ['0.00000000000000000001e20', '1.000e0', '100e-2']) {
    check(JsonNumber(text).scaled(0) == BigInt.one, 'Exact decimal $text');
  }
  check(
    JsonNumber('-0.000000001').scaled(9) == -BigInt.one,
    'Negative nanosecond',
  );
  check(
    JsonNumber('1e999999').text == '1e999999',
    'Document exponent retained',
  );
  rejects(() => JsonNumber('9223372036854775808').integer(min: min, max: max));
  rejects(() => JsonNumber('0.0000000001').scaled(9));
  rejects(() => JsonNumber('1e999999').scaled(0));
  for (final text in [
    '',
    '+1',
    '01',
    '1.',
    '.1',
    'NaN',
    '1e',
    '[1,]',
    '{"x":}',
    'true false',
    '"\n"',
  ]) {
    rejects(() => exactJsonDecode(text));
  }
  final document = JsonValue.parse(
    '{"z":[null,true,"a\\nb",1e1000],"a":9007199254740993}',
  );
  check(
    document.encode() ==
        '{"a":9007199254740993,"z":[null,true,"a\\nb",1e1000]}',
    'Stable exact document',
  );
  final source = <String, Object?>{
    'a': <Object?>[1],
  };
  final frozen = JsonValue(source);
  (source['a'] as List).add(2);
  check(frozen.encode() == '{"a":[1]}', 'Immutable document snapshot');
  rejects(() => exactJsonDecode('${'[' * 256}0${']' * 256}'));
  print('Exact JSON qualification passed');
}

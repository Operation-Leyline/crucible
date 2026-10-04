import 'dart:convert';

/// A JSON number retaining its original decimal spelling, including its exponent.
final class JsonNumber {
  final String text;
  static final _syntax = RegExp(
    r'^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$',
  );
  JsonNumber(this.text) {
    if (!_syntax.hasMatch(text))
      throw FormatException('Invalid JSON number', text);
  }

  /// Exact signed decimal coefficient at [fractionDigits] places.
  /// Bounded typed conversions reject overflow before allocating a large power.
  BigInt scaled(int fractionDigits, {int maxWholeDigits = 20}) {
    if (fractionDigits < 0 || fractionDigits > 9) {
      throw ArgumentError.value(fractionDigits, 'fractionDigits');
    }
    final negative = text.startsWith('-');
    final parts = (negative ? text.substring(1) : text).toLowerCase().split(
      'e',
    );
    final decimal = parts.first.split('.');
    var digits = decimal.join().replaceFirst(RegExp(r'^0+'), '');
    if (digits.isEmpty) return BigInt.zero;
    final exponent = parts.length == 1 ? 0 : int.tryParse(parts[1]);
    if (exponent == null || exponent.abs() > 10000) {
      throw const FormatException('Numeric exponent outside typed range');
    }
    final power =
        exponent -
        (decimal.length == 2 ? decimal[1].length : 0) +
        fractionDigits;
    if (digits.length + power > maxWholeDigits + fractionDigits) {
      throw const FormatException('Number outside typed range');
    }
    if (power < 0) {
      final cut = digits.length + power;
      if (cut <= 0 || digits.substring(cut).contains(RegExp(r'[1-9]'))) {
        throw const FormatException('Number exceeds supported precision');
      }
      digits = digits.substring(0, cut);
    } else {
      digits += '0' * power;
    }
    final result = BigInt.parse(digits);
    return negative ? -result : result;
  }

  BigInt integer({required BigInt min, required BigInt max}) {
    final value = scaled(0);
    if (value < min || value > max)
      throw const FormatException('Integer overflow');
    return value;
  }

  @override
  String toString() => text;
}

/// Immutable JSON document. Use [JsonNumber] or [BigInt] for exact numeric input.
/// Numeric tokens parsed from JSON never pass through a floating-point value.
final class JsonValue {
  final Object? value;
  JsonValue(Object? value) : value = _freeze(value, 0);
  JsonValue.parse(String text) : value = _freeze(exactJsonDecode(text), 0);
  String encode() => exactJsonEncode(value);
  static Object? _freeze(Object? value, int depth) {
    if (depth > 255)
      throw const FormatException('JSON nesting exceeds 255 levels');
    if (value == null ||
        value is bool ||
        value is String ||
        value is JsonNumber)
      return value;
    if (value is JsonValue) return _freeze(value.value, depth);
    if (value is BigInt) return JsonNumber(value.toString());
    if (value is num) return JsonNumber(value.toString());
    if (value is List)
      return List<Object?>.unmodifiable(
        value.map((item) => _freeze(item, depth + 1)),
      );
    if (value is Map<String, Object?>) {
      return Map<String, Object?>.unmodifiable(
        value.map((key, item) => MapEntry(key, _freeze(item, depth + 1))),
      );
    }
    throw ArgumentError('Not a JSON value: ${value.runtimeType}');
  }
}

Object? exactJsonDecode(String text) {
  final reader = _Reader(text);
  final result = reader.value(0);
  reader.whitespace();
  if (reader.offset != text.length)
    throw FormatException('Trailing JSON data', text, reader.offset);
  return result;
}

String exactJsonEncode(Object? value) {
  String render(Object? value, int depth) {
    if (depth > 255)
      throw const FormatException('JSON nesting exceeds 255 levels');
    if (value is JsonValue) return render(value.value, depth);
    if (value is JsonNumber) return value.text;
    if (value == null || value is String || value is bool)
      return jsonEncode(value);
    if (value is BigInt) return value.toString();
    if (value is num) return JsonNumber(value.toString()).text;
    if (value is List)
      return '[${value.map((v) => render(v, depth + 1)).join(',')}]';
    if (value is Map<String, Object?>) {
      final keys = value.keys.toList()..sort();
      return '{${keys.map((key) => '${jsonEncode(key)}:${render(value[key], depth + 1)}').join(',')}}';
    }
    throw ArgumentError('Not a JSON value: ${value.runtimeType}');
  }

  return render(value, 0);
}

final class _Reader {
  final String source;
  int offset = 0;
  _Reader(this.source);
  bool take(int code) {
    if (offset < source.length && source.codeUnitAt(offset) == code) {
      offset++;
      return true;
    }
    return false;
  }

  Never invalid(String message) =>
      throw FormatException(message, source, offset);
  void expect(int code) {
    if (!take(code)) invalid('Unexpected JSON token');
  }

  void whitespace() {
    while (offset < source.length &&
        const [9, 10, 13, 32].contains(source.codeUnitAt(offset))) {
      offset++;
    }
  }

  String string() {
    final start = offset;
    expect(34);
    var escaped = false;
    while (offset < source.length) {
      final code = source.codeUnitAt(offset++);
      if (escaped) {
        escaped = false;
      } else if (code == 92) {
        escaped = true;
      } else if (code == 34) {
        return jsonDecode(source.substring(start, offset)) as String;
      }
    }
    invalid('Unterminated JSON string');
  }

  void digits() {
    final start = offset;
    while (offset < source.length) {
      final code = source.codeUnitAt(offset);
      if (code < 48 || code > 57) break;
      offset++;
    }
    if (offset == start) invalid('Expected decimal digit');
  }

  void word(String word) {
    if (!source.startsWith(word, offset)) invalid('Unexpected JSON token');
    offset += word.length;
  }

  Object? value(int depth) {
    if (depth > 255) invalid('JSON nesting exceeds 255 levels');
    whitespace();
    if (offset == source.length) invalid('Missing JSON value');
    switch (source.codeUnitAt(offset)) {
      case 34:
        return string();
      case 123:
        offset++;
        whitespace();
        final result = <String, Object?>{};
        if (take(125)) return result;
        while (true) {
          final key = string();
          whitespace();
          expect(58);
          result[key] = value(depth + 1);
          whitespace();
          if (take(125)) return result;
          expect(44);
          whitespace();
        }
      case 91:
        offset++;
        whitespace();
        final result = <Object?>[];
        if (take(93)) return result;
        while (true) {
          result.add(value(depth + 1));
          whitespace();
          if (take(93)) return result;
          expect(44);
          whitespace();
        }
      case 116:
        word('true');
        return true;
      case 102:
        word('false');
        return false;
      case 110:
        word('null');
        return null;
      default:
        final start = offset;
        take(45);
        if (!take(48)) digits();
        if (take(46)) digits();
        if (take(101) || take(69)) {
          if (!take(43)) take(45);
          digits();
        }
        return JsonNumber(source.substring(start, offset));
    }
  }
}

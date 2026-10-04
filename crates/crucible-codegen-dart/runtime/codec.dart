import 'dart:convert';
import 'dart:typed_data';

import 'json.dart';
import 'timestamp.dart';

/// An optional member's three states: omitted, explicit null, and supplied value.
final class Field<T> {
  final bool isSet;
  final T? value;
  const Field.unset() : isSet = false, value = null;
  const Field.nullValue() : isSet = true, value = null;
  const Field.value(T this.value) : isSet = true;
}

abstract interface class WireModel {
  Object? toWire(WireContext context);
}

typedef ModelDecoder = Object Function(
  String target,
  Object? value,
  WireContext context,
);

Map<String, Object?> wireObject(Object? value) {
  if (value is! Map<String, Object?>)
    throw const FormatException('Expected JSON object');
  return value;
}

List<Object?> wireArray(Object? value) {
  if (value is! List<Object?>)
    throw const FormatException('Expected JSON array');
  return value;
}

String wireString(Object? value) {
  if (value is! String) throw const FormatException('Expected JSON string');
  return value;
}

final class WireContext {
  final WireCodec codec;
  final String target;
  final String format;
  final int depth;
  WireContext(this.codec, this.target, this.format, [this.depth = 0]) {
    if (depth > 255)
      throw const FormatException('Model nesting exceeds 255 levels');
  }
  Map<String, Object?> get shape => codec.shape(target);
  WireContext child(Object? member) {
    final info = wireObject(member);
    return codec.context(
      wireString(info['target']),
      format: info['format'] as String?,
      depth: depth + 1,
    );
  }

  WireContext member(String name) => child(wireObject(shape['members'])[name]);
  T read<T>(String name, Map<String, Object?> values) {
    final info = wireObject(wireObject(shape['members'])[name]);
    final value = values.containsKey(name) ? values[name] : info['default'];
    return codec.decode(value, child(info)) as T;
  }

  Field<T> optional<T>(String name, Map<String, Object?> values) {
    final info = wireObject(wireObject(shape['members'])[name]);
    if (!values.containsKey(name) && info['default'] == null)
      return Field<T>.unset();
    final value = values.containsKey(name) ? values[name] : info['default'];
    return value == null
        ? Field<T>.nullValue()
        : Field<T>.value(codec.decode(value, child(info)) as T);
  }

  void put(Map<String, Object?> values, String name, Object? value) {
    if (value is Field) {
      if (!value.isSet) return;
      values[name] = value.value == null
          ? null
          : codec.encode(value.value, member(name));
    } else {
      values[name] = codec.encode(value, member(name));
    }
  }
}

final class WireCodec {
  final Map<String, Object?> schema;
  final ModelDecoder models;
  WireCodec(this.schema, this.models);
  Map<String, Object?> shape(String target) =>
      wireObject(wireObject(schema['shapes'])[target]);
  WireContext context(String target, {String? format, int depth = 0}) =>
      WireContext(
        this,
        target,
        format ?? shape(target)['format'] as String? ?? 'date-time',
        depth,
      );
  static final _ranges = <String, (BigInt, BigInt)>{
    'byte': (BigInt.from(-128), BigInt.from(127)),
    'short': (BigInt.from(-32768), BigInt.from(32767)),
    'integer': (BigInt.from(-2147483648), BigInt.from(2147483647)),
    'long': (
      BigInt.parse('-9223372036854775808'),
      BigInt.parse('9223372036854775807'),
    ),
  };
  Object decode(Object? value, WireContext context) {
    final kind = context.shape['kind'];
    if (_ranges.containsKey(kind)) {
      if (value is! JsonNumber)
        throw const FormatException('Expected JSON number');
      final range = _ranges[kind]!;
      final number = value.integer(min: range.$1, max: range.$2);
      return kind == 'long' ? number : number.toInt();
    }
    switch (kind) {
      case 'string':
        return wireString(value);
      case 'boolean':
        if (value is! bool)
          throw const FormatException('Expected JSON boolean');
        return value;
      case 'float':
      case 'double':
        if (value is! JsonNumber)
          throw const FormatException('Expected JSON number');
        final number = double.parse(value.text);
        final result = kind == 'float'
            ? (Float32List(1)..[0] = number)[0]
            : number;
        if (!result.isFinite)
          throw const FormatException('Floating-point overflow');
        return result;
      case 'blob':
        final text = wireString(value);
        // The wire contract is padded standard base64, not URL-safe variants.
        if (text.length % 4 != 0 ||
            !RegExp(
              r'^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$',
            ).hasMatch(text)) {
          throw const FormatException('Invalid base64 blob');
        }
        return base64Decode(text);
      case 'document':
        return JsonValue(value);
      case 'timestamp':
        return switch (context.format) {
          'epoch-seconds' => Timestamp.fromEpoch(
            value is JsonNumber
                ? value
                : throw const FormatException('Expected epoch number'),
          ),
          'http-date' => Timestamp.parseHttp(wireString(value)),
          _ => Timestamp.parse(wireString(value)),
        };
      default:
        return models(context.target, value, context);
    }
  }

  Object? encode(Object? value, WireContext context) {
    final shape = context.shape;
    final kind = shape['kind'];
    if (_ranges.containsKey(kind)) {
      if (kind == 'long' ? value is! BigInt : value is! int)
        throw const FormatException('Expected integer');
      final number = JsonNumber(value.toString());
      final range = _ranges[kind]!;
      number.integer(min: range.$1, max: range.$2);
      return number;
    }
    switch (kind) {
      case 'string':
        return wireString(value);
      case 'boolean':
        if (value is! bool) throw const FormatException('Expected boolean');
        return value;
      case 'float':
      case 'double':
        if (value is! double) throw const FormatException('Expected double');
        final result = kind == 'float'
            ? (Float32List(1)..[0] = value)[0]
            : value;
        if (!result.isFinite)
          throw const FormatException('Non-finite floating-point number');
        return JsonNumber(result.toString());
      case 'blob':
        if (value is! Uint8List)
          throw const FormatException('Expected Uint8List');
        return base64Encode(value);
      case 'document':
        if (value is! JsonValue)
          throw const FormatException('Expected JsonValue');
        return value.value;
      case 'timestamp':
        if (value is! Timestamp)
          throw const FormatException('Expected Timestamp');
        return switch (context.format) {
          'epoch-seconds' => value.toEpoch(),
          'http-date' => value.toHttpDate(),
          _ => value.toIso8601String(),
        };
      case 'list':
        if (value is! List) throw const FormatException('Expected List');
        final child = context.child(shape['member']);
        return value.map((item) => encode(item, child)).toList();
      case 'map':
        if (value is! Map) throw const FormatException('Expected Map');
        final key = context.child(shape['key']);
        final child = context.child(shape['value']);
        return value.map(
          (name, item) =>
              MapEntry(wireString(encode(name, key)), encode(item, child)),
        );
      default:
        if (value is! WireModel)
          throw const FormatException('Expected generated model');
        return value.toWire(context);
    }
  }

  Object? header(Object? member, String text) {
    final info = wireObject(member);
    final shape = this.shape(wireString(info['target']));
    switch (shape['kind']) {
      case 'list':
        if (text.trim().isEmpty) return <Object?>[];
        final values = <Object?>[];
        final part = StringBuffer();
        var quoted = false;
        var escaped = false;
        void append() {
          final segment = part.toString().trim();
          values.add(
            header(
              shape['member'],
              segment.startsWith('"')
                  ? wireString(exactJsonDecode(segment))
                  : segment,
            ),
          );
          part.clear();
        }
        for (final rune in text.runes) {
          if (rune == 44 && !quoted) {
            append();
            continue;
          }
          part.writeCharCode(rune);
          if (escaped) {
            escaped = false;
          } else if (rune == 92 && quoted) {
            escaped = true;
          } else if (rune == 34) {
            quoted = !quoted;
          }
        }
        if (quoted) throw const FormatException('Unterminated quoted header');
        append();
        return values;
      case 'byte':
      case 'short':
      case 'integer':
      case 'long':
      case 'float':
      case 'double':
      case 'boolean':
        return exactJsonDecode(text);
      case 'timestamp':
        return info['format'] == 'epoch-seconds' ? exactJsonDecode(text) : text;
      default:
        return text;
    }
  }

  String bound(Object? value) {
    if (value is String || value is JsonNumber || value is bool)
      return value.toString();
    if (value is List) {
      return value
          .map((item) {
            final text = bound(item);
            return text.isEmpty ||
                    text.contains(',') ||
                    text.contains('"') ||
                    text.contains('\\') ||
                    text.trim() != text
                ? jsonEncode(text)
                : text;
          })
          .join(', ');
    }
    throw const FormatException('Expected bound scalar');
  }
}

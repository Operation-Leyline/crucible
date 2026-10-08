import 'dart:convert';
import 'dart:typed_data';

import 'json.dart';
import 'timestamp.dart';

/// An optional member's three states: omitted, explicit null, and supplied value.
final class Field<T> {
  final bool isSet;
  final T? value;
  const Field.unset()
      : isSet = false,
        value = null;
  const Field.nullValue()
      : isSet = true,
        value = null;
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
      values[name] =
          value.value == null ? null : codec.encode(value.value, member(name));
    } else {
      values[name] = codec.encode(value, member(name));
    }
  }
}

final class WireCodec {
  final Map<String, Object?> schema;
  final ModelDecoder models;
  WireCodec(this.schema, this.models);
  late final Map<String, Object?> _settings = schema['protocolSettings'] as Map<String, Object?>? ?? const {};
  bool get _nonFiniteStrings => _settings['nonFiniteFloatStrings'] == true;
  Map<String, Object?> mapping(Object? value, Map<String, Object?> shape) {
    final map = wireObject(value);
    if (shape['sparse'] != true && map.values.contains(null)) throw const FormatException('Null in dense map');
    return map;
  }
  List<Object?> collection(Object? value, Map<String, Object?> shape) {
    final values = wireArray(value);
    if (shape['sparse'] != true && values.contains(null)) throw const FormatException('Null in dense list');
    if (shape['set'] == true) {
      for (var i = 0; i < values.length; i++) {
        if (values.take(i).any((item) => jsonEquivalent(item, values[i]))) throw const FormatException('Duplicate set member');
      }
    }
    return values;
  }
  int intEnumValue(Object? value) {
    if (value is! JsonNumber) throw const FormatException('Expected integer enum');
    return value.integer(min: BigInt.from(-2147483648), max: BigInt.from(2147483647)).toInt();
  }
  Object? document(String target, Object? value, {required bool encode}) {
    if (value == null) return null;
    final info = shape(target);
    switch (info['kind']) {
      case 'structure':
      case 'union':
        final object = wireObject(value);
        final members = wireObject(info['members']);
        final result = <String, Object?>{};
        final known = <String>{};
        final wireNames = <String>{};
        for (final entry in members.entries) {
          final member = wireObject(entry.value);
          final wireName = member['jsonName'] as String? ?? entry.key;
          final source = encode ? entry.key : wireName;
          final target = wireString(member['target']);
          known.add(source);
          wireNames.add(wireName);
          if (!object.containsKey(source)) continue;
          // AWS JSON protocols treat a null structure member as absent.
          if (object[source] == null && info['kind'] == 'structure' && _settings['omitNullStructureMembers'] == true && shape(target)['kind'] != 'document') continue;
          result[encode ? wireName : entry.key] = document(target, object[source], encode: encode);
        }
        if (info['kind'] == 'union') {
          for (final entry in object.entries) {
            if (!encode && entry.key == '__type' && !known.contains(entry.key) && _settings['ignoreUnknownUnionType'] == true) continue;
            if (!known.contains(entry.key)) {
              if (encode && wireNames.contains(entry.key)) throw const FormatException('Unknown variant name is declared');
              result[entry.key] = entry.value;
            }
          }
        }
        return result;
      case 'list':
        return wireArray(value).map((item) => document(wireString(wireObject(info['member'])['target']), item, encode: encode)).toList();
      case 'map':
        return wireObject(value).map((name, item) => MapEntry(name, document(wireString(wireObject(info['value'])['target']), item, encode: encode)));
      default: return value;
    }
  }
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
      case 'bigInteger':
        if (value is! JsonNumber) throw const FormatException('Expected JSON number');
        final (coefficient, exponent) = value.canonical;
        if (exponent.isNegative) throw const FormatException('Expected integer');
        if (exponent > BigInt.from(1000000)) throw const FormatException('Integer exceeds allocation limit');
        return BigInt.parse(coefficient) * BigInt.from(10).pow(exponent.toInt());
      case 'bigDecimal':
        if (value is! JsonNumber) throw const FormatException('Expected JSON number');
        return BigDecimal(value.text);
      case 'string':
        return wireString(value);
      case 'boolean':
        if (value is! bool)
          throw const FormatException('Expected JSON boolean');
        return value;
      case 'float':
      case 'double':
        if (value is String && _nonFiniteStrings) {
          return switch (value) {
            'NaN' => double.nan,
            'Infinity' => double.infinity,
            '-Infinity' => double.negativeInfinity,
            _ => throw const FormatException('Invalid non-finite number'),
          };
        }
        if (value is! JsonNumber)
          throw const FormatException('Expected JSON number');
        final number = double.parse(value.text);
        final result =
            kind == 'float' ? (Float32List(1)..[0] = number)[0] : number;
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
      case 'bigInteger':
        if (value is! BigInt) throw const FormatException('Expected BigInt');
        return JsonNumber(value.toString());
      case 'bigDecimal':
        if (value is! BigDecimal) throw const FormatException('Expected BigDecimal');
        return value.number;
      case 'string':
        return wireString(value);
      case 'boolean':
        if (value is! bool) throw const FormatException('Expected boolean');
        return value;
      case 'float':
      case 'double':
        if (value is! double) throw const FormatException('Expected double');
        if (_nonFiniteStrings && !value.isFinite) return value.isNaN ? 'NaN' : value > 0 ? 'Infinity' : '-Infinity';
        final result =
            kind == 'float' ? (Float32List(1)..[0] = value)[0] : value;
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
        final result = value.map((item) => item == null && shape['sparse'] == true ? null : encode(item, child)).toList();
        collection(result, shape);
        return result;
      case 'map':
        if (value is! Map) throw const FormatException('Expected Map');
        final key = context.child(shape['key']);
        final child = context.child(shape['value']);
        final result = value.map<String, Object?>(
          (name, item) =>
              MapEntry(wireString(encode(name, key)), item == null && shape['sparse'] == true ? null : encode(item, child)),
        );
        return mapping(result, shape);
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
        final item = wireObject(shape['member']);
        final itemShape = this.shape(wireString(item['target']));
        final httpDate = itemShape['kind'] == 'timestamp' &&
            (item['format'] ?? itemShape['format']) == 'http-date';
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
          // The comma after an HTTP-date weekday belongs to the date, not the list.
          if (rune == 44 &&
              !quoted &&
              !(httpDate &&
                  RegExp(r'^[A-Za-z]{3}$').hasMatch(part.toString().trim()))) {
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
      case 'intEnum':
      case 'bigInteger':
      case 'bigDecimal':
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
      return value.map((item) {
        final text = bound(item);
        return text.isEmpty ||
                text.contains(',') ||
                text.contains('"') ||
                text.contains('\\') ||
                text.trim() != text
            ? jsonEncode(text)
            : text;
      }).join(', ');
    }
    throw const FormatException('Expected bound scalar');
  }
}

import 'json.dart';

/// Evaluates the portable JMESPath tree produced by crucible-smithy.
///
/// Values are exact JSON: null, bool, String, [JsonNumber], List and
/// Map<String, Object?>. Numbers never pass through floating point.
Object? queryEvaluate(Map<String, Object?> node, Object? input) => _eval(node, input);

Never _fail(String message) => throw FormatException(message);
String _text(Object? value) => value is String ? value : _fail('JMESPath expected string');
List<Object?> _array(Object? value) => value is List<Object?> ? value : _fail('JMESPath expected array');
Map<String, Object?> _object(Object? value) =>
    value is Map<String, Object?> ? value : _fail('JMESPath expected object');
JsonNumber _number(Object? value) => value is JsonNumber ? value : _fail('JMESPath expected number');
int _integer(Object? value) {
  final number = _number(value).scaled(0, maxWholeDigits: 15);
  return number.toInt();
}

bool queryTruth(Object? value) => switch (value) {
      null => false,
      bool v => v,
      String v => v.isNotEmpty,
      List v => v.isNotEmpty,
      Map v => v.isNotEmpty,
      _ => true,
    };

/// Exact (numerator, denominator-exponent) form: value = digits * 10^exponent.
(BigInt, int) _decimal(JsonNumber value) {
  final (digits, exponent) = value.canonical;
  if (exponent.abs() > BigInt.from(1000000)) _fail('JMESPath number exponent exceeds supported precision');
  return (BigInt.parse(digits), exponent.toInt());
}

int compareNumbers(JsonNumber left, JsonNumber right) {
  final (a, ea) = _decimal(left);
  final (b, eb) = _decimal(right);
  if (a.sign != b.sign) return a.sign.compareTo(b.sign);
  if (a.sign == 0) return 0;
  // Compare magnitudes by digit count before scaling to avoid huge powers.
  final la = a.abs().toString().length + ea, lb = b.abs().toString().length + eb;
  if (la != lb) return a.sign * la.compareTo(lb);
  final shift = ea < eb ? ea : eb;
  return (a * BigInt.from(10).pow(ea - shift)).compareTo(b * BigInt.from(10).pow(eb - shift));
}

int _compareText(String left, String right) {
  final a = left.runes.toList(), b = right.runes.toList();
  for (var i = 0; i < a.length && i < b.length; i++) {
    if (a[i] != b[i]) return a[i].compareTo(b[i]);
  }
  return a.length.compareTo(b.length);
}

int _compare(Object? left, Object? right) {
  if (left is JsonNumber) return compareNumbers(left, _number(right));
  return _compareText(_text(left), _text(right));
}

JsonNumber _render(BigInt digits, int exponent) {
  if (digits == BigInt.zero) return JsonNumber('0');
  if (exponent >= 0) return JsonNumber((digits * BigInt.from(10).pow(exponent)).toString());
  final negative = digits.isNegative;
  final text = digits.abs().toString().padLeft(-exponent + 1, '0');
  final cut = text.length + exponent;
  final fraction = text.substring(cut).replaceFirst(RegExp(r'0+$'), '');
  return JsonNumber('${negative ? '-' : ''}${text.substring(0, cut)}${fraction.isEmpty ? '' : '.$fraction'}');
}

Object? _eval(Map<String, Object?> node, Object? input) {
  Object? child(String name) => _eval(_object(node[name]), input);
  switch (_text(node['kind'])) {
    case 'identity':
      return input;
    case 'literal':
      return node['value'];
    case 'field':
      return input is Map<String, Object?> ? input[_text(node['name'])] : null;
    case 'index':
      if (input is! List<Object?>) return null;
      var index = _integer(node['index']);
      if (index < 0) index += input.length;
      return index < 0 || index >= input.length ? null : input[index];
    case 'subexpression':
      return _eval(_object(node['right']), child('left'));
    case 'projection':
      final left = child('left');
      if (left is! List<Object?>) return null;
      return [
        for (final item in left)
          if (_eval(_object(node['right']), item) case final value?) value
      ];
    case 'flatten':
      final value = child('expression');
      if (value is! List<Object?>) return null;
      return [for (final item in value) ...(item is List<Object?> ? item : [item])];
    case 'objectValues':
      final value = child('expression');
      return value is Map<String, Object?> ? _values(value) : null;
    case 'multiList':
      if (input == null) return null;
      return [for (final item in _array(node['expressions'])) _eval(_object(item), input)];
    case 'multiHash':
      if (input == null) return null;
      return {
        for (final item in _array(node['entries']))
          _text(_object(item)['key']): _eval(_object(_object(item)['value']), input)
      };
    case 'and':
      final left = child('left');
      return queryTruth(left) ? child('right') : left;
    case 'or':
      final left = child('left');
      return queryTruth(left) ? left : child('right');
    case 'not':
      return !queryTruth(child('expression'));
    case 'condition':
      return queryTruth(child('predicate')) ? child('then') : null;
    case 'compare':
      final left = child('left'), right = child('right');
      final comparator = _text(node['comparator']);
      if (comparator == 'equal') return jsonEquivalent(left, right);
      if (comparator == 'notEqual') return !jsonEquivalent(left, right);
      if (left is! JsonNumber || right is! JsonNumber) return null;
      final order = compareNumbers(left, right);
      return switch (comparator) {
        'lessThan' => order < 0,
        'lessThanEqual' => order <= 0,
        'greaterThan' => order > 0,
        'greaterThanEqual' => order >= 0,
        _ => _fail('Unknown JMESPath comparator'),
      };
    case 'slice':
      if (input is! List<Object?>) return null;
      final step = node['step'] == null ? 1 : _integer(node['step']);
      if (step == 0) _fail('Zero slice step');
      int boundary(String name, int fallback) {
        if (node[name] == null) return fallback;
        var value = _integer(node[name]);
        if (value < 0) value += input.length;
        return step > 0 ? value.clamp(0, input.length) : value.clamp(-1, input.length - 1);
      }
      final start = boundary('start', step > 0 ? 0 : input.length - 1);
      final stop = boundary('stop', step > 0 ? input.length : -1);
      return [for (var i = start; step > 0 ? i < stop : i > stop; i += step) input[i]];
    case 'function':
      return _function(_text(node['function']), _array(node['arguments']), input);
  }
  _fail('Unknown compiled JMESPath node');
}

List<String> _keys(Map<String, Object?> object) => object.keys.toList()..sort(_compareText);
List<Object?> _values(Map<String, Object?> object) => [for (final key in _keys(object)) object[key]];

List<Object?> _ordered(Object? value) {
  final result = List<Object?>.of(_array(value));
  if (result.isNotEmpty) {
    final numeric = result.first is JsonNumber;
    for (final item in result) {
      numeric ? _number(item) : _text(item);
    }
  }
  _stableSort(result, _compare);
  return result;
}

void _stableSort<T>(List<T> values, int Function(T, T) compare) {
  final indexed = [for (var i = 0; i < values.length; i++) (i, values[i])];
  indexed.sort((a, b) {
    final order = compare(a.$2, b.$2);
    return order != 0 ? order : a.$1.compareTo(b.$1);
  });
  for (var i = 0; i < values.length; i++) {
    values[i] = indexed[i].$2;
  }
}

Object? _reference(List<Object?> nodes, int index, Object? input) {
  final node = _object(nodes[index]);
  if (node['kind'] != 'expressionReference') _fail('JMESPath expected expression reference');
  return _eval(_object(node['expression']), input);
}

Object? _function(String name, List<Object?> nodes, Object? input) {
  final variadic = name == 'merge' || name == 'not_null';
  final arity = const {'contains', 'starts_with', 'ends_with', 'join', 'map', 'max_by', 'min_by', 'sort_by'}.contains(name) ? 2 : 1;
  if (variadic ? nodes.isEmpty : nodes.length != arity) _fail('JMESPath invalid function arity');
  Object? arg(int index) => _eval(_object(nodes[index]), input);
  switch (name) {
    case 'abs':
      final text = _number(arg(0)).text;
      return JsonNumber(text.startsWith('-') ? text.substring(1) : text);
    case 'ceil':
    case 'floor':
      final (digits, exponent) = _decimal(_number(arg(0)));
      if (exponent >= 0) return _render(digits, exponent);
      final divisor = BigInt.from(10).pow(-exponent);
      var quotient = digits ~/ divisor;
      final remainder = digits.remainder(divisor);
      if (remainder != BigInt.zero) {
        if (name == 'ceil' && digits > BigInt.zero) quotient += BigInt.one;
        if (name == 'floor' && digits < BigInt.zero) quotient -= BigInt.one;
      }
      return JsonNumber(quotient.toString());
    case 'sum':
    case 'avg':
      final list = _array(arg(0));
      if (name == 'avg' && list.isEmpty) return null;
      var total = BigInt.zero;
      var scale = 0;
      for (final item in list) {
        final (digits, exponent) = _decimal(_number(item));
        if (exponent < scale) {
          total *= BigInt.from(10).pow(scale - exponent);
          scale = exponent;
        }
        total += digits * BigInt.from(10).pow(exponent - scale);
      }
      if (name == 'sum') return _render(total, scale);
      // 34 significant fractional digits, truncated toward zero, like decimal128.
      const precision = 34;
      final scaled = total * BigInt.from(10).pow(precision) ~/ BigInt.from(list.length);
      return _render(scaled, scale - precision);
    case 'contains':
      final source = arg(0), wanted = arg(1);
      if (source is String) return source.contains(_text(wanted));
      return _array(source).any((item) => jsonEquivalent(item, wanted));
    case 'starts_with':
      return _text(arg(0)).startsWith(_text(arg(1)));
    case 'ends_with':
      return _text(arg(0)).endsWith(_text(arg(1)));
    case 'join':
      final separator = _text(arg(0));
      return _array(arg(1)).map(_text).join(separator);
    case 'keys':
      return _keys(_object(arg(0)));
    case 'values':
      return _values(_object(arg(0)));
    case 'length':
      final value = arg(0);
      final length = value is List ? value.length : value is Map ? value.length : _text(value).runes.length;
      return JsonNumber('$length');
    case 'map':
      return [for (final item in _array(arg(1))) _reference(nodes, 0, item)];
    case 'max':
    case 'min':
    case 'sort':
      final list = _ordered(arg(0));
      if (name == 'sort') return list;
      if (list.isEmpty) return null;
      return name == 'max' ? list.last : list.first;
    case 'max_by':
    case 'min_by':
    case 'sort_by':
      final list = _array(arg(0));
      final keys = [for (final item in list) _reference(nodes, 1, item)];
      _ordered(keys);
      if (list.isEmpty) return name == 'sort_by' ? <Object?>[] : null;
      if (name == 'sort_by') {
        final indices = List.generate(list.length, (i) => i);
        _stableSort<int>(indices, (a, b) => _compare(keys[a], keys[b]));
        return [for (final index in indices) list[index]];
      }
      var selected = 0;
      for (var i = 1; i < list.length; i++) {
        final order = _compare(keys[i], keys[selected]);
        if (name == 'max_by' ? order > 0 : order < 0) selected = i;
      }
      return list[selected];
    case 'merge':
      return {for (var i = 0; i < nodes.length; i++) ..._object(arg(i))};
    case 'not_null':
      for (var i = 0; i < nodes.length; i++) {
        if (arg(i) case final value?) return value;
      }
      return null;
    case 'reverse':
      final value = arg(0);
      if (value is List<Object?>) return value.reversed.toList();
      return String.fromCharCodes(_text(value).runes.toList().reversed);
    case 'to_array':
      final value = arg(0);
      return value is List ? value : [value];
    case 'to_number':
      final value = arg(0);
      if (value is JsonNumber) return value;
      if (value is String) {
        try {
          return JsonNumber(value);
        } on FormatException {
          return null;
        }
      }
      return null;
    case 'to_string':
      final value = arg(0);
      return value is String ? value : exactJsonEncode(value);
    case 'type':
      return switch (arg(0)) {
        null => 'null',
        bool _ => 'boolean',
        String _ => 'string',
        JsonNumber _ => 'number',
        List _ => 'array',
        Map _ => 'object',
        _ => _fail('Unknown JMESPath value'),
      };
  }
  _fail('Unknown compiled JMESPath function');
}

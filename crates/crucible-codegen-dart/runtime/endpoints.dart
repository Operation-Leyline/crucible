import 'dart:convert';
import 'json.dart';
import 'query.dart';

/// Endpoint rules rejected the call, or produced no usable endpoint.
final class EndpointException implements Exception {
  /// Why no endpoint was produced.
  final String message;
  const EndpointException(this.message);
  @override
  String toString() => 'EndpointException: $message';
}

final class ResolvedEndpoint {
  final String url;
  final Map<String, List<String>> headers;
  final Map<String, Object?> properties;
  const ResolvedEndpoint(this.url, {this.headers = const {}, this.properties = const {}});
}

Never _fail(String message) => throw EndpointException(message);
Map<String, Object?> _object(Object? value) =>
    value is Map<String, Object?> ? value : _fail('Malformed endpoint rules');
List<Object?> _array(Object? value) => value is List<Object?> ? value : _fail('Malformed endpoint rules');
String _text(Object? value) => value is String ? value : _fail('Endpoint function requires a string');
bool _boolean(Object? value) => value is bool ? value : _fail('Endpoint function requires a boolean');
int _integer(Object? value) => value is JsonNumber
    ? value.integer(min: BigInt.from(-2147483648), max: BigInt.from(2147483647)).toInt()
    : value is int
        ? value
        : _fail('Endpoint function requires an integer');

final _label = RegExp(r'^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$');

bool isValidHostLabel(String value, {bool allowSubdomains = false}) =>
    (allowSubdomains ? value.split('.') : [value]).every(_label.hasMatch);

bool _ipAddress(String host) {
  final parts = host.split('.');
  if (parts.length == 4 && parts.every((part) => RegExp(r'^(?:0|[1-9][0-9]{0,2})$').hasMatch(part) && int.parse(part) <= 255)) {
    return true;
  }
  if (!host.contains(':')) return false;
  try {
    Uri.parseIPv6Address(host);
    return true;
  } on FormatException {
    return false;
  }
}

/// The `parseUrl` rules function: an HTTP(S) URL without credentials, query or fragment.
Map<String, Object?>? parseEndpointUrl(String value) {
  if (value.contains('?') || value.contains('#') || value.codeUnits.any((unit) => unit <= 32 || unit == 127)) return null;
  final Uri url;
  try {
    url = Uri.parse(value);
  } on FormatException {
    return null;
  }
  if (!const ['http', 'https'].contains(url.scheme) || url.host.isEmpty || url.userInfo.isNotEmpty || url.port > 65535) return null;
  final host = url.host.toLowerCase();
  final ip = _ipAddress(host);
  if (!ip && !isValidHostLabel(host.endsWith('.') ? host.substring(0, host.length - 1) : host, allowSubdomains: true)) {
    return null;
  }
  // The scheme's default port is omitted from the authority.
  var authority = host.contains(':') ? '[$host]' : host;
  if (url.hasPort && url.port != (url.scheme == 'https' ? 443 : 80)) authority += ':${url.port}';
  final path = url.path.isEmpty ? '/' : url.path;
  return {
    'scheme': url.scheme,
    'authority': authority,
    'path': path,
    'normalizedPath': path.endsWith('/') ? path : '$path/',
    'isIp': ip,
  };
}

Object? endpointAttribute(Object? value, Object? path) {
  final steps = <Map<String, Object?>>[];
  if (path is String) {
    for (final part in path.split('.')) {
      final open = part.indexOf('[');
      final key = open < 0 ? part : part.substring(0, open);
      if (key.isNotEmpty) steps.add({'kind': 'member', 'name': key});
      if (open >= 0) {
        if (!part.endsWith(']')) _fail('Invalid endpoint attribute path');
        final index = int.tryParse(part.substring(open + 1, part.length - 1));
        if (index == null) _fail('Invalid endpoint attribute index');
        steps.add({'kind': 'index', 'index': index});
      }
    }
  } else {
    steps.addAll(_array(path).map(_object));
  }
  for (final step in steps) {
    if (step['kind'] == 'member') {
      value = value is Map<String, Object?> ? value[_text(step['name'])] : null;
    } else {
      if (value is! List<Object?>) return null;
      var index = _integer(step['index']);
      if (index < 0) index += value.length;
      value = index < 0 || index >= value.length ? null : value[index];
    }
    if (value == null) return null;
  }
  return value;
}

String uriEncode(String text) {
  final buffer = StringBuffer();
  for (final byte in utf8.encode(text)) {
    if (byte >= 65 && byte <= 90 || byte >= 97 && byte <= 122 || byte >= 48 && byte <= 57 || const [45, 46, 95, 126].contains(byte)) {
      buffer.writeCharCode(byte);
    } else {
      buffer.write('%${byte.toRadixString(16).toUpperCase().padLeft(2, '0')}');
    }
  }
  return buffer.toString();
}

Object? endpointFunction(String name, List<Object?> args) {
  switch (name) {
    case 'isSet':
      return args[0] != null;
    case 'not':
      return !_boolean(args[0]);
    case 'booleanEquals':
      return args[0] is bool && args[1] is bool && args[0] == args[1];
    case 'stringEquals':
      return args[0] is String && args[1] is String && args[0] == args[1];
    case 'getAttr':
      return endpointAttribute(args[0], args[1]);
    case 'substring':
      final text = _text(args[0]);
      final start = _integer(args[1]), end = _integer(args[2]);
      if (text.codeUnits.any((unit) => unit > 127) || start < 0 || end < start || end > text.length) return null;
      return _boolean(args[3]) ? text.substring(text.length - end, text.length - start) : text.substring(start, end);
    case 'isValidHostLabel':
      return isValidHostLabel(_text(args[0]), allowSubdomains: _boolean(args[1]));
    case 'parseUrl':
      return parseEndpointUrl(_text(args[0]));
    case 'uriEncode':
      return uriEncode(_text(args[0]));
  }
  _fail('Unsupported endpoint function $name');
}

Object? _expression(Map<String, Object?> node, Map<String, Object?> values) {
  Object? eval(Object? item) => _expression(_object(item), values);
  switch (node['kind']) {
    case 'reference':
      return values[_text(node['name'])];
    case 'boolean':
    case 'integer':
      return node['value'];
    case 'array':
      return [for (final item in _array(node['values'])) eval(item)];
    case 'object':
      return {for (final entry in _object(node['values']).entries) entry.key: eval(entry.value)};
    case 'call':
      return endpointFunction(_text(node['function']), [for (final item in _array(node['arguments'])) eval(item)]);
    case 'string':
      final text = StringBuffer();
      for (final item in _array(node['parts'])) {
        final part = _object(item);
        final value = part['kind'] == 'literal' ? part['value'] : endpointAttribute(values[_text(part['name'])], part['path']);
        if (value is String || value is bool || value is JsonNumber || value is int) {
          text.write(value);
        } else {
          _fail('Endpoint template references a missing or non-scalar value');
        }
      }
      return text.toString();
  }
  _fail('Unsupported endpoint expression ${node['kind']}');
}

ResolvedEndpoint? _rules(List<Object?> nodes, Map<String, Object?> parameters) {
  for (final entry in nodes) {
    final rule = _object(entry);
    final scope = Map<String, Object?>.of(parameters);
    var matched = true;
    for (final item in _array(rule['conditions'])) {
      final condition = _object(item);
      final value = endpointFunction(_text(condition['function']),
          [for (final argument in _array(condition['arguments'])) _expression(_object(argument), scope)]);
      if (value == null || value == false) {
        matched = false;
        break;
      }
      if (condition['assign'] case final String name) scope[name] = value;
    }
    if (!matched) continue;
    final result = _object(rule['result']);
    switch (result['kind']) {
      case 'tree':
        if (_rules(_array(result['rules']), scope) case final nested?) return nested;
      case 'error':
        _fail(_text(_expression(_object(result['message']), scope)));
      case 'endpoint':
        final url = _text(_expression(_object(result['url']), scope));
        if (parseEndpointUrl(url) == null) _fail('Endpoint rule returned an invalid HTTP(S) URL');
        return ResolvedEndpoint(url, headers: {
          for (final header in _object(result['headers']).entries)
            header.key: [for (final item in _array(header.value)) _text(_expression(_object(item), scope))]
        }, properties: {
          for (final property in _object(result['properties']).entries) property.key: _expression(_object(property.value), scope)
        });
    }
  }
  return null;
}

/// Bind parameters by their sources in descending precedence, then evaluate the rules.
ResolvedEndpoint resolveEndpoint(Map<String, Object?> ruleSet, Map<String, Object?> sources, Object? input,
    Map<String, Object?> client, Map<String, Object?> builtIns) {
  final parameters = <String, Object?>{};
  for (final entry in _object(ruleSet['parameters']).entries) {
    final parameter = _object(entry.value);
    Object? value;
    for (final item in sources[entry.key] as List? ?? const []) {
      final source = _object(item);
      value = switch (source['kind']) {
        'static' || 'default' => source['value'],
        'inputMember' => input is Map<String, Object?> ? input[_text(source['member'])] : null,
        'inputPath' => queryEvaluate(_object(_object(source['path'])['expression']), input),
        'client' => client[_text(source['name'])],
        'builtIn' => builtIns[_text(source['name'])],
        _ => _fail('Unsupported endpoint parameter source'),
      };
      if (value != null) break;
    }
    if (value == null && parameter['required'] == true) _fail('Required endpoint parameter ${entry.key} is missing');
    if (value != null) {
      final valid = switch (parameter['kind']) {
        'string' => value is String,
        'boolean' => value is bool,
        'stringArray' => value is List && value.every((item) => item is String),
        _ => false,
      };
      if (!valid) _fail('Endpoint parameter ${entry.key} has the wrong type');
    }
    parameters[entry.key] = value;
  }
  return _rules(_array(ruleSet['rules']), parameters) ?? _fail('No endpoint rule matched');
}

/// Expands an operation's host prefix; labels must be single DNS labels.
String prefixEndpoint(String endpoint, Map<String, Object?> description, Map<String, Object?> input) {
  final prefix = StringBuffer();
  for (final item in _array(description['parts'])) {
    final part = _object(item);
    if (part['kind'] == 'literal') {
      prefix.write(_text(part['value']));
    } else {
      final value = input[_text(part['member'])];
      if (value is! String || !isValidHostLabel(value)) _fail('Invalid endpoint host label ${part['member']}');
      prefix.write(value);
    }
  }
  final url = Uri.parse(endpoint);
  final host = '$prefix${url.host}';
  if (host.length > 253 || !isValidHostLabel(host, allowSubdomains: true)) _fail('Invalid endpoint hostname after prefix expansion');
  return url.replace(host: host).toString();
}

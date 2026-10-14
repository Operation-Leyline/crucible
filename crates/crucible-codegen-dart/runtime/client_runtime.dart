import 'dart:async';
import 'dart:convert';
import 'dart:math';
import 'dart:typed_data';
import 'package:crypto/crypto.dart';
import 'auth.dart';
import 'cancellation.dart';
import 'checksums.dart';
import 'codec.dart';
import 'config.dart';
import 'endpoints.dart';
import 'errors.dart';
import 'json.dart';
import 'timestamp.dart';
import 'transport.dart';
import 'waiters.dart';

typedef ErrorDecoder = ServiceError Function(
    String target, Object? value, WireContext context, ErrorMetadata metadata);

final class _Prepared {
  final HttpRequest request;
  final ResolvedEndpoint? endpoint;
  final ChecksumSettings checksum;
  const _Prepared(this.request, this.endpoint, this.checksum);
}

final class ClientRuntime {
  final ClientConfig config;
  final WireCodec codec;
  final ErrorDecoder errors;
  final Object Function() unit;
  final bool digest;
  late final String endpoint;
  late final HttpTransport _transport;
  late final bool _owned;
  bool _closed = false;
  ClientRuntime(this.config, this.codec, this.errors, this.unit,
      {required String endpoint, required this.digest}) {
    this.endpoint =
        (config.endpoint ?? endpoint).replaceFirst(RegExp(r'/+$'), '');
    // Modeled endpoint rules may resolve every URL without a configured endpoint.
    if (this.endpoint.isNotEmpty || codec.schema['endpointRules'] == null) {
      final url = Uri.parse(this.endpoint);
      _validateUrl(url);
      if (url.hasQuery || url.hasFragment)
        throw ArgumentError('Endpoint must not have a query or fragment');
    }
    _owned = config.transport == null;
    _transport = config.transport ?? DefaultHttpTransport();
  }
  void close() {
    if (!_closed && _owned) _transport.close();
    _closed = true;
  }

  String get _protocol => codec.schema['protocol'] as String? ?? 'crucibleJson';
  bool get _awsJson => const ['awsRestJson1', 'awsJson1_0', 'awsJson1_1'].contains(_protocol);

  Map<String, Object?> _operation(String name) =>
      wireObject(wireObject(codec.schema['operations'])[name]);

  Map<String, Object?> _input(Map<String, Object?> op, WireModel input) =>
      op['input'] == null
          ? <String, Object?>{}
          : wireObject(codec.encode(input, codec.context(wireString(op['input']))));

  Future<T> call<T>(String name, WireModel input,
      {CancellationToken? cancellation}) async {
    final op = _operation(name);
    // Snapshot mutable collections before the first await; retries replay it.
    return _invoke<T>(name, op, () => _input(op, input), cancellation, config.timeout);
  }

  Future<T> _invoke<T>(String name, Map<String, Object?> op, Map<String, Object?> Function() values,
      CancellationToken? cancellation, Duration timeout) async {
    final scope = CallScope(timeout, cancellation);
    try {
      scope.check();
      final prepared = _prepare(name, op, values());
      return await scope.run(() async {
        final result = await _send(name, op, prepared, scope);
        scope.check();
        return (op['output'] == null
                ? unit()
                : codec.decode(result, codec.context(wireString(op['output']))))
            as T;
      });
    } finally {
      scope.close();
    }
  }

  Stream<T> pages<T>(String name, WireModel input,
      {CancellationToken? cancellation}) {
    final op = _operation(name);
    final pagination = wireObject(op['pagination']);
    final initial = _input(op, input);
    late final StreamController<T> controller;
    CallScope? active;
    Completer<void>? resumed;
    var cancelled = false;
    void resume() {
      resumed?.complete();
      resumed = null;
    }

    Future<void> iterate() async {
      final values = Map<String, Object?>.from(initial);
      final seen = <String>{};
      final inputToken = wireString(pagination['input']);
      if (values[inputToken] case final String token when token.isNotEmpty)
        seen.add(token);
      try {
        while (!cancelled) {
          if (controller.isPaused) await (resumed ??= Completer<void>()).future;
          if (cancelled) return;
          cancellation?.throwIfCancelled();
          final scope = CallScope(config.timeout, cancellation);
          active = scope;
          late final Map<String, Object?> result;
          late final T page;
          try {
            (result, page) = await scope.run(() async {
              final result = await _send(name, op, _prepare(name, op, values), scope);
              final page = codec.decode(
                  result, codec.context(wireString(op['output']))) as T;
              scope.check();
              return (result, page);
            });
          } finally {
            active = null;
            scope.close();
          }
          if (cancelled) return;
          controller.add(page);
          Object? token = result;
          for (final part in wireArray(pagination['output'])) {
            token =
                token is Map<String, Object?> ? token[wireString(part)] : null;
          }
          if (token == null || token == '') return;
          final next = wireString(token);
          if (!seen.add(next)) throw StateError('Pagination token cycle');
          values[inputToken] = next;
        }
      } catch (error, stack) {
        if (!cancelled) controller.addError(error, stack);
      } finally {
        unawaited(controller.close());
      }
    }

    controller = StreamController<T>(
        sync: true,
        // Start after listen returns so a synchronous listener can pause or cancel.
        onListen: () {
          unawaited(Future<void>(iterate));
        },
        onResume: resume,
        onCancel: () {
          cancelled = true;
          active?.token.cancel();
          resume();
        });
    return controller.stream;
  }

  /// Polls until a modeled acceptor reaches a terminal state. One deadline
  /// covers every attempt and delay; each attempt also keeps its own timeout.
  Future<T?> wait<T>(String name, String waiter, WireModel input,
      {required Duration timeout, Duration? minDelay, Duration? maxDelay, CancellationToken? cancellation}) async {
    final op = _operation(name);
    final description = wireObject(wireObject(op['waiters'])[waiter]);
    final minimum = minDelay ?? waiterSeconds(description['minDelay']);
    final maximum = maxDelay ?? waiterSeconds(description['maxDelay']);
    if (timeout <= Duration.zero || minimum.isNegative || maximum < minimum) {
      throw ArgumentError('Waiter timeout must be positive and delays ordered and nonnegative');
    }
    final values = _input(op, input);
    final queryInput = op['input'] == null ? null : queryValue(wireString(op['input']), values);
    final scope = CallScope(timeout, cancellation);
    var attempts = 0;
    T? output;
    Object? error;
    WaiterTimeoutException expired() => WaiterTimeoutException(waiter, attempts, response: output, error: error);
    try {
      while (true) {
        if (scope.remaining <= Duration.zero) throw expired();
        attempts++;
        try {
          final remaining = scope.remaining;
          output = await _invoke<T>(name, op, () => Map.of(values), scope.token,
              remaining < config.timeout ? remaining : config.timeout);
          error = null;
        } catch (caught) {
          if (scope.token.isCancelled) {
            if (scope.token.reason is TimeoutException) throw expired();
            rethrow;
          }
          output = null;
          error = caught;
        }
        if (scope.remaining <= Duration.zero) throw expired();
        final String state;
        final Object? message;
        try {
          final queryOutput = output == null || op['output'] == null
              ? null
              : queryValue(wireString(op['output']), codec.encode(output, codec.context(wireString(op['output']))));
          (state, message) = waiterState(description, queryInput, queryOutput, error);
        } on FormatException catch (caught) {
          throw WaiterException(waiter, attempts, response: output, error: caught);
        }
        if (state == 'success') return output;
        if (state == 'failure') throw WaiterException(waiter, attempts, response: output, error: error, message: message);
        try {
          await scope.sleep(waiterDelay(attempts, scope.remaining, minimum, maximum));
        } catch (_) {
          if (scope.token.reason is TimeoutException) throw expired();
          rethrow;
        }
      }
    } finally {
      scope.close();
    }
  }

  /// Wire values keyed by member name, with timestamps as exact epoch seconds,
  /// as waiter and endpoint queries observe them.
  Object? queryValue(String target, Object? value, [String? format]) {
    if (value == null) return null;
    final shape = codec.shape(target);
    switch (shape['kind']) {
      case 'structure':
      case 'union':
        final members = wireObject(shape['members']);
        return {
          for (final entry in wireObject(value).entries)
            if (members[entry.key] case final Map<String, Object?> member)
              entry.key: queryValue(wireString(member['target']), entry.value, member['format'] as String?)
            else
              entry.key: entry.value
        };
      case 'list':
        final member = wireObject(shape['member']);
        return [for (final item in wireArray(value)) queryValue(wireString(member['target']), item, member['format'] as String?)];
      case 'map':
        final member = wireObject(shape['value']);
        return wireObject(value).map((key, item) => MapEntry(key, queryValue(wireString(member['target']), item, member['format'] as String?)));
      case 'timestamp':
        return _parseTime(value, format ?? shape['format'] as String? ?? 'date-time').toEpoch();
      default:
        return value;
    }
  }

  static Timestamp _parseTime(Object? value, String format) => switch (format) {
        'epoch-seconds' => Timestamp.fromEpoch(value is JsonNumber ? value : JsonNumber(wireString(value))),
        'http-date' => Timestamp.parseHttp(wireString(value)),
        _ => Timestamp.parse(wireString(value)),
      };
  static Object _formatTime(Timestamp value, String format) => switch (format) {
        'epoch-seconds' => value.toEpoch(),
        'http-date' => value.toHttpDate(),
        _ => value.toIso8601String(),
      };

  /// Re-encodes a timestamp member for an HTTP binding with its own format.
  Object? _rebind(String target, Map<String, Object?> bindings, String member, Object? value, {required bool encode}) {
    final format = (bindings['timestampFormats'] as Map?)?[member] as String?;
    if (format == null || value == null) return value;
    final info = wireObject(wireObject(codec.shape(target)['members'])[member]);
    final shape = codec.shape(wireString(info['target']));
    final item = shape['kind'] == 'list' ? wireObject(shape['member']) : info;
    final document = item['format'] as String? ?? 'date-time';
    Object convert(Object? value) =>
        encode ? _formatTime(_parseTime(value, document), format) : _formatTime(_parseTime(value, format), document);
    if (shape['kind'] != 'list') return convert(value);
    if (encode) return [for (final entry in wireArray(value)) entry == null ? null : convert(entry)];
    final text = wireString(value);
    final parts = format == 'http-date'
        ? RegExp(r'[A-Za-z]{3}, \d{2} [A-Za-z]{3} \d{4} \d{2}:\d{2}:\d{2} GMT').allMatches(text).map((match) => match[0]!)
        : text.split(',').map((part) => part.trim()).where((part) => part.isNotEmpty);
    return [for (final part in parts) convert(format == 'epoch-seconds' ? JsonNumber(part) : part)];
  }

  _Prepared _prepare(String name, Map<String, Object?> op, Map<String, Object?> input) {
    final values = Map<String, Object?>.from(input);
    final inputTarget = op['input'] as String?;
    final token = op['token'];
    if (token is String && values[token] == null) values[token] = _uuid();
    final checksum = prepareChecksum(op['checksum'] as Map<String, Object?>?, values,
        config.requestChecksumCalculation, config.responseChecksumValidation);
    ResolvedEndpoint? resolved;
    var base = endpoint;
    if (codec.schema['endpointRules'] case final Map<String, Object?> rules) {
      final builtIns = <String, Object?>{
        'AWS::Region': config.region,
        'SDK::Endpoint': endpoint.isEmpty ? null : endpoint,
        ...config.endpointBuiltIns,
      };
      resolved = resolveEndpoint(rules, op['endpointParameters'] as Map<String, Object?>? ?? const {},
          inputTarget == null ? <String, Object?>{} : queryValue(inputTarget, values), config.endpointParameters, builtIns);
      base = resolved.url;
    }
    if (op['endpoint'] case final Map<String, Object?> prefix when !config.disableHostPrefix) {
      base = prefixEndpoint(base, prefix, values);
    }
    base = base.replaceFirst(RegExp(r'/+$'), '');
    final bindings = wireObject(op['request']);
    Object? bound(String member) => inputTarget == null ? values[member] : _rebind(inputTarget, bindings, member, values[member], encode: true);
    var path = wireString(op['uri']);
    for (final label in wireArray(bindings['labels'])) {
      final name = wireString(label);
      final value = bound(name);
      if (value == null) throw FormatException('Missing label: $name');
      final text = codec.bound(value);
      if (text.isEmpty) throw FormatException('Empty label: $name');
      final greedy = (op['greedyLabels'] as List? ?? const []).contains(name);
      final parts = greedy ? text.split('/') : [text];
      if (parts.any((part) => part == '.' || part == '..')) throw FormatException('Dot-segment HTTP label: $name');
      path = path.replaceAll(greedy ? '{$name+}' : '{$name}', parts.map(_escape).join('/'));
    }
    final query = <String>[];
    for (final binding in wireArray(bindings['query'])) {
      final pair = wireArray(binding);
      final value = bound(wireString(pair[0]));
      if (value == null) continue;
      for (final item in value is List ? value : [value]) {
        query.add(
            '${_escape(wireString(pair[1]))}=${_escape(codec.bound(item))}');
      }
    }
    if (bindings['queryParams'] case final String member) {
      final reserved = wireArray(bindings['query']).map((pair) => wireString(wireArray(pair)[1])).toSet();
      if (path.contains('?')) {
        for (final pair in path.split('?').last.split('&')) reserved.add(Uri.decodeComponent(pair.split('=').first));
      }
      if (values[member] != null) {
        final map = wireObject(values[member]);
        for (final key in map.keys.toList()..sort()) {
          if (reserved.contains(key)) continue;
          final value = map[key];
          for (final item in value is List ? value : [value]) {
            if (item != null) query.add('${_escape(key)}=${_escape(codec.bound(item))}');
          }
        }
      }
    }
    if (query.isNotEmpty)
      path += '${path.contains('?') ? '&' : '?'}${query.join('&')}';
    final headers = _headers(config.headers);
    for (final entry in resolved?.headers.entries ?? const <MapEntry<String, List<String>>>[]) {
      headers[entry.key.toLowerCase()] = entry.value.join(', ');
    }
    // A Content-Type bound to an input member outranks the payload's media
    // type, which outranks endpoint and client header defaults.
    var modelledContentType = false;
    for (final binding in wireArray(bindings['headers'])) {
      final pair = wireArray(binding);
      final value = bound(wireString(pair[0]));
      if (value == null) continue;
      final wire = wireString(pair[1]).toLowerCase();
      // HTTP-date list members are joined without quoting their commas.
      headers[wire] = value is List && (bindings['timestampFormats'] as Map?)?[pair[0]] == 'http-date'
          ? value.join(', ')
          : codec.bound(_media(value, bindings, wireString(pair[0]), true));
      if (wire == 'content-type') modelledContentType = true;
    }
    Uint8List? body;
    final explicitHeaders = wireArray(bindings['headers']).map((pair) => wireString(wireArray(pair)[1]).toLowerCase()).toSet();
    for (final binding in bindings['prefixHeaders'] as List? ?? const []) {
      final pair = wireArray(binding);
      final value = values[wireString(pair[0])];
      if (value == null) continue;
      final map = wireObject(value);
      for (final key in map.keys.toList()..sort()) {
        final name = '${wireString(pair[1])}$key'.toLowerCase();
        if (!explicitHeaders.contains(name)) headers[name] = codec.bound(_media(map[key], bindings, wireString(pair[0]), true));
      }
    }
    final members = wireArray(bindings['body']);
    if (members.isNotEmpty) {
      final object = <String, Object?>{};
      for (final member in members) {
        final name = wireString(member);
        if (values.containsKey(name)) object[name] = values[name];
      }
      body = Uint8List.fromList(utf8.encode(exactJsonEncode(codec.document(wireString(op['input']), object, encode: true))));
      headers['content-type'] = 'application/json';
    }
    if (bindings['payload'] case final List payload) {
      final value = values[wireString(payload[0])];
      if (value != null) {
        final kind = bindings['payloadKind'] ?? 'blob';
        final bytes = kind == 'string' ? Uint8List.fromList(utf8.encode(wireString(value)))
            : kind == 'document' ? Uint8List.fromList(utf8.encode(exactJsonEncode(codec.document(_payloadTarget(wireString(op['input']), payload), value, encode: true))))
            : base64Decode(wireString(value));
        // An empty raw payload sends no body or content type.
        if (bytes.isNotEmpty || kind == 'document') {
          body = bytes;
          if (!modelledContentType) headers['content-type'] = wireString(payload[1]);
        }
      }
    }
    if (_protocol == 'awsJson1_0' || _protocol == 'awsJson1_1') {
      body ??= Uint8List.fromList(utf8.encode('{}'));
      headers['content-type'] = 'application/x-amz-json-${_protocol == 'awsJson1_0' ? '1.0' : '1.1'}';
      headers['x-amz-target'] = '${wireString(codec.schema['service']).split('#').last}.$name';
    }
    // The release this client was generated for (docs/api-versions-clients.md).
    final apiVersion = codec.schema['apiVersion'];
    if (apiVersion is String) headers['crucible-version'] = apiVersion;
    return _Prepared(
        HttpRequest(url: Uri.parse(base + path), method: wireString(op['method']), headers: headers, body: body),
        resolved,
        checksum);
  }

  Future<Map<String, Object?>> _send(String name,
      Map<String, Object?> op, _Prepared prepared, CallScope scope) async {
    if (_closed) throw StateError('Client is closed');
    final safe = op['safe'] == true;
    for (var attempt = 1; attempt <= config.maxAttempts; attempt++) {
      scope.check();
      final auth = await authenticate(prepared.request.copyWith(), op, codec.schema, config, prepared.endpoint, scope);
      var request = auth.request;
      for (final middleware in config.middleware) {
        scope.check();
        request = await middleware(request);
      }
      scope.check();
      var headers = _headers(request.headers);
      _validateUrl(request.url);
      headers = checksumRequest(prepared.checksum, headers, request.body);
      if (digest && (request.body != null || const ['POST', 'PUT', 'PATCH'].contains(request.method)))
        headers['x-amz-content-sha256'] =
            sha256.convert(request.body ?? const <int>[]).toString();
      request = request.copyWith(headers: headers);
      if (auth.sign case final sign?) request = sign(request);
      if ((auth.credentialed || request.headers.containsKey('authorization')) && !_secure(request.url))
        throw ArgumentError('Credentials require HTTPS or loopback HTTP');
      late final HttpResponse response;
      try {
        final bindings = wireObject(op['response']);
        response = await _transport.send(request,
            readBody: wireArray(bindings['body']).isNotEmpty ||
                bindings['payload'] != null ||
                prepared.checksum.response.isNotEmpty,
            cancellation: scope.token);
      } catch (error) {
        scope.check();
        if (error is CancelledException || error is TimeoutException) rethrow;
        if (!safe || attempt == config.maxAttempts)
          throw NetworkException(error);
        await scope.sleep(retryDelay(attempt, null));
        continue;
      }
      scope.check();
      if (response.status >= 200 && response.status < 300) {
        final algorithm = checksumResponse(prepared.checksum, response.headers, response.body);
        if (algorithm != null) config.checksumValidated?.call(name, algorithm);
        return _success(op, response);
      }
      final error = _failure(op, response);
      final retry = error.metadata.throttling ||
          safe &&
              (error.metadata.retryable ||
                  [500, 502, 503, 504].contains(response.status));
      if (!retry || attempt == config.maxAttempts) throw error;
      await scope.sleep(retryDelay(attempt, response.headers['retry-after']));
    }
    throw StateError('No request attempt');
  }

  Map<String, Object?> _success(
      Map<String, Object?> op, HttpResponse response) {
    final bindings = wireObject(op['response']);
    final Map<String, Object?> body;
    if (bindings['payload'] case final List payload) {
      body = {wireString(payload[0]): _payloadValue(wireString(op['output']), bindings, response.body)};
    } else if (wireArray(bindings['body']).isEmpty || response.body.isEmpty) {
      body = {};
    } else {
      body = wireObject(codec.document(wireString(op['output']), exactJsonDecode(utf8.decode(response.body)), encode: false));
    }
    if (op['output'] case final String target)
      _applyHeaders(target, bindings, response, body);
    return body;
  }

  void _applyHeaders(String target, Map<String, Object?> bindings, HttpResponse response,
      Map<String, Object?> body) {
    final members = wireObject(codec.shape(target)['members']);
    final formats = bindings['timestampFormats'] as Map? ?? const {};
    for (final binding in wireArray(bindings['headers'])) {
      final pair = wireArray(binding);
      final name = wireString(pair[0]);
      final text = response.headers[wireString(pair[1]).toLowerCase()];
      if (text == null) continue;
      body[name] = formats.containsKey(name)
          ? _rebind(target, bindings, name, text, encode: false)
          : _media(codec.header(members[name], text), bindings, name, false);
    }
    final explicit = wireArray(bindings['headers']).map((pair) => wireString(wireArray(pair)[1]).toLowerCase()).toSet();
    for (final binding in bindings['prefixHeaders'] as List? ?? const []) {
      final pair = wireArray(binding);
      final prefix = wireString(pair[1]).toLowerCase();
      final map = <String, Object?>{};
      for (final entry in response.headers.entries) {
        final name = entry.key.toLowerCase();
        if (name.startsWith(prefix) && !explicit.contains(name)) map[name.substring(prefix.length)] = _media(entry.value, bindings, wireString(pair[0]), false);
      }
      body[wireString(pair[0])] = map;
    }
    if (bindings['responseCode'] case final String member) body[member] = JsonNumber(response.status.toString());
  }

  ServiceError _failure(Map<String, Object?> op, HttpResponse response) {
    Map<String, Object?> body = {};
    try {
      body = wireObject(exactJsonDecode(utf8.decode(response.body)));
    } on FormatException {/* HTTP metadata remains authoritative. */}
    String? text(Object? value) => value is String ? value : null;
    var name = response.headers['x-crucible-error'] ?? text(body['__type']);
    String? requestId = response.headers['x-request-id'] ?? text(body['requestId']);
    if (_awsJson) {
      // AWS JSON error codes may carry a namespace and a trailing URI.
      name = (response.headers['x-amzn-errortype'] ?? text(body['code']) ?? text(body['__type']))?.split(':').first.split('#').last;
      requestId = response.headers['x-amzn-requestid'] ?? response.headers['x-amz-request-id'] ?? requestId;
    }
    final matches = wireArray(op['errors']).cast<String>().where((id) {
      final shape = codec.shape(id);
      if (name != null) return ((shape['error'] as Map)['wireCode'] ?? shape['name']) == name;
      return (wireObject(shape['error'])['status'] as JsonNumber).scaled(0) ==
          BigInt.from(response.status);
    }).toList();
    final target = matches.length == 1 ? matches.single : null;
    final shape = target == null ? null : codec.shape(target);
    final traits = shape == null ? null : wireObject(shape['error']);
    final message = text(body['message']) ?? (_awsJson ? text(body['Message']) : null);
    final metadata = ErrorMetadata(
        name: name ?? shape?['name'] as String? ?? 'UnknownError',
        message: message ?? 'HTTP ${response.status}',
        status: response.status,
        requestId: requestId,
        headers: response.headers,
        retryable: traits?['retryable'] == true,
        throttling: traits?['throttling'] == true);
    if (target != null) {
      try {
        if (traits!['payload'] case final List payload) { body = {wireString(payload[0]): _payloadValue(target, traits, response.body)}; }
        else { body = wireObject(codec.document(target, body, encode: false)); }
        _applyHeaders(target, traits, response, body);
        return errors(target, body, codec.context(target), metadata);
      } on FormatException {
        /* Retain metadata if modeled members cannot decode. */
      } on TypeError {/* Generated casts also reject malformed members. */}
    }
    return UnknownServiceError(metadata);
  }

  String _payloadTarget(String target, List payload) => wireString(wireObject(wireObject(codec.shape(target)['members'])[wireString(payload[0])])['target']);
  Object? _payloadValue(String target, Map<String, Object?> bindings, Uint8List body) {
    switch (bindings['payloadKind'] ?? 'blob') {
      case 'string': return utf8.decode(body);
      case 'document': return body.isEmpty ? null : codec.document(_payloadTarget(target, wireArray(bindings['payload'])), exactJsonDecode(utf8.decode(body)), encode: false);
      default: return base64Encode(body);
    }
  }
  static Object? _media(Object? value, Map<String, Object?> bindings, String member, bool encode) {
    if ((bindings['headerEncodings'] as Map?)?[member] != 'base64') return value;
    if (value is List) return value.map((item) => _media(item, bindings, member, encode)).toList();
    final text = wireString(value);
    if (encode) return base64Encode(utf8.encode(text));
    final bytes = base64Decode(text);
    if (base64Encode(bytes) != text) throw const FormatException('Invalid media-type header base64');
    return utf8.decode(bytes);
  }

  Duration retryDelay(int attempt, String? header) {
    if (header != null) {
      final seconds = double.tryParse(header);
      if (seconds != null &&
          seconds.isFinite &&
          seconds >= 0 &&
          seconds <= 8640000000)
        return Duration(microseconds: (seconds * 1000000).round());
      try {
        final date = Timestamp.parseHttp(header).toDateTime();
        final delay = date.difference(DateTime.now().toUtc());
        if (!delay.isNegative) return delay;
      } on FormatException {/* Use jitter for malformed dates. */}
    }
    final cap = min(config.maxDelay.inMicroseconds,
            config.baseDelay.inMicroseconds * pow(2, min(attempt - 1, 30)))
        .toDouble();
    return Duration(microseconds: (Random().nextDouble() * cap).floor());
  }

  static void _validateUrl(Uri url) {
    if (!['http', 'https'].contains(url.scheme) ||
        url.host.isEmpty ||
        url.userInfo.isNotEmpty)
      throw ArgumentError('Absolute HTTP(S) URL without credentials required');
  }

  static bool _secure(Uri url) =>
      url.scheme == 'https' ||
      url.scheme == 'http' &&
          ['localhost', '127.0.0.1', '::1'].contains(url.host.toLowerCase());
  static Map<String, String> _headers(Map<String, String> values) {
    final keys = values.keys.toList()..sort();
    return {for (final key in keys) key.toLowerCase(): values[key]!};
  }

  static String _escape(String text) => utf8
      .encode(text)
      .map((byte) => byte >= 65 && byte <= 90 ||
              byte >= 97 && byte <= 122 ||
              byte >= 48 && byte <= 57 ||
              [45, 46, 95, 126].contains(byte)
          ? String.fromCharCode(byte)
          : '%${byte.toRadixString(16).toUpperCase().padLeft(2, '0')}')
      .join();
  static String _uuid() {
    final random = Random.secure();
    final bytes = List.generate(16, (_) => random.nextInt(256));
    bytes[6] = bytes[6] & 15 | 64;
    bytes[8] = bytes[8] & 63 | 128;
    final hex =
        bytes.map((byte) => byte.toRadixString(16).padLeft(2, '0')).join();
    return '${hex.substring(0, 8)}-${hex.substring(8, 12)}-${hex.substring(12, 16)}-${hex.substring(16, 20)}-${hex.substring(20)}';
  }
}

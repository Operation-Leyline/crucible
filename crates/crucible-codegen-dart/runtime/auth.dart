import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:crypto/crypto.dart';
import 'auth_platform_stub.dart' if (dart.library.io) 'auth_platform_io.dart' as platform;
import 'cancellation.dart';
import 'config.dart';
import 'endpoints.dart';
import 'transport.dart';

/// HTTP Basic credentials. The password never appears in [toString].
final class BasicCredentials {
  final String username;
  final String password;
  BasicCredentials(this.username, this.password) {
    if (username.contains(':')) throw ArgumentError('Basic username cannot contain a colon');
  }
  @override
  String toString() => 'BasicCredentials([redacted])';
}

/// AWS credentials for Signature Version 4. Secrets never appear in [toString].
final class AwsCredentials {
  final String accessKeyId;
  final String secretAccessKey;
  final String? sessionToken;
  final DateTime? expiresAt;
  AwsCredentials(this.accessKeyId, this.secretAccessKey, {this.sessionToken, this.expiresAt}) {
    if (accessKeyId.isEmpty || secretAccessKey.isEmpty) throw ArgumentError('AWS credentials must be nonempty');
  }
  @override
  String toString() => 'AwsCredentials([redacted])';
}

typedef BasicCredentialProvider = FutureOr<BasicCredentials?> Function();
typedef ApiKeyProvider = FutureOr<String?> Function();
typedef AwsCredentialProvider = FutureOr<AwsCredentials?> Function();

/// Tries providers in order. `null` means unavailable; errors remain errors.
AwsCredentialProvider credentialChain(List<AwsCredentialProvider> providers) => () async {
      for (final provider in providers) {
        final identity = await provider();
        if (identity != null) return identity;
      }
      return null;
    };

/// AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY and AWS_SESSION_TOKEN. Native platforms only.
AwsCredentials? environmentCredentials() {
  final values = platform.environment();
  return _identity(values['AWS_ACCESS_KEY_ID'], values['AWS_SECRET_ACCESS_KEY'], values['AWS_SESSION_TOKEN']);
}

/// Static keys from a shared credentials file; never runs credential_process.
AwsCredentials? profileCredentials({String? profile, String? path}) {
  final environment = platform.environment();
  path ??= environment['AWS_SHARED_CREDENTIALS_FILE'] ??
      '${environment['HOME'] ?? environment['USERPROFILE']}/.aws/credentials';
  final contents = platform.readProfile(path);
  if (contents == null) return null;
  profile ??= environment['AWS_PROFILE'] ?? 'default';
  String? section;
  final values = <String, String>{};
  for (var line in const LineSplitter().convert(contents)) {
    line = line.trim();
    if (line.isEmpty || line.startsWith('#') || line.startsWith(';')) continue;
    if (line.startsWith('[') && line.endsWith(']')) {
      section = line.substring(1, line.length - 1).trim();
      continue;
    }
    final separator = line.indexOf('=');
    if (section == profile && separator >= 0) {
      values[line.substring(0, separator).trim()] = line.substring(separator + 1).trim();
    }
  }
  return _identity(values['aws_access_key_id'], values['aws_secret_access_key'], values['aws_session_token']);
}

AwsCredentials? _identity(String? access, String? secret, String? token) {
  if ((access == null || access.isEmpty) && (secret == null || secret.isEmpty)) return null;
  if (access == null || access.isEmpty || secret == null || secret.isEmpty) {
    throw StateError('AWS credentials are incomplete');
  }
  return AwsCredentials(access, secret, sessionToken: token == null || token.isEmpty ? null : token);
}

/// Shares one provider across concurrent calls and refreshes expiring
/// identities once, before they expire.
final class CachedCredentials {
  final AwsCredentialProvider provider;
  final Duration refreshBefore;
  final DateTime Function() clock;
  AwsCredentials? _value;
  DateTime? _refreshAt;
  Future<AwsCredentials?>? _pending;
  CachedCredentials(this.provider, {this.refreshBefore = const Duration(seconds: 60), DateTime Function()? clock})
      : clock = clock ?? DateTime.now {
    if (refreshBefore.isNegative) throw ArgumentError('Invalid credential refresh interval');
  }

  Future<AwsCredentials?> call() {
    final value = _value;
    if (value != null && (_refreshAt == null || _refreshAt!.isAfter(clock()))) return Future.value(value);
    return _pending ??= Future<AwsCredentials?>.sync(provider).then((value) {
      _refreshAt = null;
      if (value?.expiresAt case final DateTime expires) {
        final remaining = expires.difference(clock());
        if (remaining <= Duration.zero) throw StateError('Credential provider returned expired credentials');
        // Short-lived credentials stay reusable for half their lifetime.
        final skew = refreshBefore < remaining ~/ 2 ? refreshBefore : remaining ~/ 2;
        _refreshAt = expires.subtract(skew);
      }
      return _value = value;
    }).whenComplete(() => _pending = null);
  }

  @override
  String toString() => 'CachedCredentials([redacted])';
}

String _encode(String value) => utf8
    .encode(value)
    .map((byte) => byte >= 65 && byte <= 90 || byte >= 97 && byte <= 122 || byte >= 48 && byte <= 57 || const [45, 46, 95, 126].contains(byte)
        ? String.fromCharCode(byte)
        : '%${byte.toRadixString(16).toUpperCase().padLeft(2, '0')}')
    .join();

String _two(int value) => value.toString().padLeft(2, '0');

/// Signs the final buffered request with AWS Signature Version 4.
///
/// S3 keeps its path as sent; other services normalize it and encode it twice,
/// unless [disableNormalizePath] or [disableDoubleEncoding] says otherwise.
HttpRequest signV4(HttpRequest request, AwsCredentials identity,
    {required String service,
    required String region,
    DateTime? now,
    bool unsignedPayload = false,
    bool? disableDoubleEncoding,
    bool? disableNormalizePath}) {
  now = (now ?? DateTime.now()).toUtc();
  if (identity.expiresAt != null && !identity.expiresAt!.isAfter(now)) throw ArgumentError('AWS credentials have expired');
  if (service.isEmpty || region.isEmpty || RegExp(r'[/\r\n]').hasMatch(service + region)) {
    throw ArgumentError('SigV4 service and region must be nonempty scope components');
  }
  final headers = {for (final entry in request.headers.entries) entry.key.toLowerCase(): entry.value}
    ..remove('authorization')
    ..remove('x-amz-security-token');
  final date = '${now.year.toString().padLeft(4, '0')}${_two(now.month)}${_two(now.day)}';
  final stamp = '${date}T${_two(now.hour)}${_two(now.minute)}${_two(now.second)}Z';
  headers['x-amz-date'] = stamp;
  if (identity.sessionToken != null) headers['x-amz-security-token'] = identity.sessionToken!;
  final digest = unsignedPayload ? 'UNSIGNED-PAYLOAD' : sha256.convert(request.body ?? Uint8List(0)).toString();
  headers['x-amz-content-sha256'] = digest;
  headers['host'] = request.url.authority.toLowerCase();
  final raw = request.url.toString().substring('${request.url.scheme}://${request.url.authority}'.length);
  var path = raw.split('?').first.split('#').first;
  if (path.isEmpty) path = '/';
  if (!(disableNormalizePath ?? service == 's3')) {
    final parts = <String>[];
    for (final part in path.split('/')) {
      if (part == '..') {
        if (parts.isNotEmpty) parts.removeLast();
      } else if (part.isNotEmpty && part != '.') {
        parts.add(part);
      }
    }
    path = '/${parts.join('/')}${path.endsWith('/') && parts.isNotEmpty ? '/' : ''}';
  }
  if (!(disableDoubleEncoding ?? service == 's3')) path = path.split('/').map(_encode).join('/');
  final query = <(String, String)>[];
  for (final part in request.url.query.split('&')) {
    if (part.isEmpty) continue;
    final i = part.indexOf('=');
    query.add((_encode(Uri.decodeComponent(i < 0 ? part : part.substring(0, i))), _encode(Uri.decodeComponent(i < 0 ? '' : part.substring(i + 1)))));
  }
  query.sort((a, b) => a.$1 != b.$1 ? a.$1.compareTo(b.$1) : a.$2.compareTo(b.$2));
  const excluded = {'authorization', 'connection', 'expect', 'transfer-encoding', 'user-agent', 'x-amzn-trace-id', 'accept-encoding', 'content-length'};
  final names = headers.keys.where((key) => !excluded.contains(key)).toList()..sort();
  final signed = names.join(';');
  final lines = names.map((name) => '$name:${headers[name]!.trim().split(RegExp(r'\s+')).join(' ')}\n').join();
  final canonical = [request.method, path, query.map((pair) => '${pair.$1}=${pair.$2}').join('&'), lines, signed, digest].join('\n');
  final scope = '$date/$region/$service/aws4_request';
  final text = 'AWS4-HMAC-SHA256\n$stamp\n$scope\n${sha256.convert(utf8.encode(canonical))}';
  List<int> key = utf8.encode('AWS4${identity.secretAccessKey}');
  for (final part in [date, region, service, 'aws4_request']) {
    key = Hmac(sha256, key).convert(utf8.encode(part)).bytes;
  }
  headers['authorization'] =
      'AWS4-HMAC-SHA256 Credential=${identity.accessKeyId}/$scope, SignedHeaders=$signed, Signature=${Hmac(sha256, key).convert(utf8.encode(text))}';
  return request.copyWith(headers: headers);
}

/// The outcome of credential selection for one attempt.
final class Authentication {
  final HttpRequest request;
  final bool credentialed;
  final HttpRequest Function(HttpRequest request)? sign;
  const Authentication(this.request, {this.credentialed = false, this.sign});
}

bool _preset(HttpRequest request, Map<String, Object?> scheme) {
  String value;
  String prefix;
  switch (scheme['kind']) {
    case 'apiKey':
      final name = scheme['name'] as String;
      if (scheme['location'] == 'query') return (request.url.queryParameters[name] ?? '').isNotEmpty;
      value = request.headers[name.toLowerCase()] ?? '';
      if (scheme['scheme'] == null) return value.isNotEmpty;
      prefix = scheme['scheme'] as String;
    case 'bearer':
      value = request.headers['authorization'] ?? '';
      prefix = 'Bearer';
    case 'basic':
      value = request.headers['authorization'] ?? '';
      prefix = 'Basic';
    case 'sigV4':
      value = request.headers['authorization'] ?? '';
      prefix = 'AWS4-HMAC-SHA256';
    default:
      return false;
  }
  return value.length > prefix.length + 1 && value.substring(0, prefix.length + 1).toLowerCase() == '${prefix.toLowerCase()} ';
}

/// Selects the first modeled scheme with caller-supplied credentials or an
/// identity. Caller-supplied credentials bypass every provider. SigV4 signing
/// is deferred until middleware and checksums have produced the final bytes.
Future<Authentication> authenticate(HttpRequest request, Map<String, Object?> op, Map<String, Object?> schema,
    ClientConfig config, ResolvedEndpoint? endpoint, CallScope scope) async {
  final schemes = schema['authSchemes'] as Map? ?? const {};
  final ids = (op['auth'] as List?)?.cast<String>() ??
      (op['bearer'] == true ? const ['smithy.api#httpBearerAuth'] : const <String>[]);
  for (final id in ids) {
    final described = (schemes[id] as Map?)?.cast<String, Object?>() ??
        (id == 'smithy.api#httpBearerAuth' ? const {'kind': 'bearer'} : throw StateError('Unknown authentication scheme $id'));
    var scheme = Map<String, Object?>.of(described);
    if (endpoint?.properties['authSchemes'] case final List choices) {
      final match = choices
          .whereType<Map>()
          .where((item) => '${item['name']}'.toLowerCase() == '${scheme['kind']}'.toLowerCase())
          .firstOrNull;
      if (match == null) continue;
      scheme = {...scheme, for (final entry in match.entries) if (entry.key != 'name') entry.key as String: entry.value};
    }
    if (_preset(request, scheme)) return Authentication(request, credentialed: true);
    switch (scheme['kind']) {
      case 'bearer':
        final token = await config.token?.call();
        scope.check();
        if (token != null) {
          return Authentication(request.copyWith(headers: {...request.headers, 'authorization': 'Bearer $token'}), credentialed: true);
        }
      case 'basic':
        final basic = await config.basicAuth?.call();
        scope.check();
        if (basic != null) {
          final value = base64Encode(utf8.encode('${basic.username}:${basic.password}'));
          return Authentication(request.copyWith(headers: {...request.headers, 'authorization': 'Basic $value'}), credentialed: true);
        }
      case 'apiKey':
        final key = await config.apiKey?.call();
        scope.check();
        if (key != null) {
          if (key.isEmpty) throw ArgumentError('API key must be nonempty');
          final name = scheme['name'] as String;
          if (scheme['location'] == 'query') {
            final pairs = request.url.query
                .split('&')
                .where((part) => part.isNotEmpty && Uri.decodeComponent(part.split('=').first) != name)
                .toList()
              ..add('${_encode(name)}=${_encode(key)}');
            return Authentication(request.copyWith(url: request.url.replace(query: pairs.join('&'))), credentialed: true);
          }
          final prefix = scheme['scheme'] == null ? '' : '${scheme['scheme']} ';
          return Authentication(request.copyWith(headers: {...request.headers, name.toLowerCase(): '$prefix$key'}), credentialed: true);
        }
      case 'sigV4':
        final identity = await config.credentials?.call();
        scope.check();
        if (identity != null) {
          final region = scheme['signingRegion'] as String? ?? config.region;
          if (region == null) throw ArgumentError('A region is required for SigV4');
          return Authentication(request,
              credentialed: true,
              sign: (request) => signV4(request, identity,
                  service: scheme['signingName'] as String,
                  region: region,
                  unsignedPayload: op['unsignedPayload'] == true,
                  disableDoubleEncoding: scheme['disableDoubleEncoding'] as bool?,
                  disableNormalizePath: scheme['disableNormalizePath'] as bool?));
        }
    }
  }
  if (ids.any((id) => id != 'smithy.api#httpBearerAuth') && op['optionalAuth'] != true) {
    throw StateError('No credentials available for the operation\'s authentication schemes');
  }
  return Authentication(request);
}

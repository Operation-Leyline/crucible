import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:crypto/crypto.dart';
import 'auth_platform_stub.dart' if (dart.library.io) 'auth_platform_io.dart' as platform;
import 'cancellation.dart';
import 'config.dart';
import 'transport.dart';

final class BasicCredentials {
  final String username;
  final String password;
  BasicCredentials(this.username, this.password) {
    if (username.contains(':')) throw ArgumentError('Basic username cannot contain a colon');
  }
  @override
  String toString() => 'BasicCredentials([redacted])';
}
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
typedef AwsCredentialProvider = FutureOr<AwsCredentials?> Function();
AwsCredentialProvider credentialChain(List<AwsCredentialProvider> providers) => () async {
  for (final provider in providers) { final identity = await provider(); if (identity != null) return identity; }
  return null;
};
AwsCredentials? environmentCredentials() {
  final values = platform.environment();
  return _identity(values['AWS_ACCESS_KEY_ID'], values['AWS_SECRET_ACCESS_KEY'], values['AWS_SESSION_TOKEN']);
}
AwsCredentials? profileCredentials({String? profile, String? path}) {
  final environment = platform.environment();
  path ??= environment['AWS_SHARED_CREDENTIALS_FILE'] ?? '${environment['HOME'] ?? environment['USERPROFILE']}/.aws/credentials';
  final contents = platform.readProfile(path);
  if (contents == null) return null;
  profile ??= environment['AWS_PROFILE'] ?? 'default';
  String? section; final values = <String,String>{};
  for (var line in const LineSplitter().convert(contents)) {
    line = line.trim();
    if (line.isEmpty || line.startsWith('#') || line.startsWith(';')) continue;
    if (line.startsWith('[') && line.endsWith(']')) { section = line.substring(1,line.length - 1); continue; }
    final separator = line.indexOf('=');
    if (section == profile && separator >= 0) values[line.substring(0,separator).trim()] = line.substring(separator+1).trim();
  }
  return _identity(values['aws_access_key_id'], values['aws_secret_access_key'], values['aws_session_token']);
}
AwsCredentials? _identity(String? access, String? secret, String? token) {
  if (access == null && secret == null) return null;
  if (access == null || secret == null) throw StateError('AWS credentials are incomplete');
  return AwsCredentials(access,secret,sessionToken: token);
}
final class CachedCredentials {
  final AwsCredentialProvider provider;
  final Duration refreshBefore;
  final DateTime Function() clock;
  AwsCredentials? _value;
  DateTime? _refreshAt;
  Future<AwsCredentials?>? _pending;
  CachedCredentials(this.provider, {this.refreshBefore = const Duration(seconds:60), DateTime Function()? clock}) : clock = clock ?? DateTime.now {
    if (refreshBefore.isNegative) throw ArgumentError('Invalid credential refresh interval');
  }
  Future<AwsCredentials?> call() {
    final now = clock();
    if (_value != null && (_refreshAt == null || _refreshAt!.isAfter(now))) return Future.value(_value);
    if (_pending != null) return _pending!;
    final pending = Future<AwsCredentials?>.sync(provider).then((value) {
      _refreshAt = null;
      if (value?.expiresAt case final DateTime expires) {
        final remaining = expires.difference(clock());
        if (remaining <= Duration.zero) throw StateError('Credential provider returned expired credentials');
        final skew = refreshBefore < remaining ~/ 2 ? refreshBefore : remaining ~/ 2;
        _refreshAt = expires.subtract(skew);
      }
      return _value = value;
    });
    _pending = pending.whenComplete(() { _pending = null; });
    return _pending!;
  }
  @override
  String toString() => 'CachedCredentials([redacted])';
}
bool _secure(Uri uri) => uri.scheme == 'https' || const ['localhost','127.0.0.1','::1'].contains(uri.host.toLowerCase());
HttpRequest _authenticated(HttpRequest request) {
  if (!_secure(request.url)) throw ArgumentError('Credentials require HTTPS or loopback HTTP');
  return request;
}
Future<HttpRequest> applyAuthentication(HttpRequest request, Map<String,Object?> op, Map<String,Object?> schema, ClientConfig config, CallScope scope) async {
  final schemes = schema['authSchemes'] as Map? ?? const {};
  final ids = op['auth'] as List? ?? (op['bearer'] == true ? ['smithy.api#httpBearerAuth'] : const []);
  for (final id in ids) {
    final scheme = schemes[id] as Map? ?? const {'kind':'bearer'};
    final kind = scheme['kind']; final auth = request.headers['authorization'];
    if ((kind == 'bearer' || kind == 'basic') && auth != null && auth.toLowerCase().startsWith('$kind ')) return _authenticated(request);
    if (kind == 'sigV4' && auth != null && auth.startsWith('AWS4-HMAC-SHA256 ')) return _authenticated(request);
    if (kind == 'apiKey' && (scheme['location'] == 'query' ? request.url.queryParameters.containsKey(scheme['name']) : request.headers.containsKey((scheme['name'] as String).toLowerCase()))) return _authenticated(request);
  }
  for (final id in ids) {
    final scheme = schemes[id] as Map? ?? const {'kind':'bearer'};
    final kind = scheme['kind'];
    Object? identity;
    switch (kind) {
      case 'bearer': identity = await config.token?.call();
      case 'basic': identity = await config.basicAuth?.call();
      case 'apiKey': identity = await config.apiKey?.call();
      case 'sigV4': identity = await config.credentials?.call();
    }
    scope.check(); if (identity == null) continue;
    if (kind == 'sigV4') return _authenticated(signV4(request,identity as AwsCredentials,service: scheme['signingName'] as String,region: config.region ?? '',unsignedPayload: op['unsignedPayload'] == true));
    final headers = {...request.headers};
    if (kind == 'bearer') headers['authorization'] = 'Bearer $identity';
    if (kind == 'basic') { final basic = identity as BasicCredentials; headers['authorization'] = 'Basic ${base64Encode(utf8.encode('${basic.username}:${basic.password}'))}'; }
    if (kind == 'apiKey') {
      final key = identity as String; if (key.isEmpty) throw ArgumentError('API key must be nonempty');
      if (scheme['location'] == 'query') {
        final pairs = request.url.query.split('&').where((part) => part.isNotEmpty && Uri.decodeComponent(part.split('=')[0]) != scheme['name']).toList();
        pairs.add('${_encode(scheme['name'] as String)}=${_encode(key)}');
        return _authenticated(request.copyWith(url: request.url.replace(query: pairs.join('&'))));
      }
      headers[(scheme['name'] as String).toLowerCase()] = '${scheme['scheme'] == null ? '' : '${scheme['scheme']} '}$key';
    }
    return _authenticated(request.copyWith(headers: headers));
  }
  if (ids.any((id) => id != 'smithy.api#httpBearerAuth') && op['optionalAuth'] != true) throw StateError('No credentials for an enabled authentication scheme');
  return request;
}
String _encode(String value) => utf8.encode(value).map((byte) => (byte >= 65 && byte <= 90 || byte >= 97 && byte <= 122 || byte >= 48 && byte <= 57 || const [45,46,95,126].contains(byte)) ? String.fromCharCode(byte) : '%${byte.toRadixString(16).toUpperCase().padLeft(2,'0')}').join();
HttpRequest signV4(HttpRequest request, AwsCredentials identity, {required String service, required String region, DateTime? now, bool unsignedPayload = false}) {
  now = (now ?? DateTime.now()).toUtc();
  if (identity.expiresAt != null && !identity.expiresAt!.isAfter(now)) throw ArgumentError('AWS credentials have expired');
  if (service.isEmpty || region.isEmpty || RegExp(r'[/\r\n]').hasMatch(service+region)) throw ArgumentError('Invalid SigV4 service or region');
  final headers = {for (final entry in request.headers.entries) entry.key.toLowerCase():entry.value}..remove('authorization')..remove('x-amz-security-token');
  String padded(int value,int length) => value.toString().padLeft(length,'0');
  final date = '${padded(now.year,4)}${padded(now.month,2)}${padded(now.day,2)}';
  headers['x-amz-date'] = '${date}T${padded(now.hour,2)}${padded(now.minute,2)}${padded(now.second,2)}Z';
  if (identity.sessionToken != null) headers['x-amz-security-token'] = identity.sessionToken!;
  final digest = unsignedPayload ? 'UNSIGNED-PAYLOAD' : sha256.convert(request.body ?? Uint8List(0)).toString(); headers['x-amz-content-sha256'] = digest;
  headers['host'] = request.url.authority.toLowerCase();
  final raw = request.url.toString().substring('${request.url.scheme}://${request.url.authority}'.length);
  var path = raw.split('?')[0]; if (path.isEmpty) path = '/';
  if (service != 's3') {
    final parts = <String>[];
    for (final part in path.split('/')) { if (part == '..') { if (parts.isNotEmpty) parts.removeLast(); } else if (part.isNotEmpty && part != '.') { parts.add(part); } }
    path = '/${parts.join('/')}${path.endsWith('/') && parts.isNotEmpty ? '/' : ''}';
    path = _encode(path).replaceAll('%2F','/');
  }
  final query = <List<String>>[];
  for (final part in request.url.query.split('&')) { if (part.isEmpty) continue; final i = part.indexOf('='); query.add([_encode(Uri.decodeComponent(i < 0 ? part : part.substring(0,i))),_encode(Uri.decodeComponent(i < 0 ? '' : part.substring(i+1)))]); }
  query.sort((a,b) { final compare = a[0].compareTo(b[0]); return compare != 0 ? compare : a[1].compareTo(b[1]); });
  const excluded = ['authorization','connection','expect','transfer-encoding','user-agent','x-amzn-trace-id','accept-encoding','content-length'];
  final names = headers.keys.where((key)=>!excluded.contains(key)).toList()..sort();
  final signed = names.join(';'); final lines = names.map((name)=>'$name:${headers[name]!.trim().replaceAll(RegExp(r'\s+'),' ')}\n').join();
  final canonical = [request.method,path,query.map((p)=>p.join('=')).join('&'),lines,signed,digest].join('\n');
  final scope = '$date/$region/$service/aws4_request';
  final text = 'AWS4-HMAC-SHA256\n${headers['x-amz-date']}\n$scope\n${sha256.convert(utf8.encode(canonical))}';
  var key = utf8.encode('AWS4${identity.secretAccessKey}'); for (final part in [date,region,service,'aws4_request']) { key = Hmac(sha256,key).convert(utf8.encode(part)).bytes; }
  headers['authorization'] = 'AWS4-HMAC-SHA256 Credential=${identity.accessKeyId}/$scope, SignedHeaders=$signed, Signature=${Hmac(sha256,key).convert(utf8.encode(text))}';
  return request.copyWith(headers: headers);
}

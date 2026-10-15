import 'cancellation.dart';
import 'config.dart';
import 'transport.dart';

/// The outcome of credential selection for one attempt.
final class Authentication {
  final HttpRequest request;
  final bool credentialed;
  const Authentication(this.request, {this.credentialed = false});
}

/// Adds the caller's bearer token to operations that accept one. A request
/// that already carries an `Authorization: Bearer` header bypasses the token
/// provider.
Future<Authentication> authenticate(HttpRequest request, Map<String, Object?> op, ClientConfig config, CallScope scope) async {
  if (op['bearer'] != true) return Authentication(request);
  final preset = request.headers['authorization'] ?? '';
  if (preset.length > 7 && preset.substring(0, 7).toLowerCase() == 'bearer ') return Authentication(request, credentialed: true);
  final token = await config.token?.call();
  scope.check();
  if (token == null) return Authentication(request);
  return Authentication(request.copyWith(headers: {...request.headers, 'authorization': 'Bearer $token'}), credentialed: true);
}

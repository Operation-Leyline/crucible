import 'dart:async';
import 'transport.dart';

typedef TokenProvider = FutureOr<String?> Function();
typedef Middleware = FutureOr<HttpRequest> Function(HttpRequest request);

final class ClientConfig {
  final String? endpoint;
  final TokenProvider? token;
  final HttpTransport? transport;
  final Duration timeout;
  final int maxAttempts;
  final Duration baseDelay;
  final Duration maxDelay;
  final Map<String, String> headers;
  final List<Middleware> middleware;
  ClientConfig(
      {this.endpoint,
      this.token,
      this.transport,
      this.timeout = const Duration(seconds: 30),
      this.maxAttempts = 3,
      this.baseDelay = const Duration(milliseconds: 100),
      this.maxDelay = const Duration(seconds: 10),
      Map<String, String> headers = const {},
      List<Middleware> middleware = const []})
      : headers = Map.unmodifiable(headers),
        middleware = List.unmodifiable(middleware) {
    if (timeout <= Duration.zero ||
        maxAttempts < 1 ||
        baseDelay < Duration.zero ||
        maxDelay < Duration.zero) {
      throw ArgumentError('Invalid timeout, retry delay or attempt limit');
    }
  }
}

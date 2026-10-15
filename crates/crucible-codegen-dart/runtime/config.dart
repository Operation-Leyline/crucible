import 'dart:async';
import 'transport.dart';

/// Supplies a bearer token for each request; null sends the request without one.
typedef TokenProvider = FutureOr<String?> Function();

/// Rewrites a request before it is sent. Runs on every attempt,
/// so a retry gets fresh values.
typedef Middleware = FutureOr<HttpRequest> Function(HttpRequest request);

/// Settings for a generated client, fixed when the client is constructed.
///
/// Every field is optional. Without credentials, only operations that need
/// none can succeed. A configuration is immutable and may be shared by any
/// number of clients.
final class ClientConfig {
  /// The service's base URL, overriding the one generated into the client.
  /// Must be absolute HTTP(S) without credentials, query or fragment.
  final String? endpoint;

  /// Bearer token source, for services that use HTTP bearer authentication.
  final TokenProvider? token;

  /// Sends the HTTP requests. Defaults to `package:http`'s native or browser
  /// client, which the generated client closes with itself. A transport you
  /// supply stays yours to close.
  final HttpTransport? transport;

  /// The time one call may take in total, covering every attempt and retry
  /// delay. Exceeding it fails the call with a `TimeoutException`.
  final Duration timeout;

  /// The most times a request is sent, the first attempt included. 1 disables
  /// retries. Only operations that are read-only or idempotent are retried
  /// after a network failure or a 500, 502, 503, 504 or retryable response;
  /// throttling responses are retried for any operation.
  final int maxAttempts;

  /// The ceiling of the first retry delay. Delays use exponential backoff with
  /// full jitter, doubling this value per attempt up to [maxDelay]; a
  /// `Retry-After` response header takes precedence.
  final Duration baseDelay;

  /// The ceiling of any single backoff delay.
  final Duration maxDelay;

  /// Headers added to every request.
  final Map<String, String> headers;

  /// Request rewriters, run in order on every attempt.
  final List<Middleware> middleware;

  /// Throws [ArgumentError] when [timeout] is not positive, [maxAttempts] is
  /// below 1, or a delay is negative.
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

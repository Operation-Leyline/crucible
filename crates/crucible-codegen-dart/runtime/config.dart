import 'dart:async';
import 'auth.dart';
import 'checksums.dart';
import 'transport.dart';

typedef TokenProvider = FutureOr<String?> Function();
typedef Middleware = FutureOr<HttpRequest> Function(HttpRequest request);

final class ClientConfig {
  final String? endpoint;
  final TokenProvider? token;
  final BasicCredentialProvider? basicAuth;
  final ApiKeyProvider? apiKey;
  final AwsCredentialProvider? credentials;

  /// The SigV4 signing region and the `AWS::Region` endpoint built-in.
  final String? region;

  /// Client context parameters for modeled endpoint rules.
  final Map<String, Object?> endpointParameters;

  /// Overrides for endpoint rule built-ins such as `AWS::UseFIPS`.
  final Map<String, Object?> endpointBuiltIns;
  final bool disableHostPrefix;
  final ChecksumPolicy requestChecksumCalculation;
  final ChecksumPolicy responseChecksumValidation;

  /// Called with the operation and algorithm after a response checksum matches.
  final void Function(String operation, String algorithm)? checksumValidated;
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
      this.basicAuth,
      this.apiKey,
      this.credentials,
      this.region,
      Map<String, Object?> endpointParameters = const {},
      Map<String, Object?> endpointBuiltIns = const {},
      this.disableHostPrefix = false,
      this.requestChecksumCalculation = ChecksumPolicy.whenSupported,
      this.responseChecksumValidation = ChecksumPolicy.whenSupported,
      this.checksumValidated,
      this.transport,
      this.timeout = const Duration(seconds: 30),
      this.maxAttempts = 3,
      this.baseDelay = const Duration(milliseconds: 100),
      this.maxDelay = const Duration(seconds: 10),
      Map<String, String> headers = const {},
      List<Middleware> middleware = const []})
      : endpointParameters = Map.unmodifiable(_parameters(endpointParameters)),
        endpointBuiltIns = Map.unmodifiable(_parameters(endpointBuiltIns)),
        headers = Map.unmodifiable(headers),
        middleware = List.unmodifiable(middleware) {
    if (timeout <= Duration.zero ||
        maxAttempts < 1 ||
        baseDelay < Duration.zero ||
        maxDelay < Duration.zero) {
      throw ArgumentError('Invalid timeout, retry delay or attempt limit');
    }
  }

  static Map<String, Object?> _parameters(Map<String, Object?> values) => {
        for (final entry in values.entries)
          entry.key: switch (entry.value) {
            null || String() || bool() => entry.value,
            List list when list.every((item) => item is String) => List<String>.unmodifiable(list.cast<String>()),
            _ => throw ArgumentError('Endpoint parameters must be strings, booleans or string lists'),
          }
      };
}

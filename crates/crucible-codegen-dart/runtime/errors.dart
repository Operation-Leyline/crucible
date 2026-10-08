/// What the transport learned about a failed call, apart from the modeled body.
final class ErrorMetadata {
  /// The error's shape name as received, or `UnknownError` when none was sent.
  final String name;

  /// The service's human-readable message, empty when it sent none.
  final String message;

  /// The HTTP status code, or 0 for a failure that has no HTTP response.
  final int status;

  /// The request ID from the response headers, when the service sent one.
  final String? requestId;

  /// Response headers with lower-case names.
  final Map<String, String> headers;

  /// Whether the retry policy treats this failure as transient.
  final bool retryable;

  /// Whether the service asked the client to slow down.
  final bool throttling;
  const ErrorMetadata(
      {this.name = 'UnknownError',
      this.message = '',
      this.status = 0,
      this.requestId,
      this.headers = const {},
      this.retryable = false,
      this.throttling = false});

  /// Omits [message] and [headers]: they may carry sensitive data.
  @override
  String toString() =>
      'ErrorMetadata(name=$name, status=$status${requestId == null ? '' : ', requestId=$requestId'}${retryable ? ', retryable=true' : ''}${throttling ? ', throttling=true' : ''})';
}

/// A response the service sent that is not a success.
///
/// Catch [ServiceException] for every modeled and unknown service error, or a
/// generated subclass for one modeled error. Transport failures are a
/// [NetworkException] instead.
abstract class ServiceError implements Exception {
  /// Status, request ID and retry classification of the failed response.
  final ErrorMetadata metadata;
  const ServiceError(this.metadata);
  @override
  String toString() => '${metadata.name}: ${metadata.message}';
}

/// The base of every error this service returns, modeled or not.
class ServiceException extends ServiceError {
  const ServiceException(super.metadata);
}

/// An error response this client's model does not describe, such as one added
/// by a newer server, or one whose body could not be decoded.
final class UnknownServiceError extends ServiceException {
  const UnknownServiceError(super.metadata);
}

/// The request never produced a response: DNS, connection, TLS or I/O failure.
final class NetworkException implements Exception {
  /// The underlying transport error.
  final Object cause;
  const NetworkException(this.cause);
  @override
  String toString() => 'NetworkException: $cause';
}

/// The caller's `CancellationToken` was cancelled before the call finished.
final class CancelledException implements Exception {
  const CancelledException();
  @override
  String toString() => 'Request cancelled';
}

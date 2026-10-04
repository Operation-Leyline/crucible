final class ErrorMetadata {
  final String name;
  final String message;
  final int status;
  final String? requestId;
  final Map<String, String> headers;
  final bool retryable;
  final bool throttling;
  const ErrorMetadata(
      {this.name = 'UnknownError',
      this.message = '',
      this.status = 0,
      this.requestId,
      this.headers = const {},
      this.retryable = false,
      this.throttling = false});
}

abstract class ServiceError implements Exception {
  final ErrorMetadata metadata;
  const ServiceError(this.metadata);
  @override
  String toString() => '${metadata.name}: ${metadata.message}';
}

final class UnknownServiceError extends ServiceError {
  const UnknownServiceError(super.metadata);
}

final class NetworkException implements Exception {
  final Object cause;
  const NetworkException(this.cause);
  @override
  String toString() => 'NetworkException: $cause';
}

final class CancelledException implements Exception {
  const CancelledException();
  @override
  String toString() => 'Request cancelled';
}

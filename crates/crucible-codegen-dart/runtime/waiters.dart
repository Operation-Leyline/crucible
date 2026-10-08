import 'dart:math';
import 'errors.dart';
import 'json.dart';
import 'query.dart';

/// A waiter reached a failure state, or a matcher could not be evaluated.
class WaiterException implements Exception {
  /// The waiter's name.
  final String name;

  /// How many times the operation was called.
  final int attempts;

  /// The last output received, if any.
  final Object? response;

  /// The last error received, if any.
  final Object? error;

  /// The acceptor's message: a String or a List<String>, when modeled.
  final Object? message;
  const WaiterException(this.name, this.attempts, {this.response, this.error, this.message});
  @override
  String toString() => 'Waiter $name failed after $attempts attempts${message == null ? '' : ': $message'}';
}

/// The waiter's total deadline elapsed before a terminal state.
final class WaiterTimeoutException extends WaiterException {
  const WaiterTimeoutException(super.name, super.attempts, {super.response, super.error});
  @override
  String toString() => 'Waiter $name timed out after $attempts attempts';
}

/// Evaluates modeled acceptors in order: the first match decides the state.
(String, Object?) waiterState(Map<String, Object?> description, Object? input, Object? output, Object? error) {
  for (final item in description['acceptors'] as List) {
    final acceptor = item as Map<String, Object?>;
    final matcher = acceptor['matcher'] as Map<String, Object?>;
    final kind = matcher['kind'];
    var matched = false;
    if (kind == 'success') {
      matched = (error == null) == matcher['expected'];
    } else if (kind == 'errorType') {
      final name = matcher['name'] as String;
      matched = error is ServiceError && (error.metadata.name == name || error.metadata.name == name.split('#').last);
    } else if (error == null) {
      final source = kind == 'inputOutput' ? {'input': input, 'output': output} : output;
      final result = queryEvaluate((matcher['path'] as Map<String, Object?>)['expression'] as Map<String, Object?>, source);
      final expected = matcher['expected'] as String;
      matched = switch (matcher['comparator']) {
        'stringEquals' => result is String && result == expected,
        'booleanEquals' => result is bool && result == (expected == 'true'),
        'allStringEquals' => result is List && result.isNotEmpty && result.every((value) => value is String && value == expected),
        'anyStringEquals' => result is List && result.any((value) => value is String && value == expected),
        _ => false,
      };
    }
    if (matched) {
      Object? message;
      if (acceptor['message'] case final Map<String, Object?> path) {
        final value = queryEvaluate(path['expression'] as Map<String, Object?>, output);
        if (value is String || value is List && value.every((item) => item is String)) message = value;
      }
      return (acceptor['state'] as String, message);
    }
  }
  return (error == null ? 'retry' : 'failure', null);
}

Duration waiterDelay(int attempt, Duration remaining, Duration minimum, Duration maximum, [Random? random]) {
  final cap = minimum.inMicroseconds * pow(2, min(attempt - 1, 30));
  final ceiling = min(maximum.inMicroseconds, cap).toInt();
  final delay = Duration(
      microseconds: minimum.inMicroseconds + ((random ?? Random()).nextDouble() * (ceiling - minimum.inMicroseconds)).round());
  if (remaining <= minimum) return remaining;
  return remaining - delay <= minimum ? remaining - minimum : delay;
}

Duration waiterSeconds(Object? value) {
  if (value is! JsonNumber) throw const FormatException('Invalid waiter delay');
  return Duration(seconds: value.integer(min: BigInt.zero, max: BigInt.from(8640000000)).toInt());
}

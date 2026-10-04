import 'dart:async';
import 'errors.dart';

/// Caller-owned cancellation shared by any number of requests.
final class CancellationToken {
  Object? _reason;
  final _listeners = <void Function()>{};
  bool get isCancelled => _reason != null;
  Object? get reason => _reason;
  void cancel() => cancelWith(const CancelledException());
  void cancelWith(Object reason) {
    if (_reason != null) return;
    _reason = reason;
    final listeners = _listeners.toList();
    _listeners.clear();
    for (final listener in listeners) {
      listener();
    }
  }

  void throwIfCancelled() {
    if (_reason case final reason?) throw reason;
  }

  /// The returned function removes the listener when a call completes normally.
  void Function() listen(void Function() listener) {
    if (isCancelled) {
      listener();
      return () {};
    }
    _listeners.add(listener);
    return () {
      _listeners.remove(listener);
    };
  }
}

/// Owns a call's deadline and cancellation link, never the caller's token.
final class CallScope {
  final token = CancellationToken();
  final Duration timeout;
  final _watch = Stopwatch()..start();
  late final _LongTimer _timer;
  late final void Function() _unlink;
  CallScope(this.timeout, CancellationToken? caller) {
    _unlink = caller?.listen(() => token.cancelWith(caller.reason!)) ?? () {};
    _timer = _LongTimer(timeout, _expire);
  }
  void _expire() =>
      token.cancelWith(TimeoutException('Request deadline exceeded', timeout));
  void check() {
    if (_watch.elapsed >= timeout) _expire();
    token.throwIfCancelled();
  }

  Future<T> run<T>(FutureOr<T> Function() work) async {
    final stopped = Completer<T>();
    final unlink = token.listen(() => stopped.completeError(token.reason!));
    // Register both outcomes before work runs, including an already-cancelled caller.
    final operation = Future<T>(() async {
      check();
      return await work();
    });
    try {
      return await Future.any([operation, stopped.future]);
    } finally {
      unlink();
    }
  }

  Future<void> sleep(Duration delay) async {
    check();
    final completed = Completer<void>();
    final timer = _LongTimer(delay, () => completed.complete());
    final unlink = token.listen(() {
      if (!completed.isCompleted) completed.completeError(token.reason!);
    });
    try {
      await completed.future;
      check();
    } finally {
      timer.cancel();
      unlink();
    }
  }

  void close() {
    _timer.cancel();
    _unlink();
    _watch.stop();
  }
}

// Browser timers accept signed 32-bit milliseconds. Chunk longer waits so a
// large Retry-After or caller deadline cannot wrap into an immediate timer.
final class _LongTimer {
  final Duration duration;
  final void Function() callback;
  final _watch = Stopwatch()..start();
  late Timer _timer;
  bool _cancelled = false;
  _LongTimer(this.duration, this.callback) {
    _schedule();
  }
  void _schedule() {
    final remaining = duration.inMicroseconds - _watch.elapsedMicroseconds;
    final milliseconds = remaining <= 0 ? 0 : (remaining / 1000).ceil();
    _timer =
        Timer(Duration(milliseconds: milliseconds.clamp(0, 2147483647)), () {
      if (_cancelled) return;
      if (_watch.elapsed < duration) {
        _schedule();
        return;
      }
      _watch.stop();
      callback();
    });
  }

  void cancel() {
    _cancelled = true;
    _timer.cancel();
    _watch.stop();
  }
}

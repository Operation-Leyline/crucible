import java.nio.ByteBuffer;
import java.util.ArrayDeque;
import java.util.List;
import java.util.Objects;
import java.util.concurrent.Flow;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;

/**
 * Carries one streaming body between a source and a single subscriber. It requests one
 * upstream chunk at a time and only while the subscriber has demand, so memory stays bounded
 * by a chunk. It enforces the declared length and modeled bounds, verifies a streaming
 * response checksum at the end, and is aborted by the call's deadline or by close.
 */
final class StreamBody implements Flow.Publisher<ByteBuffer> {
  private final Flow.Publisher<List<ByteBuffer>> upstream;
  private final Long length, minimum, maximum;
  private final Checksums.Running checksum;
  private final Runnable cleanup;
  private final AtomicBoolean finished = new AtomicBoolean();
  private final AtomicInteger work = new AtomicInteger();
  private final ArrayDeque<ByteBuffer> queue = new ArrayDeque<>();
  private Flow.Subscriber<? super ByteBuffer> downstream;
  private Flow.Subscription source;
  private boolean upstreamSubscribed, requested, complete, terminated, cancelled;
  private long demand, size;
  private Throwable error;
  private volatile Throwable failure;

  private StreamBody(Flow.Publisher<List<ByteBuffer>> upstream, Long length, Long minimum, Long maximum,
      Checksums.Running checksum, Runnable cleanup) {
    this.upstream = upstream; this.length = length; this.minimum = minimum; this.maximum = maximum;
    this.checksum = checksum; this.cleanup = cleanup;
  }

  static StreamBody response(Flow.Publisher<List<ByteBuffer>> body, Long minimum, Long maximum,
      Checksums.Running checksum, Runnable cleanup) {
    return new StreamBody(body, null, minimum, maximum, checksum, cleanup);
  }

  static StreamBody request(Flow.Publisher<ByteBuffer> body, Long length, Long minimum, Long maximum) {
    Flow.Publisher<List<ByteBuffer>> lists = subscriber -> body.subscribe(new Flow.Subscriber<ByteBuffer>() {
      public void onSubscribe(Flow.Subscription subscription) { subscriber.onSubscribe(subscription); }
      public void onNext(ByteBuffer item) { subscriber.onNext(List.of(item)); }
      public void onError(Throwable throwable) { subscriber.onError(throwable); }
      public void onComplete() { subscriber.onComplete(); }
    });
    return new StreamBody(lists, length, minimum, maximum, null, () -> {});
  }

  /** The source or validation failure, as distinct from a transport cancellation. */
  Throwable failure() { return failure; }

  @Override public void subscribe(Flow.Subscriber<? super ByteBuffer> subscriber) {
    Objects.requireNonNull(subscriber, "subscriber");
    Throwable rejected = null;
    boolean subscribeUpstream = false;
    synchronized (this) {
      if (downstream != null) rejected = new IllegalStateException("stream already opened");
      else if (terminated) rejected = error == null ? new IllegalStateException("stream closed") : error;
      else {
        downstream = subscriber;
        subscribeUpstream = !upstreamSubscribed;
        upstreamSubscribed = true;
      }
    }
    if (rejected != null) {
      subscriber.onSubscribe(new Flow.Subscription() { public void request(long n) {} public void cancel() {} });
      subscriber.onError(rejected);
      return;
    }
    subscriber.onSubscribe(new Flow.Subscription() {
      public void request(long n) {
        if (n <= 0) { abort(new IllegalArgumentException("demand must be positive")); return; }
        synchronized (StreamBody.this) { demand = demand > Long.MAX_VALUE - n ? Long.MAX_VALUE : demand + n; }
        drain();
      }
      public void cancel() { cancelled(); }
    });
    if (subscribeUpstream) upstream.subscribe(new Inner());
    drain();
  }

  /** Ends the stream with {@code reason}; a subscriber receives it as onError. */
  void abort(Throwable reason) {
    Flow.Subscription current;
    boolean subscribeUpstream, idle;
    synchronized (this) {
      if (terminated || cancelled) return;
      if (error == null) error = reason;
      cancelled = true;
      queue.clear();
      current = source;
      subscribeUpstream = !upstreamSubscribed;
      upstreamSubscribed = true;
      idle = downstream == null;
      if (idle) terminated = true;
    }
    if (current != null) current.cancel();
    // An unsubscribed HTTP body still holds its connection until it is subscribed and cancelled.
    if (subscribeUpstream) upstream.subscribe(new Inner());
    if (idle) finish();
    else drain();
  }

  private void cancelled() {
    Flow.Subscription current;
    synchronized (this) {
      if (terminated) return;
      terminated = true;
      cancelled = true;
      queue.clear();
      current = source;
    }
    if (current != null) current.cancel();
    finish();
  }

  private void finish() {
    if (finished.compareAndSet(false, true)) cleanup.run();
  }

  private void fail(Throwable reason) {
    failure = reason;
    synchronized (this) { if (error == null) error = reason; }
  }

  private void drain() {
    if (work.getAndIncrement() != 0) return;
    int missed = 1;
    for (;;) {
      for (;;) {
        Flow.Subscriber<? super ByteBuffer> subscriber;
        ByteBuffer next = null;
        Throwable failed = null;
        boolean completed = false;
        Flow.Subscription pull = null;
        synchronized (this) {
          subscriber = downstream;
          if (terminated || subscriber == null) break;
          if (error != null) { terminated = true; failed = error; queue.clear(); }
          else if (demand > 0 && !queue.isEmpty()) { next = queue.poll(); demand--; }
          else if (queue.isEmpty() && complete) { terminated = true; completed = true; }
          else if (demand > 0 && !requested && source != null) { requested = true; pull = source; }
          else break;
        }
        if (failed != null) {
          Flow.Subscription current;
          synchronized (this) { current = source; }
          if (current != null && !complete) current.cancel();
          finish();
          subscriber.onError(failed);
          break;
        }
        if (completed) { finish(); subscriber.onComplete(); break; }
        if (next != null) {
          try { subscriber.onNext(next); }
          catch (Throwable thrown) { abort(thrown); }
          continue;
        }
        pull.request(1);
      }
      missed = work.addAndGet(-missed);
      if (missed == 0) return;
    }
  }

  private final class Inner implements Flow.Subscriber<List<ByteBuffer>> {
    @Override public void onSubscribe(Flow.Subscription subscription) {
      boolean cancel;
      synchronized (StreamBody.this) {
        cancel = source != null || cancelled || terminated;
        if (source == null) source = subscription;
      }
      if (cancel) subscription.cancel();
      else drain();
    }

    @Override public void onNext(List<ByteBuffer> items) {
      Throwable exceeded = null;
      synchronized (StreamBody.this) {
        requested = false;
        if (terminated || cancelled) return;
        for (ByteBuffer item : items) {
          if (!item.hasRemaining()) continue;
          size += item.remaining();
          if (checksum != null) checksum.update(item.duplicate());
          queue.add(item.asReadOnlyBuffer());
        }
        if ((length != null && size > length) || (maximum != null && size > maximum))
          exceeded = new IllegalArgumentException("stream exceeds its declared length");
      }
      if (exceeded != null) fail(exceeded);
      drain();
    }

    @Override public void onError(Throwable throwable) {
      synchronized (StreamBody.this) {
        requested = false;
        if (terminated || cancelled) return;
      }
      fail(throwable);
      drain();
    }

    @Override public void onComplete() {
      Throwable invalid = null;
      synchronized (StreamBody.this) {
        requested = false;
        if (terminated || cancelled) return;
        complete = true;
        if ((length != null && size != length) || (minimum != null && size < minimum))
          invalid = new IllegalArgumentException("stream ended before its declared length");
      }
      if (invalid == null && checksum != null) {
        try { checksum.verify(); } catch (RuntimeException mismatch) { invalid = mismatch; }
      }
      if (invalid != null) fail(invalid);
      drain();
    }
  }
}

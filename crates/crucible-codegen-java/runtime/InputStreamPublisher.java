import java.io.IOException;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.util.Objects;
import java.util.concurrent.Flow;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.Supplier;

/**
 * Reads an InputStream one chunk per unit of demand on a worker thread, so at most one
 * chunk is held per request. The stream is opened at the first demand and closed at its
 * end, on failure, or on cancellation (which also unblocks a pending read).
 */
final class InputStreamPublisher implements Flow.Publisher<ByteBuffer> {
  private static final int CHUNK = 64 * 1024;
  private final Supplier<? extends InputStream> factory;
  private final AtomicBoolean subscribed = new AtomicBoolean();

  InputStreamPublisher(Supplier<? extends InputStream> factory) { this.factory = factory; }

  @Override public void subscribe(Flow.Subscriber<? super ByteBuffer> subscriber) {
    Objects.requireNonNull(subscriber, "subscriber");
    if (subscribed.getAndSet(true)) {
      subscriber.onSubscribe(new Flow.Subscription() { public void request(long n) {} public void cancel() {} });
      subscriber.onError(new IllegalStateException("stream source already subscribed"));
      return;
    }
    var reader = new Reader(subscriber);
    subscriber.onSubscribe(reader);
  }

  private final class Reader implements Flow.Subscription, Runnable {
    private final Flow.Subscriber<? super ByteBuffer> subscriber;
    private final AtomicLong demand = new AtomicLong();
    private final AtomicInteger work = new AtomicInteger();
    private volatile boolean cancelled;
    private volatile InputStream input;
    private boolean done;

    Reader(Flow.Subscriber<? super ByteBuffer> subscriber) { this.subscriber = subscriber; }

    @Override public void request(long n) {
      if (n <= 0) {
        if (!cancelled) { cancel(); subscriber.onError(new IllegalArgumentException("demand must be positive")); }
        return;
      }
      demand.getAndUpdate(current -> current > Long.MAX_VALUE - n ? Long.MAX_VALUE : current + n);
      if (work.getAndIncrement() == 0) ClientRuntime.WORKERS.execute(this);
    }

    @Override public void cancel() {
      cancelled = true;
      close();
    }

    private void close() {
      InputStream current = input;
      if (current != null) try { current.close(); } catch (IOException ignored) { }
    }

    @Override public void run() {
      int missed = 1;
      do {
        while (!done && !cancelled && demand.get() > 0) {
          try {
            if (input == null) {
              input = Objects.requireNonNull(factory.get(), "stream factory returned null");
              if (cancelled) { close(); return; }
            }
            byte[] chunk = new byte[CHUNK];
            int read = input.read(chunk);
            if (cancelled) return;
            if (read < 0) {
              done = true;
              close();
              subscriber.onComplete();
              return;
            }
            if (read == 0) continue;
            demand.decrementAndGet();
            subscriber.onNext(ByteBuffer.wrap(chunk, 0, read));
          } catch (Throwable error) {
            done = true;
            close();
            if (!cancelled) subscriber.onError(error);
            return;
          }
        }
        missed = work.addAndGet(-missed);
      } while (missed != 0);
    }
  }
}

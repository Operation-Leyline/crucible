import java.io.IOException;
import java.io.InputStream;
import java.io.InterruptedIOException;
import java.nio.ByteBuffer;
import java.util.Objects;
import java.util.concurrent.Flow;

/**
 * A blocking view of a byte publisher that requests one chunk only when the previous one is
 * consumed. Closing it, or interrupting a blocked reader, cancels the body.
 */
final class StreamInput extends InputStream implements Flow.Subscriber<ByteBuffer> {
  private Flow.Subscription subscription;
  private ByteBuffer current;
  private boolean requested, done, closed;
  private Throwable error;

  @Override public synchronized void onSubscribe(Flow.Subscription value) {
    if (subscription != null || closed) { value.cancel(); return; }
    subscription = value;
    notifyAll();
  }

  @Override public synchronized void onNext(ByteBuffer item) {
    current = item;
    requested = false;
    notifyAll();
  }

  @Override public synchronized void onError(Throwable throwable) {
    if (error == null) error = throwable;
    done = true;
    notifyAll();
  }

  @Override public synchronized void onComplete() {
    done = true;
    notifyAll();
  }

  @Override public int read() throws IOException {
    byte[] one = new byte[1];
    int count;
    while ((count = read(one, 0, 1)) == 0) { }
    return count < 0 ? -1 : one[0] & 0xff;
  }

  @Override public int read(byte[] bytes, int offset, int length) throws IOException {
    Objects.checkFromIndexSize(offset, length, bytes.length);
    if (length == 0) return 0;
    for (;;) {
      Flow.Subscription pull = null;
      synchronized (this) {
        while (pull == null) {
          if (closed) throw new IOException("stream closed");
          if (current != null && current.hasRemaining()) {
            int count = Math.min(length, current.remaining());
            current.get(bytes, offset, count);
            return count;
          }
          current = null;
          if (error != null) throw failure(error);
          if (done) return -1;
          if (!requested && subscription != null) {
            requested = true;
            pull = subscription;
            break;
          }
          try { wait(); }
          catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
            close();
            throw new InterruptedIOException("interrupted while reading a stream");
          }
        }
      }
      pull.request(1);
    }
  }

  @Override public synchronized int available() {
    return current == null ? 0 : current.remaining();
  }

  @Override public void close() {
    Flow.Subscription active;
    synchronized (this) {
      if (closed) return;
      closed = true;
      current = null;
      active = done ? null : subscription;
      notifyAll();
    }
    if (active != null) active.cancel();
  }

  private static IOException failure(Throwable error) {
    if (error instanceof IOException io) return io;
    return new IOException(error.getMessage(), error);
  }
}

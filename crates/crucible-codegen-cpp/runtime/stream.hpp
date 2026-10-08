#pragma once
#include "call_scope.hpp"
#include "types.hpp"
#include <atomic>
#include <cstdint>
#include <filesystem>
#include <functional>
#include <iosfwd>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string>

namespace crucible_generated::runtime {
/// A caller's stream failed its contract. These are never retried as network failures.
class StreamError : public std::runtime_error { public: using std::runtime_error::runtime_error; };
/// The bytes broke the declared length, the modeled @length bounds, or @requiresLength.
class StreamLengthError : public StreamError { public: using StreamError::StreamError; };
/// A one-shot stream was passed to a second call or read twice.
class StreamConsumedError : public StreamError { public: StreamConsumedError() : StreamError("One-shot stream was already opened by an earlier call") {} };
/// A digest or checksum would require buffering the stream; declare it instead.
class StreamDigestError : public StreamError { public: using StreamError::StreamError; };
/// The response stream was closed, possibly by another thread.
class StreamClosedError : public StreamError { public: StreamClosedError() : StreamError("Response stream is closed") {} };

/// Fills buffer with up to size bytes and returns how many; 0 means the end of the stream.
using StreamReader = std::function<std::size_t(char* buffer, std::size_t size)>;

/// One attempt's request body as a transport reads it: exactly length() bytes when known, else chunked.
class RequestBody {
public:
    virtual ~RequestBody() = default;
    [[nodiscard]] virtual std::optional<std::uint64_t> length() const = 0;
    /// Returns 0 at the end. Throws the stream's own error, which the transport must rethrow unchanged.
    virtual std::size_t read(char* buffer, std::size_t size) = 0;
};
/// A response body returned at its headers and read on demand.
class ResponseBody {
public:
    virtual ~ResponseBody() = default;
    /// Returns 0 at the end; throws DeadlineError, CancelledError, NetworkError, or StreamClosedError.
    virtual std::size_t read(char* buffer, std::size_t size) = 0;
    /// Releases the connection. Safe to call from another thread to end a blocked read.
    virtual void close() noexcept = 0;
};

namespace detail { struct StreamSource; }

/// A @streaming blob. Requests choose a replayable or one-shot source; responses are
/// read once, pulling bytes from the connection only as the caller asks for them.
/// Copies share the same stream; the last copy releases a response's connection.
class ByteStream {
    std::shared_ptr<detail::StreamSource> source_;
    std::shared_ptr<ResponseBody> response_;
    std::shared_ptr<std::optional<StreamReader>> direct_;
    std::string sha256_;
public:
    /// No body: an empty request, or an unset response.
    ByteStream() = default;
    explicit ByteStream(std::shared_ptr<ResponseBody> response);
    /// Replayable sources with a known length.
    [[nodiscard]] static ByteStream from_bytes(Blob bytes);
    [[nodiscard]] static ByteStream from_string(std::string text);
    /// Reopened for every attempt; the length is the file size when the call starts.
    [[nodiscard]] static ByteStream from_file(std::filesystem::path path);
    /// Replayable: open is called once per attempt and must yield the same bytes each time.
    [[nodiscard]] static ByteStream from_factory(std::function<StreamReader()> open, std::optional<std::uint64_t> length = {});
    /// One-shot: read at most once, so its operation is never retried.
    [[nodiscard]] static ByteStream from_reader(StreamReader read, std::optional<std::uint64_t> length = {});
    /// One-shot over a caller-owned stream, which must outlive the call and is never closed.
    [[nodiscard]] static ByteStream from_istream(std::istream& input, std::optional<std::uint64_t> length = {});
    /// Declares the hex SHA-256 of the bytes, sent as x-amz-content-sha256 and signed, so the stream is never hashed.
    [[nodiscard]] ByteStream with_sha256(std::string hex) const;

    [[nodiscard]] bool empty() const noexcept { return !source_ && !response_; }
    [[nodiscard]] bool is_response() const noexcept { return static_cast<bool>(response_); }
    [[nodiscard]] std::optional<std::uint64_t> length() const;
    [[nodiscard]] bool replayable() const noexcept;
    [[nodiscard]] const std::string& sha256() const noexcept { return sha256_; }

    /// Reads up to size bytes; 0 means the end. A request source is opened on first use
    /// (a one-shot source read this way can no longer be sent).
    std::size_t read(char* buffer, std::size_t size);
    [[nodiscard]] Blob read_all();
    [[nodiscard]] std::string read_text();
    std::uint64_t transfer_to(std::ostream& output);
    /// Releases a response's connection; ends a read blocked in another thread.
    void close() noexcept;

    /// Runtime internals.
    [[nodiscard]] const std::shared_ptr<detail::StreamSource>& source() const noexcept { return source_; }
    [[nodiscard]] const std::shared_ptr<ResponseBody>& response() const noexcept { return response_; }
};

namespace detail {
struct StreamSource {
    std::function<StreamReader()> open;
    std::optional<std::uint64_t> length;
    bool replayable = false;
    std::shared_ptr<std::atomic<bool>> used = std::make_shared<std::atomic<bool>>(false);
    /// Opens the source for one attempt; a one-shot source opens once.
    [[nodiscard]] StreamReader acquire() const;
};
}

/// One attempt's body over a source: counts bytes against the declared length and modeled bounds.
class AttemptBody final : public RequestBody {
    std::shared_ptr<detail::StreamSource> source_;
    std::optional<std::uint64_t> min_, max_;
    CallScope scope_;
    std::optional<StreamReader> reader_;
    std::uint64_t count_ = 0;
    bool finished_ = false;
public:
    AttemptBody(std::shared_ptr<detail::StreamSource> source, std::optional<std::uint64_t> min, std::optional<std::uint64_t> max, CallScope scope);
    [[nodiscard]] std::optional<std::uint64_t> length() const override { return source_->length; }
    std::size_t read(char* buffer, std::size_t size) override;
    [[nodiscard]] bool opened() const noexcept { return reader_.has_value(); }
};
/// A response held in memory, for transports that do not stream.
class MemoryBody final : public ResponseBody {
    std::string data_;
    std::size_t offset_ = 0;
public:
    explicit MemoryBody(std::string data) : data_(std::move(data)) {}
    std::size_t read(char* buffer, std::size_t size) override;
    void close() noexcept override { data_.clear(); offset_ = 0; }
};
/// Reads a whole body: an error response, or a non-streaming response on a streaming transfer.
[[nodiscard]] std::string drain(ResponseBody& body);

/// Generated code moves streams between models and calls through these.
[[nodiscard]] inline std::optional<ByteStream> request_stream(const ByteStream& value) { return value; }
[[nodiscard]] inline std::optional<ByteStream> request_stream(const Field<ByteStream>& value) { return value ? std::optional<ByteStream>(*value) : std::nullopt; }
inline void assign_stream(ByteStream& target, std::optional<ByteStream> value) { target = value ? std::move(*value) : ByteStream(); }
inline void assign_stream(Field<ByteStream>& target, std::optional<ByteStream> value) { if (value) target = std::move(*value); else target.reset(); }
}

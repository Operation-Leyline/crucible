#include "stream.hpp"
#include <algorithm>
#include <cstring>
#include <fstream>
#include <istream>
#include <ostream>

namespace crucible_generated::runtime {
namespace {
std::shared_ptr<detail::StreamSource> replayable_source(std::function<StreamReader()> open, std::optional<std::uint64_t> length) {
    auto source = std::make_shared<detail::StreamSource>();
    source->open = std::move(open);
    source->length = length;
    source->replayable = true;
    return source;
}
StreamReader memory(std::shared_ptr<const std::string> data) {
    return [data = std::move(data), offset = std::size_t{0}](char* buffer, std::size_t size) mutable {
        const auto count = (std::min)(size, data->size() - offset);
        if (count) std::memcpy(buffer, data->data() + offset, count);
        offset += count;
        return count;
    };
}
}
StreamReader detail::StreamSource::acquire() const {
    if (!replayable && used->exchange(true)) throw StreamConsumedError();
    auto reader = open();
    if (!reader) throw StreamError("Stream factory returned no reader");
    return reader;
}
ByteStream::ByteStream(std::shared_ptr<ResponseBody> response) : response_(std::move(response)), direct_(std::make_shared<std::optional<StreamReader>>()) {}
ByteStream ByteStream::from_bytes(Blob bytes) {
    std::string text(bytes.size(), '\0');
    if (!bytes.empty()) std::memcpy(text.data(), bytes.data(), bytes.size());
    return from_string(std::move(text));
}
ByteStream ByteStream::from_string(std::string text) {
    auto data = std::make_shared<const std::string>(std::move(text));
    ByteStream result;
    result.source_ = replayable_source([data] { return memory(data); }, data->size());
    result.direct_ = std::make_shared<std::optional<StreamReader>>();
    return result;
}
ByteStream ByteStream::from_file(std::filesystem::path path) {
    ByteStream result;
    result.source_ = replayable_source([path] {
        auto file = std::make_shared<std::ifstream>(path, std::ios::binary);
        if (!*file) throw StreamError("Cannot open stream file: " + path.string());
        return StreamReader([file](char* buffer, std::size_t size) -> std::size_t {
            file->read(buffer, static_cast<std::streamsize>(size));
            if (file->bad()) throw StreamError("Stream file read failed");
            return static_cast<std::size_t>(file->gcount());
        });
    }, static_cast<std::uint64_t>(std::filesystem::file_size(path)));
    result.direct_ = std::make_shared<std::optional<StreamReader>>();
    return result;
}
ByteStream ByteStream::from_factory(std::function<StreamReader()> open, std::optional<std::uint64_t> length) {
    if (!open) throw std::invalid_argument("A stream factory is required");
    ByteStream result;
    result.source_ = replayable_source(std::move(open), length);
    result.direct_ = std::make_shared<std::optional<StreamReader>>();
    return result;
}
ByteStream ByteStream::from_reader(StreamReader read, std::optional<std::uint64_t> length) {
    if (!read) throw std::invalid_argument("A stream reader is required");
    ByteStream result;
    result.source_ = std::make_shared<detail::StreamSource>();
    result.source_->open = [read = std::move(read)] { return read; };
    result.source_->length = length;
    result.direct_ = std::make_shared<std::optional<StreamReader>>();
    return result;
}
ByteStream ByteStream::from_istream(std::istream& input, std::optional<std::uint64_t> length) {
    return from_reader([&input](char* buffer, std::size_t size) -> std::size_t {
        input.read(buffer, static_cast<std::streamsize>(size));
        if (input.bad()) throw StreamError("Stream read failed");
        return static_cast<std::size_t>(input.gcount());
    }, length);
}
ByteStream ByteStream::with_sha256(std::string hex) const {
    if (hex.size() != 64 || hex.find_first_not_of("0123456789abcdefABCDEF") != std::string::npos) throw std::invalid_argument("Declared SHA-256 must be 64 hexadecimal characters");
    for (auto& c : hex) if (c >= 'A' && c <= 'F') c = static_cast<char>(c - 'A' + 'a');
    ByteStream copy = *this;
    copy.sha256_ = std::move(hex);
    return copy;
}
std::optional<std::uint64_t> ByteStream::length() const { return source_ ? source_->length : std::nullopt; }
bool ByteStream::replayable() const noexcept { return source_ && source_->replayable; }
std::size_t ByteStream::read(char* buffer, std::size_t size) {
    if (response_) return response_->read(buffer, size);
    if (!source_ || size == 0) return 0;
    if (!*direct_) *direct_ = source_->acquire();
    return (**direct_)(buffer, size);
}
Blob ByteStream::read_all() {
    const auto text = read_text();
    Blob result(text.size());
    if (!text.empty()) std::memcpy(result.data(), text.data(), text.size());
    return result;
}
std::string ByteStream::read_text() {
    std::string result;
    char buffer[65536];
    while (const auto count = read(buffer, sizeof(buffer))) result.append(buffer, count);
    return result;
}
std::uint64_t ByteStream::transfer_to(std::ostream& output) {
    std::uint64_t total = 0;
    char buffer[65536];
    while (const auto count = read(buffer, sizeof(buffer))) {
        output.write(buffer, static_cast<std::streamsize>(count));
        if (!output) throw StreamError("Stream output write failed");
        total += count;
    }
    return total;
}
void ByteStream::close() noexcept { if (response_) response_->close(); }

AttemptBody::AttemptBody(std::shared_ptr<detail::StreamSource> source, std::optional<std::uint64_t> min, std::optional<std::uint64_t> max, CallScope scope)
    : source_(std::move(source)), min_(min), max_(max), scope_(scope) {}
std::size_t AttemptBody::read(char* buffer, std::size_t size) {
    scope_.check();
    if (finished_ || size == 0) return 0;
    if (!reader_) reader_ = source_->acquire();
    const auto declared = source_->length;
    // Never hand the transport more than the declared length.
    if (declared && size > *declared - count_) size = static_cast<std::size_t>(*declared - count_);
    std::size_t count = 0;
    if (size > 0) {
        count = (*reader_)(buffer, size);
        scope_.check();
        if (count > size) throw StreamError("Stream reader returned more bytes than requested");
    }
    count_ += count;
    if (max_ && count_ > *max_) throw StreamLengthError("The stream exceeded the modeled maximum of " + std::to_string(*max_) + " bytes");
    if (declared && count_ == *declared) {
        // A transport stops reading at Content-Length, so probe for an overlong source here.
        char extra = 0;
        if ((*reader_)(&extra, 1) != 0) throw StreamLengthError("The stream produced more than its declared " + std::to_string(*declared) + " bytes");
        scope_.check();
        finished_ = true;
        if (min_ && count_ < *min_) throw StreamLengthError("The stream ended below the modeled minimum of " + std::to_string(*min_) + " bytes");
        return count;
    }
    if (count == 0) {
        finished_ = true;
        if (declared && count_ != *declared) throw StreamLengthError("The stream ended after " + std::to_string(count_) + " of its declared " + std::to_string(*declared) + " bytes");
        if (min_ && count_ < *min_) throw StreamLengthError("The stream ended below the modeled minimum of " + std::to_string(*min_) + " bytes");
    }
    return count;
}
std::size_t MemoryBody::read(char* buffer, std::size_t size) {
    const auto count = (std::min)(size, data_.size() - offset_);
    if (count) std::memcpy(buffer, data_.data() + offset_, count);
    offset_ += count;
    return count;
}
std::string drain(ResponseBody& body) {
    std::string result;
    char buffer[65536];
    while (const auto count = body.read(buffer, sizeof(buffer))) result.append(buffer, count);
    return result;
}
}

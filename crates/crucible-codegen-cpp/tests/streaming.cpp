#include "streaming_client/client.hpp"
#include "streaming_client/schema.hpp"
#define SCRIPTED_RUNTIME streaming_sdk::runtime
#include "scripted.hpp"
#include <atomic>
#include <chrono>
#include <cstring>
#include <filesystem>
#include <sstream>
#include <thread>
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <psapi.h>
#endif

// Streaming blobs against tools/test-cpp.py's streaming loopback server (STREAM_URL).
namespace sdk = streaming_sdk;
using namespace sdk::models;
using namespace std::chrono_literals;
constexpr std::uint64_t large = 256ULL << 20;

struct Server {
    std::string url = env("STREAM_URL");
    rt::CurlTransport transport;
    rt::Json post(const std::string& path, rt::Json body = rt::Json(rt::Json::Object{})) {
        const auto response = transport.send({url + path, "POST", {{"content-type", "application/json"}}, body.dump()}, true, rt::CallScope(30));
        check(response.status == 200, "fixture control " + path);
        return rt::Json::parse(response.body);
    }
    void mode(const std::string& name) { (void)post("/__mode", rt::Json(rt::Json::Object{{"mode", rt::Json(name)}})); }
    void release() { (void)post("/__release"); }
    rt::Json last() { return post("/__last"); }
    bool aborted() { return post("/__aborted").at("aborted").boolean(); }
    std::int64_t attempts() { return last().at("attempts").number().integer(); }
};
std::int64_t number(const rt::Json& value, const char* name) { return value.at(name).number().integer(); }
template<class E, class F> E raises(F action, const std::string& message) {
    try { action(); } catch (const E& error) { return error; }
    catch (const std::exception& other) { throw std::runtime_error("Unexpected exception for " + message + ": " + other.what()); }
    throw std::runtime_error("Expected exception: " + message);
}
// Produces bytes on demand, so a large body never exists in memory.
rt::StreamReader generated(std::uint64_t size) {
    return [left = size](char* buffer, std::size_t capacity) mutable -> std::size_t {
        const auto count = static_cast<std::size_t>((std::min<std::uint64_t>)(left, capacity));
        std::memset(buffer, 'x', count);
        left -= count;
        return count;
    };
}
rt::StreamReader text_reader(std::string text) { return [text = std::move(text), offset = std::size_t{0}](char* buffer, std::size_t size) mutable { const auto count = (std::min)(size, text.size() - offset); std::memcpy(buffer, text.data() + offset, count); offset += count; return count; }; }
rt::ClientConfig settings(const Server& server, double timeout = 120) {
    rt::ClientConfig config;
    config.endpoint = server.url;
    config.base_delay = 0.001;
    config.max_delay = 0.001;
    config.timeout = timeout;
    return config;
}

struct Memory {
    static std::uint64_t current() {
#ifdef _WIN32
        PROCESS_MEMORY_COUNTERS counters{};
        GetProcessMemoryInfo(GetCurrentProcess(), &counters, sizeof(counters));
        return counters.WorkingSetSize;
#else
        return status("VmRSS:");
#endif
    }
    static std::uint64_t peak() {
#ifdef _WIN32
        PROCESS_MEMORY_COUNTERS counters{};
        GetProcessMemoryInfo(GetCurrentProcess(), &counters, sizeof(counters));
        return counters.PeakWorkingSetSize;
#else
        return status("VmHWM:");
#endif
    }
#ifndef _WIN32
    static std::uint64_t status(const std::string& key) {
        std::ifstream file("/proc/self/status");
        std::string line;
        while (std::getline(file, line)) if (line.starts_with(key)) return std::stoull(line.substr(key.size())) * 1024;
        return 0;
    }
#endif
};

// Moves 256 MiB each way over loopback HTTP; the process working set must stay bounded.
void large_streams_bounded_memory(Server& server) {
    const sdk::StreamingClient client(settings(server));
    const auto baseline = Memory::current();
    const auto started = std::chrono::steady_clock::now();
    server.mode("ok");
    (void)client.upload(UploadInput(rt::ByteStream::from_reader(generated(large))));
    auto observed = server.last();
    check(number(observed, "size") == static_cast<std::int64_t>(large) && observed.at("chunked").boolean(), "chunked upload of unknown length");
    (void)client.upload(UploadInput(rt::ByteStream::from_factory([] { return generated(large); }, large)));
    observed = server.last();
    check(number(observed, "size") == static_cast<std::int64_t>(large) && number(observed, "contentLength") == static_cast<std::int64_t>(large) && !observed.at("chunked").boolean(), "sized upload");
    server.mode("large");
    auto output = client.download();
    std::uint64_t copied = 0;
    std::vector<char> buffer(1 << 16);
    while (const auto count = output.body.read(buffer.data(), buffer.size())) copied += count;
    output.body.close();
    check(copied == large, "large download " + std::to_string(copied));
    const auto peak = Memory::peak();
    const auto growth = peak > baseline ? peak - baseline : 0;
    std::cout << "moved " << (large >> 20) << " MiB each way in " << std::chrono::duration<double>(std::chrono::steady_clock::now() - started).count()
              << " s; working set " << (baseline >> 20) << " MiB at start, peak " << (peak >> 20) << " MiB\n";
    check(growth < (64ULL << 20), "working set grew by " + std::to_string(growth >> 20) + " MiB for 256 MiB each way");
}

void uploads(Server& server) {
    const sdk::StreamingClient client(settings(server));
    // A plain reader is one-shot: sent once, chunked, even for a retryable operation.
    server.mode("503");
    const auto failure = raises<rt::ServiceError>([&] { (void)client.upload(UploadInput(rt::ByteStream::from_reader(text_reader("one shot")))); }, "503 upload");
    auto observed = server.last();
    check(failure.metadata().status == 503 && number(observed, "attempts") == 1, "one-shot dispatched once");
    check(observed.at("chunked").boolean() && observed.at("received").string() == "one shot", "one-shot body");
    const auto once = rt::ByteStream::from_reader(text_reader("once"), 4);
    check(!once.replayable() && once.length() == 4u, "one-shot properties");
    server.mode("ok");
    (void)client.upload(UploadInput(once));
    (void)raises<rt::StreamConsumedError>([&] { (void)client.upload(UploadInput(once)); }, "one-shot reused");

    // A factory opens once per attempt and reproduces the bytes; its length is sent.
    server.mode("503-once");
    int opens = 0;
    const auto factory = rt::ByteStream::from_factory([&] { ++opens; return text_reader("same bytes"); }, 10);
    check(factory.replayable(), "factory is replayable");
    (void)client.upload(UploadInput(factory));
    observed = server.last();
    check(opens == 2 && number(observed, "attempts") == 2 && observed.at("received").string() == "same bytes" && number(observed, "contentLength") == 10 && !observed.at("chunked").boolean(), "replay per attempt");

    // Bytes and files are replayable with a known length; files are reopened per attempt.
    server.mode("503-once");
    (void)client.upload(UploadInput(rt::ByteStream::from_string("replayed")));
    observed = server.last();
    check(number(observed, "attempts") == 2 && observed.at("received").string() == "replayed" && number(observed, "contentLength") == 8, "bytes replay");
    const auto path = std::filesystem::temp_directory_path() / "crucible-cpp-stream-body.bin";
    { std::ofstream file(path, std::ios::binary); file << "file bytes"; }
    server.mode("503-once");
    (void)client.upload(UploadInput(rt::ByteStream::from_file(path)));
    observed = server.last();
    check(number(observed, "attempts") == 2 && observed.at("received").string() == "file bytes" && number(observed, "contentLength") == 10, "file upload");
    std::filesystem::remove(path);
    std::istringstream input("istream bytes");
    server.mode("ok");
    (void)client.upload(UploadInput(rt::ByteStream::from_istream(input)));
    check(server.last().at("received").string() == "istream bytes", "istream upload");

    // An empty stream is an empty body.
    (void)client.upload(UploadInput(rt::ByteStream()));
    observed = server.last();
    check(number(observed, "contentLength") <= 0 && number(observed, "size") == 0, "empty upload");
    (void)client.upload(UploadInput(rt::ByteStream::from_string("")));
    check(number(server.last(), "size") == 0, "zero-length upload");

    // requiresLength and @length are checked before sending, and while bytes flow.
    server.mode("ok");
    (void)raises<rt::StreamLengthError>([&] { (void)client.sized(SizedInput(rt::ByteStream::from_reader(text_reader("abc")))); }, "unknown length accepted");
    (void)raises<rt::StreamLengthError>([&] { (void)client.sized(SizedInput(rt::ByteStream::from_string("abcd"))); }, "modeled maximum ignored");
    check(number(server.last(), "attempts") == 0, "invalid lengths were not sent");
    (void)client.sized(SizedInput(rt::ByteStream::from_string("abc")));
    observed = server.last();
    check(number(observed, "contentLength") == 3 && observed.at("received").string() == "abc", "sized upload");
    for (const auto* text : {"ab", "abcd"}) {
        (void)raises<rt::StreamLengthError>([&] { (void)client.sized(SizedInput(rt::ByteStream::from_reader(text_reader(text), 3))); }, std::string("declared length broken by ") + text);
    }
    // A failing reader surfaces its own error, never a retried network error.
    server.mode("ok");
    bool first = true;
    const auto broken = rt::ByteStream::from_factory([&] {
        return rt::StreamReader([&](char* buffer, std::size_t) -> std::size_t {
            if (first) { first = false; std::memcpy(buffer, "partial", 7); return 7; }
            throw std::runtime_error("disk failed");
        });
    });
    const auto disk = raises<std::runtime_error>([&] { (void)client.upload(UploadInput(broken)); }, "reader failure");
    check(std::string(disk.what()) == "disk failed", "reader failure is its own error");
}

void upload_cancellation(Server& server) {
    const sdk::StreamingClient client(settings(server));
    server.mode("ok");
    std::atomic<int> reads = 0;
    const auto endless = rt::ByteStream::from_reader([&](char* buffer, std::size_t) -> std::size_t {
        ++reads;
        std::this_thread::sleep_for(10ms);
        std::memcpy(buffer, "started", 7);
        return 7;
    });
    std::stop_source cancel;
    std::jthread stopper([&] { std::this_thread::sleep_for(200ms); cancel.request_stop(); });
    const auto started = std::chrono::steady_clock::now();
    (void)raises<rt::CancelledError>([&] { (void)client.upload(UploadInput(endless), cancel.get_token()); }, "cancelled upload");
    check(std::chrono::steady_clock::now() - started < 5s, "cancellation stopped the upload promptly");
    const auto after = reads.load();
    std::this_thread::sleep_for(100ms);
    check(reads.load() == after && after > 0, "the caller's stream is not read after the call ends");
}

void downloads(Server& server) {
    const sdk::StreamingClient client(settings(server));
    // The response is returned at its headers; the body is read afterwards.
    server.mode("ok");
    auto output = client.download();
    check(output.count && *output.count == 3, "headers decoded before the body");
    server.release();
    check(output.body.read_text() == "abc", "download body");
    check(output.body.read_text().empty(), "end of stream");
    output.body.close();

    // Closing an unread response, or closing mid-read, releases the connection.
    server.mode("flood");
    output = client.download();
    output.body.close();
    check(server.aborted(), "unread response was released");
    server.mode("flood");
    {
        auto dropped = client.download();
        char buffer[16];
        check(dropped.body.read(buffer, sizeof(buffer)) > 0, "partial read");
    }
    check(server.aborted(), "destroyed response was released");
    server.mode("flood");
    output = client.download();
    char buffer[16];
    check(output.body.read(buffer, sizeof(buffer)) > 0, "partial read");
    output.body.close();
    check(server.aborted(), "closed response was released");
    (void)raises<rt::StreamClosedError>([&] { (void)output.body.read(buffer, sizeof(buffer)); }, "read after close");

    // Closing from another thread ends a blocked read.
    server.mode("hang");
    output = client.download();
    std::atomic<bool> failed = false;
    std::jthread reader([&] { try { char one[1]; (void)output.body.read(one, 1); } catch (const rt::StreamClosedError&) { failed = true; } catch (...) {} });
    std::this_thread::sleep_for(200ms);
    output.body.close();
    reader.join();
    check(failed, "close ended a blocked read");

    // The call's deadline still bounds reading after the operation returned.
    server.mode("hang");
    const sdk::StreamingClient short_client(settings(server, 0.5));
    output = short_client.download();
    auto started = std::chrono::steady_clock::now();
    (void)raises<rt::DeadlineError>([&] { (void)output.body.read_text(); }, "deadline after return");
    check(std::chrono::steady_clock::now() - started < 5s, "deadline promptly ended the read");

    // Cancelling the call's stop token ends reading too.
    server.mode("hang");
    std::stop_source cancel;
    output = client.download(cancel.get_token());
    std::jthread stopper([&] { std::this_thread::sleep_for(100ms); cancel.request_stop(); });
    (void)raises<rt::CancelledError>([&] { (void)output.body.read_text(); }, "cancelled read");

    // Invalid response headers release the unread body.
    server.mode("invalid");
    (void)raises<std::invalid_argument>([&] { (void)client.download(); }, "invalid header accepted");
    check(server.aborted(), "invalid response body was released");
    server.release();
}

void checksums_and_digests(Server& server) {
    std::vector<std::string> validated;
    auto config = settings(server);
    config.checksum_validated = [&](const std::string& operation, const std::string& algorithm) { validated.push_back(operation + ":" + algorithm); };
    const sdk::StreamingClient checked(config);
    // A streaming response checksum is verified at its end.
    server.mode("ok");
    auto fetched = checked.fetch(FetchInput());
    check(validated.empty(), "not validated before reading");
    check(fetched.body.read_text() == "123456789" && validated.size() == 1 && validated[0] == "Fetch:CRC32", "streaming checksum");
    server.mode("bad");
    fetched = checked.fetch(FetchInput());
    check(raises<rt::ChecksumError>([&] { (void)fetched.body.read_text(); }, "streaming checksum mismatch").algorithm() == "CRC32", "mismatch algorithm");
    char one[1];
    (void)raises<rt::ChecksumError>([&] { (void)fetched.body.read(one, 1); }, "mismatch is sticky");

    // Request checksums are never computed by buffering a stream.
    const sdk::StreamingClient client(settings(server));
    server.mode("ok");
    (void)client.store(StoreInput(rt::ByteStream::from_string("abc")));
    auto observed = server.last();
    check(header(observed, "x-algorithm").empty() && header(observed, "x-amz-checksum-crc32").empty(), "no default checksum for a stream");
    (void)raises<rt::StreamDigestError>([&] { (void)client.store(StoreInput(rt::ByteStream::from_string("abc"), Algorithm::known_crc32())); }, "selected checksum computed from a stream");
    (void)client.store(StoreInput(rt::ByteStream::from_string("123456789"), Algorithm::known_crc32(), std::string("y/Q5Jg==")));
    observed = server.last();
    check(header(observed, "x-amz-checksum-crc32") == "y/Q5Jg==" && header(observed, "x-algorithm") == "CRC32", "supplied streaming checksum");

    // A content digest is declared, never computed from a stream.
    const std::string digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const rt::ClientRuntime digesting(settings(server), sdk::schema(), [](const std::string&, const rt::Json&, rt::ErrorMetadata) { return std::exception_ptr(); }, "", true);
    int opens = 0;
    const auto unsized = rt::ByteStream::from_factory([&] { ++opens; return text_reader("abc"); });
    const auto encoded = rt::Json(rt::Json::Object{});
    (void)raises<rt::StreamDigestError>([&] { (void)digesting.exchange("Upload", encoded, digesting.scope(), unsized); }, "undeclared digest");
    check(opens == 0, "an undeclared stream was not opened");
    std::string upper = digest;
    for (auto& c : upper) c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
    (void)digesting.exchange("Upload", encoded, digesting.scope(), unsized.with_sha256(upper));
    observed = server.last();
    check(opens == 1 && header(observed, "x-amz-content-sha256") == digest && observed.at("received").string() == "abc" && observed.at("chunked").boolean(), "declared digest");
    (void)raises<std::invalid_argument>([&] { (void)unsized.with_sha256("not hex"); }, "invalid declared digest");

    // SigV4 signs a stream with its declared digest, or as UNSIGNED-PAYLOAD.
    const rt::AwsCredentials identity{"AKIDEXAMPLE", "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY", "", std::nullopt};
    const auto signed_request = [&](const std::string& declared, bool unsigned_payload, std::shared_ptr<rt::AttemptBody>& body) {
        body = std::make_shared<rt::AttemptBody>(rt::ByteStream::from_reader(text_reader("abc")).source(), std::nullopt, std::nullopt, rt::CallScope(10));
        rt::HttpRequest request{"https://example.amazonaws.com/upload", "PUT", {}, std::nullopt, body};
        if (!declared.empty()) request.headers.emplace("X-Amz-Content-Sha256", declared);
        rt::SigningOptions options;
        options.service = "service";
        options.region = "us-east-1";
        options.time = std::chrono::sys_days{std::chrono::year{2015} / 8 / 30} + 12h + 36min;
        options.unsigned_payload = unsigned_payload;
        rt::sign_v4(request, identity, options);
        return request;
    };
    std::shared_ptr<rt::AttemptBody> body;
    (void)raises<rt::StreamDigestError>([&] { (void)signed_request("", false, body); }, "SigV4 read an undeclared stream");
    check(!body->opened(), "SigV4 left the stream unopened");
    auto request = signed_request(digest, false, body);
    check(request.headers.at("x-amz-content-sha256") == digest && request.headers.at("authorization").find("x-amz-content-sha256") != std::string::npos && !body->opened(), "SigV4 streaming digest");
    request = signed_request("", true, body);
    check(request.headers.at("x-amz-content-sha256") == "UNSIGNED-PAYLOAD", "SigV4 unsigned stream");
}

void incremental_checksum_vectors() {
    const auto vectors = rt::Json::parse(read_file(env("CHECKSUM_CASES")));
    const auto& input = vectors.at("inputUtf8").string();
    for (const auto& [algorithm, expected] : vectors.at("base64").object()) {
        rt::IncrementalChecksum digest(algorithm);
        for (const char c : input) digest.update(std::string_view(&c, 1));
        const auto raw = digest.finish();
        check(rt::base64(blob(raw)) == expected.string() && rt::checksum(algorithm, input) == expected.string(), "incremental " + algorithm);
    }
}

void custom_transport_buffers() {
    // A transport that does not override open() receives and returns buffered bodies.
    struct Echo : rt::Transport {
        rt::HttpResponse send(const rt::HttpRequest& request, bool, const rt::CallScope&) override {
            check(!request.stream, "buffered stream body");
            if (request.method == "PUT") check(request.body == "abc", "buffered upload bytes");
            return rt::HttpResponse{200, {{"x-count", "1"}}, request.body.value_or(""), nullptr};
        }
    };
    rt::ClientConfig config;
    config.endpoint = "https://example.invalid";
    config.transport = std::make_shared<Echo>();
    (void)sdk::StreamingClient(config).upload(UploadInput(rt::ByteStream::from_string("abc")));
    auto output = sdk::StreamingClient(config).download();
    check(output.body.read_text().empty() && *output.count == 1, "buffered download");
}

int main() {
    try {
        Server server;
        large_streams_bounded_memory(server);
        uploads(server);
        upload_cancellation(server);
        downloads(server);
        checksums_and_digests(server);
        incremental_checksum_vectors();
        custom_transport_buffers();
        std::cout << "C++ streaming blobs passed\n";
    } catch (const std::exception& error) {
        std::cerr << error.what() << "\n";
        return 1;
    }
}

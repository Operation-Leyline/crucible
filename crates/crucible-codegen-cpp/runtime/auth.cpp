#include "auth.hpp"
#include "checksums.hpp"
#include <openssl/evp.h>
#include <openssl/hmac.h>
#include <algorithm>
#include <array>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <map>
#include <mutex>
#include <ostream>
#include <set>

namespace crucible_generated::runtime {
namespace {
std::optional<std::string> environment(const char* name) {
#ifdef _WIN32
    char* raw = nullptr;
    std::size_t size = 0;
    if (_dupenv_s(&raw, &size, name) != 0 || !raw) return std::nullopt;
    std::string value(raw);
    std::free(raw);
    return value;
#else
    const char* value = std::getenv(name);
    return value ? std::optional<std::string>(value) : std::nullopt;
#endif
}
std::optional<AwsCredentials> static_credentials(const std::string& access, const std::string& secret, const std::string& token) {
    if (access.empty() && secret.empty()) return std::nullopt;
    if (access.empty() || secret.empty()) throw std::invalid_argument("AWS credentials are incomplete");
    return AwsCredentials{access, secret, token, std::nullopt};
}
std::string trim(std::string_view text) {
    const auto first = text.find_first_not_of(" \t\r\n");
    if (first == std::string_view::npos) return {};
    return std::string(text.substr(first, text.find_last_not_of(" \t\r\n") - first + 1));
}
std::string hmac(std::string_view key, std::string_view data) {
    std::array<unsigned char, EVP_MAX_MD_SIZE> bytes{};
    unsigned length = 0;
    if (!HMAC(EVP_sha256(), key.data(), static_cast<int>(key.size()), reinterpret_cast<const unsigned char*>(data.data()), data.size(), bytes.data(), &length)) throw std::runtime_error("HMAC failed");
    return std::string(reinterpret_cast<const char*>(bytes.data()), length);
}
std::string timestamp(std::chrono::system_clock::time_point time, bool date_only) {
    const auto day = std::chrono::floor<std::chrono::days>(time);
    const std::chrono::year_month_day date{day};
    const std::chrono::hh_mm_ss clock{std::chrono::floor<std::chrono::seconds>(time - day)};
    char buffer[32];
    if (date_only) std::snprintf(buffer, sizeof(buffer), "%04d%02u%02u", static_cast<int>(date.year()), static_cast<unsigned>(date.month()), static_cast<unsigned>(date.day()));
    else std::snprintf(buffer, sizeof(buffer), "%04d%02u%02uT%02d%02d%02dZ", static_cast<int>(date.year()), static_cast<unsigned>(date.month()), static_cast<unsigned>(date.day()),
        static_cast<int>(clock.hours().count()), static_cast<int>(clock.minutes().count()), static_cast<int>(clock.seconds().count()));
    return buffer;
}
// Removes dot segments and empty segments, keeping a trailing slash, like the AWS SDKs.
std::string normalize(const std::string& path) {
    std::vector<std::string> segments;
    std::size_t start = 0;
    while (start <= path.size()) {
        const auto end = path.find('/', start);
        const auto segment = path.substr(start, end == std::string::npos ? std::string::npos : end - start);
        if (segment == "..") { if (!segments.empty()) segments.pop_back(); }
        else if (!segment.empty() && segment != ".") segments.push_back(segment);
        if (end == std::string::npos) break;
        start = end + 1;
    }
    std::string result;
    for (const auto& segment : segments) result += "/" + segment;
    if (result.empty()) return "/";
    if (path.ends_with('/')) result += '/';
    return result;
}
}
std::ostream& operator<<(std::ostream& stream, const BasicCredentials&) { return stream << "BasicCredentials{redacted}"; }
std::ostream& operator<<(std::ostream& stream, const AwsCredentials&) { return stream << "AwsCredentials{redacted}"; }

CredentialsProvider credential_chain(std::vector<CredentialsProvider> providers) {
    return [providers = std::move(providers)](const CallScope& scope) -> std::optional<AwsCredentials> {
        for (const auto& provider : providers) {
            scope.check();
            if (!provider) continue;
            if (auto value = provider(scope)) return value;
        }
        return std::nullopt;
    };
}
std::optional<AwsCredentials> environment_credentials(const CallScope& scope) {
    scope.check();
    return static_credentials(environment("AWS_ACCESS_KEY_ID").value_or(""), environment("AWS_SECRET_ACCESS_KEY").value_or(""), environment("AWS_SESSION_TOKEN").value_or(""));
}
CredentialsProvider profile_credentials(std::string profile, std::string filename) {
    return [profile = std::move(profile), filename = std::move(filename)](const CallScope& scope) -> std::optional<AwsCredentials> {
        scope.check();
        auto name = profile.empty() ? environment("AWS_PROFILE").value_or("default") : profile;
        if (name.empty()) name = "default";
        auto source = filename.empty() ? environment("AWS_SHARED_CREDENTIALS_FILE").value_or("") : filename;
        if (source.empty()) {
#ifdef _WIN32
            const auto home = environment("USERPROFILE");
#else
            const auto home = environment("HOME");
#endif
            if (!home) return std::nullopt;
            source = *home + "/.aws/credentials";
        }
        std::ifstream file(source);
        if (!file) return std::nullopt;
        std::string section, line;
        std::map<std::string, std::string> values;
        while (std::getline(file, line)) {
            scope.check();
            const auto text = trim(line);
            if (text.empty() || text.front() == '#' || text.front() == ';') continue;
            if (text.front() == '[' && text.back() == ']') { section = trim(std::string_view(text).substr(1, text.size() - 2)); continue; }
            if (section != name) continue;
            const auto equals = text.find('=');
            if (equals == std::string::npos) throw std::invalid_argument("Invalid credentials profile entry");
            if (!values.emplace(trim(std::string_view(text).substr(0, equals)), trim(std::string_view(text).substr(equals + 1))).second) throw std::invalid_argument("Duplicate credentials profile entry");
        }
        return static_credentials(values["aws_access_key_id"], values["aws_secret_access_key"], values["aws_session_token"]);
    };
}

struct CachedCredentials::State {
    CredentialsProvider provider;
    std::chrono::system_clock::duration skew;
    std::mutex mutex;
    std::condition_variable_any changed;
    bool refreshing = false;
    std::optional<AwsCredentials> value;
    std::optional<std::chrono::system_clock::time_point> refresh_at;
};
CachedCredentials::CachedCredentials(CredentialsProvider provider, std::chrono::seconds refresh_before) : state_(std::make_shared<State>()) {
    if (!provider || refresh_before.count() < 0) throw std::invalid_argument("A provider and nonnegative refresh interval are required");
    state_->provider = std::move(provider);
    state_->skew = refresh_before;
}
std::optional<AwsCredentials> CachedCredentials::retrieve(const CallScope& scope) const {
    auto& state = *state_;
    std::unique_lock lock(state.mutex);
    for (;;) {
        scope.check();
        if (state.value && (!state.refresh_at || std::chrono::system_clock::now() < *state.refresh_at)) return state.value;
        if (state.refreshing) { scope.wait_until(state.changed, lock, [&] { return !state.refreshing; }); continue; }
        state.refreshing = true;
        lock.unlock();
        std::optional<AwsCredentials> value;
        std::exception_ptr failure;
        const auto now = std::chrono::system_clock::now();
        try {
            value = state.provider(scope);
            if (value && value->expires_at && *value->expires_at <= now) throw std::runtime_error("Credential provider returned expired credentials");
        } catch (...) { failure = std::current_exception(); }
        lock.lock();
        if (!failure) {
            state.value = value;
            state.refresh_at.reset();
            if (value && value->expires_at) state.refresh_at = *value->expires_at - (std::min)(state.skew, (*value->expires_at - now) / 2);
        }
        state.refreshing = false;
        state.changed.notify_all();
        if (failure) std::rethrow_exception(failure);
        return value;
    }
}

void sign_v4(HttpRequest& request, const AwsCredentials& credentials, const SigningOptions& options) {
    const auto now = options.time.value_or(std::chrono::system_clock::now());
    if (credentials.access_key_id.empty() || credentials.secret_access_key.empty() || (credentials.expires_at && *credentials.expires_at <= now)) throw std::invalid_argument("AWS credentials are missing or expired");
    if (options.region.empty() || options.service.empty() || (options.region + options.service).find_first_of("/\r\n") != std::string::npos) throw std::invalid_argument("SigV4 region and service are required scope components");
    Headers headers;
    for (const auto& [name, value] : request.headers) headers.insert_or_assign(lower(name), value);
    headers.erase("authorization");
    const auto stamp = timestamp(now, false);
    headers.insert_or_assign("x-amz-date", stamp);
    if (!credentials.session_token.empty()) headers.insert_or_assign("x-amz-security-token", credentials.session_token);
    else headers.erase("x-amz-security-token");
    const auto digest = options.unsigned_payload ? std::string("UNSIGNED-PAYLOAD") : hex(digest_bytes("SHA256", request.body.value_or("")));
    headers.insert_or_assign("x-amz-content-sha256", digest);
    const auto url = split_url(request.url);
    auto uri = url.path.empty() ? std::string("/") : url.path;
    const bool s3 = options.service == "s3";
    if (options.disable_normalize_path ? !*options.disable_normalize_path : !s3) uri = normalize(uri);
    if (options.disable_double_encoding ? !*options.disable_double_encoding : !s3) {
        std::string encoded;
        std::size_t start = 0;
        for (;;) {
            const auto end = uri.find('/', start);
            encoded += uri_encode(uri.substr(start, end == std::string::npos ? std::string::npos : end - start));
            if (end == std::string::npos) break;
            encoded += '/';
            start = end + 1;
        }
        uri = std::move(encoded);
    }
    std::vector<std::string> query;
    std::size_t start = 0;
    while (start < url.query.size()) {
        const auto end = url.query.find('&', start);
        const auto pair = url.query.substr(start, end == std::string::npos ? std::string::npos : end - start);
        if (!pair.empty()) {
            const auto equals = pair.find('=');
            query.push_back(uri_encode(uri_decode(pair.substr(0, equals))) + "=" + (equals == std::string::npos ? std::string{} : uri_encode(uri_decode(pair.substr(equals + 1)))));
        }
        if (end == std::string::npos) break;
        start = end + 1;
    }
    std::sort(query.begin(), query.end());
    static const std::set<std::string, std::less<>> excluded{"authorization", "connection", "expect", "transfer-encoding", "user-agent", "x-amzn-trace-id", "accept-encoding", "content-length", "host"};
    std::map<std::string, std::string> canonical_headers;
    for (const auto& [name, value] : headers) {
        if (excluded.contains(name)) continue;
        std::string collapsed;
        std::size_t index = 0;
        while (index < value.size()) {
            const auto first = value.find_first_not_of(" \t", index);
            if (first == std::string::npos) break;
            const auto last = value.find_first_of(" \t", first);
            if (!collapsed.empty()) collapsed += ' ';
            collapsed += value.substr(first, last == std::string::npos ? std::string::npos : last - first);
            index = last == std::string::npos ? value.size() : last;
        }
        canonical_headers.emplace(name, collapsed);
    }
    // libcurl omits a scheme's default port from Host; sign exactly what is sent.
    auto host = url.authority;
    if ((url.scheme == "https" && host.ends_with(":443")) || (url.scheme == "http" && host.ends_with(":80"))) host.erase(host.rfind(':'));
    canonical_headers.insert_or_assign("host", host);
    std::string header_text, signed_headers, query_text;
    for (const auto& [name, value] : canonical_headers) {
        header_text += name + ":" + value + "\n";
        if (!signed_headers.empty()) signed_headers += ';';
        signed_headers += name;
    }
    for (const auto& item : query) { if (!query_text.empty()) query_text += '&'; query_text += item; }
    const auto canonical = request.method + "\n" + uri + "\n" + query_text + "\n" + header_text + "\n" + signed_headers + "\n" + digest;
    const auto date = timestamp(now, true);
    const auto scope = date + "/" + options.region + "/" + options.service + "/aws4_request";
    const auto to_sign = "AWS4-HMAC-SHA256\n" + stamp + "\n" + scope + "\n" + hex(digest_bytes("SHA256", canonical));
    auto key = "AWS4" + credentials.secret_access_key;
    for (const auto& part : {date, options.region, options.service, std::string("aws4_request")}) key = hmac(key, part);
    headers.insert_or_assign("authorization", "AWS4-HMAC-SHA256 Credential=" + credentials.access_key_id + "/" + scope + ", SignedHeaders=" + signed_headers + ", Signature=" + hex(hmac(key, to_sign)));
    request.headers = std::move(headers);
}
}

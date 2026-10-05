#include "auth_client/client.hpp"
#define SCRIPTED_RUNTIME auth_sdk::runtime
#include "scripted.hpp"
#include <atomic>
#include <future>
#include <sstream>
#include <thread>

using namespace std::chrono;
namespace sdk = auth_sdk;

void set_environment(const char* name, const char* value) {
#ifdef _WIN32
    _putenv_s(name, value);
#else
    setenv(name, value, 1);
#endif
}

void published_signing_vector() {
    // AWS SigV4 documentation example: GET Object with a Range header.
    rt::HttpRequest request{"https://examplebucket.s3.amazonaws.com/test.txt", "GET", {{"Range", "bytes=0-9"}}, std::nullopt};
    const rt::AwsCredentials identity{"AKIAIOSFODNN7EXAMPLE", "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY", "", std::nullopt};
    rt::SigningOptions options;
    options.service = "s3";
    options.region = "us-east-1";
    options.time = sys_days{year{2013} / May / 24};
    rt::sign_v4(request, identity, options);
    const auto& authorization = request.headers.at("authorization");
    check(authorization == "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41", "published SigV4 vector: " + authorization);
    check(request.headers.at("x-amz-date") == "20130524T000000Z", "SigV4 date header");
    check(request.headers.at("x-amz-content-sha256") == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", "SigV4 payload digest");
    std::ostringstream printed;
    printed << identity << rt::BasicCredentials{"user", "password"};
    check(printed.str().find(identity.secret_access_key) == std::string::npos && printed.str().find("password") == std::string::npos, "identities print redacted");
    // Non-S3 services normalize and double-encode the path and sort the canonical query.
    rt::HttpRequest normalized{"https://example.test:443/a/./b/../c%20d?b=2&a=1", "POST", {}, std::string("{}")};
    options.service = "example";
    rt::sign_v4(normalized, rt::AwsCredentials{"id", "secret", "token", std::nullopt}, options);
    check(normalized.headers.at("x-amz-security-token") == "token" && normalized.headers.at("authorization").find("SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token,") != std::string::npos, "session token is signed");
    options.time.reset();
    (void)rejects<std::invalid_argument>([&] { rt::sign_v4(normalized, rt::AwsCredentials{"id", "secret", "", system_clock::now() - seconds(1)}, options); }, "expired identity");
    options.region.clear();
    (void)rejects<std::invalid_argument>([&] { rt::sign_v4(normalized, rt::AwsCredentials{"id", "secret", "", std::nullopt}, options); }, "missing region");
}

void preconfigured_credentials(Scripted& server) {
    server.script({}, reply(204, ""));
    rt::ClientConfig config;
    config.endpoint = server.url;
    config.headers = {{"Authorization", "Basic supplied"}};
    config.basic_credentials = []() -> std::optional<rt::BasicCredentials> { throw std::runtime_error("preconfigured identity invoked provider"); };
    (void)sdk::AuthClient(config).read();
    check(header(server.last(), "authorization") == "Basic supplied", "caller Authorization is sent unchanged");
    config = {};
    config.endpoint = server.url;
    config.headers = {{"X-Api-Key", "ApiKey supplied"}};
    config.api_key = []() -> std::optional<std::string> { throw std::runtime_error("preconfigured key invoked provider"); };
    (void)sdk::AuthClient(config).read();
    check(header(server.last(), "x-api-key") == "ApiKey supplied", "caller API key is sent unchanged");
    config = {};
    config.endpoint = server.url;
    config.headers = {{"Authorization", "Digest unrelated"}};
    (void)rejects<std::invalid_argument>([&] { (void)sdk::AuthClient(config).read(); }, "unrelated Authorization does not satisfy modeled schemes");
    check(server.requests().empty(), "unauthenticated request was not sent");
}

void modeled_authentication(Scripted& server) {
    server.script({}, reply(204, ""));
    std::atomic<int> calls = 0;
    rt::ClientConfig config;
    config.endpoint = server.url;
    config.basic_credentials = [&] { ++calls; return std::optional<rt::BasicCredentials>(rt::BasicCredentials{"user", "password"}); };
    (void)sdk::AuthClient(config).public_value();
    check(calls == 0 && !has_header(server.last(), "authorization"), "@auth([]) sends no identity");
    (void)sdk::AuthClient(config).read();
    check(header(server.last(), "authorization") == "Basic dXNlcjpwYXNzd29yZA==", "basic authentication");
    config.basic_credentials = [] { return std::optional<rt::BasicCredentials>(rt::BasicCredentials{"us:er", "password"}); };
    (void)rejects<std::invalid_argument>([&] { (void)sdk::AuthClient(config).read(); }, "colon in basic username");
    config.basic_credentials = nullptr;
    config.api_key = [] { return std::optional<std::string>("secret"); };
    (void)sdk::AuthClient(config).read();
    check(header(server.last(), "x-api-key") == "ApiKey secret", "API key header with scheme");
    config.api_key = nullptr;
    config.region = "us-east-1";
    config.credentials = [](const rt::CallScope&) { return std::optional<rt::AwsCredentials>(rt::AwsCredentials{"id", "secret", "", std::nullopt}); };
    config.middleware.push_back([](rt::HttpRequest request) { request.headers["X-Hook"] = "final"; return request; });
    (void)sdk::AuthClient(config).read();
    const auto signed_request = server.last();
    const auto authorization = header(signed_request, "authorization");
    check(authorization.starts_with("AWS4-HMAC-SHA256 Credential=id/") && authorization.find("/us-east-1/example/aws4_request") != std::string::npos, "SigV4 scope: " + authorization);
    check(authorization.find(";x-hook,") != std::string::npos && !header(signed_request, "x-amz-date").empty(), "SigV4 signs the final middleware headers");
    // Recompute the signature over what the server received.
    rt::HttpRequest replay{server.url + signed_request.at("path").string(), "GET", {}, std::nullopt};
    for (const auto& [name, value] : signed_request.at("headers").object()) if (name == "x-hook") replay.headers.emplace(name, value.string());
    rt::SigningOptions options;
    options.region = "us-east-1";
    options.service = "example";
    const auto stamp = header(signed_request, "x-amz-date");
    options.time = sys_days{year{std::stoi(stamp.substr(0, 4))} / std::stoi(stamp.substr(4, 2)) / std::stoi(stamp.substr(6, 2))} + hours{std::stoi(stamp.substr(9, 2))} + minutes{std::stoi(stamp.substr(11, 2))} + seconds{std::stoi(stamp.substr(13, 2))};
    rt::sign_v4(replay, rt::AwsCredentials{"id", "secret", "", std::nullopt}, options);
    check(replay.headers.at("authorization") == authorization, "server-side signature verification");
    config.credentials = nullptr;
    (void)rejects<std::invalid_argument>([&] { (void)sdk::AuthClient(config).read(); }, "required identity missing");
    (void)sdk::AuthClient(config).optional();
    check(!has_header(server.last(), "authorization"), "@optionalAuth proceeds anonymously");
    config.endpoint = "http://example.test";
    config.api_key = [] { return std::optional<std::string>("secret"); };
    check(rejects<std::invalid_argument>([&] { (void)sdk::AuthClient(config).read(); }, "plaintext credentials").find("HTTPS") != std::string::npos, "credentials require HTTPS outside loopback");
}

void credential_providers() {
    const rt::CallScope scope(10);
    const auto source = env("CREDENTIALS_FILE");
    { std::ofstream file(source); file << "[other]\naws_access_key_id = wrong\naws_secret_access_key = wrong\n[test]\naws_access_key_id = id\naws_secret_access_key = s%cret\naws_session_token = token\n"; }
    const auto chain = rt::credential_chain({[](const rt::CallScope&) { return std::optional<rt::AwsCredentials>(); }, nullptr, rt::profile_credentials("test", source)});
    const auto profile = chain(scope);
    check(profile && profile->access_key_id == "id" && profile->secret_access_key == "s%cret" && profile->session_token == "token", "profile credentials through a chain");
    check(!rt::profile_credentials("missing", source)(scope), "missing profile is unavailable");
    { std::ofstream file(source); file << "[test]\naws_access_key_id = id\n"; }
    (void)rejects<std::invalid_argument>([&] { (void)rt::profile_credentials("test", source)(scope); }, "incomplete profile");
    set_environment("AWS_ACCESS_KEY_ID", "environment-id");
    set_environment("AWS_SECRET_ACCESS_KEY", "environment-secret");
    const auto environment = rt::environment_credentials(scope);
    check(environment && environment->access_key_id == "environment-id" && environment->secret_access_key == "environment-secret", "environment credentials");

    std::atomic<int> calls = 0;
    const rt::CachedCredentials cached([&](const rt::CallScope&) {
        ++calls;
        std::this_thread::sleep_for(milliseconds(50));
        return std::optional<rt::AwsCredentials>(rt::AwsCredentials{"id", "secret", "", system_clock::now() + seconds(20)});
    }, minutes(1));
    {
        std::vector<std::jthread> threads;
        std::atomic<int> failures = 0;
        for (int index = 0; index < 30; ++index) threads.emplace_back([&] { try { if (!cached.retrieve(rt::CallScope(10))) ++failures; } catch (...) { ++failures; } });
        threads.clear();
        check(failures == 0, "concurrent cached retrievals succeed");
    }
    check(calls == 1, "one refresh in flight for concurrent callers: " + std::to_string(calls.load()));
    auto copy = cached(scope);
    copy->secret_access_key = "changed";
    check(cached(scope)->secret_access_key == "secret" && calls == 1, "cache returns independent copies");

    std::atomic<int> refreshes = 0;
    const rt::CachedCredentials expiring([&](const rt::CallScope&) { ++refreshes; return std::optional<rt::AwsCredentials>(rt::AwsCredentials{"id", "secret", "", system_clock::now() + milliseconds(400)}); }, minutes(1));
    (void)expiring(scope); (void)expiring(scope);
    check(refreshes == 1, "identity cached before its refresh point");
    std::this_thread::sleep_for(milliseconds(250));
    (void)expiring(scope);
    check(refreshes == 2, "identity refreshed before expiry");

    std::promise<void> started, release;
    auto release_future = release.get_future().share();
    const rt::CachedCredentials blocked([&](const rt::CallScope&) { started.set_value(); release_future.wait(); return std::optional<rt::AwsCredentials>(); }, minutes(1));
    std::jthread refreshing([&] { (void)blocked(rt::CallScope(10)); });
    started.get_future().wait();
    std::stop_source cancelled;
    cancelled.request_stop();
    (void)rejects<rt::CancelledError>([&] { (void)blocked(rt::CallScope(10, cancelled.get_token())); }, "waiting caller honors cancellation");
    (void)rejects<rt::DeadlineError>([&] { (void)blocked(rt::CallScope(0.05)); }, "waiting caller honors its deadline");
    release.set_value();
    refreshing.join();

    const rt::CachedCredentials expired([](const rt::CallScope&) { return std::optional<rt::AwsCredentials>(rt::AwsCredentials{"id", "secret", "", system_clock::now() - seconds(1)}); }, seconds(0));
    (void)rejects<std::runtime_error>([&] { (void)expired(scope); }, "expired identity rejected");
    (void)rejects<std::invalid_argument>([] { (void)rt::CachedCredentials(nullptr); }, "provider required");
}

void cached_client_credentials(Scripted& server) {
    server.script({}, reply(204, ""));
    std::atomic<int> calls = 0;
    rt::ClientConfig config;
    config.endpoint = server.url;
    config.region = "us-east-1";
    config.credentials = rt::CachedCredentials([&](const rt::CallScope&) { ++calls; return std::optional<rt::AwsCredentials>(rt::AwsCredentials{"cached", "secret", "", system_clock::now() + hours(1)}); });
    sdk::AuthClient client(config);
    (void)client.read(); (void)client.read();
    check(calls == 1 && header(server.last(), "authorization").find("Credential=cached/") != std::string::npos, "client uses cached credentials");
}

int main() {
    try {
        Scripted server;
        published_signing_vector();
        preconfigured_credentials(server);
        modeled_authentication(server);
        credential_providers();
        cached_client_credentials(server);
        std::cout << "C++ authentication passed\n";
    } catch (const std::exception& error) {
        std::cerr << error.what() << "\n";
        return 1;
    }
}

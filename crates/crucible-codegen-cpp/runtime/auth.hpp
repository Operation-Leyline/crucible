#pragma once
#include "transport.hpp"
#include <chrono>
#include <functional>
#include <iosfwd>
#include <memory>
#include <optional>
#include <string>
#include <vector>

namespace crucible_generated::runtime {
// Identities print redacted so logging a value never reveals its secret.
struct BasicCredentials {
    std::string username;
    std::string password;
};
std::ostream& operator<<(std::ostream& stream, const BasicCredentials& value);
struct AwsCredentials {
    std::string access_key_id;
    std::string secret_access_key;
    std::string session_token;
    std::optional<std::chrono::system_clock::time_point> expires_at;
};
std::ostream& operator<<(std::ostream& stream, const AwsCredentials& value);

// A provider returns std::nullopt when it has no identity; an exception stops resolution.
using CredentialsProvider = std::function<std::optional<AwsCredentials>(const CallScope&)>;
// Stops on the first identity or error.
[[nodiscard]] CredentialsProvider credential_chain(std::vector<CredentialsProvider> providers);
// AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, and AWS_SESSION_TOKEN.
[[nodiscard]] std::optional<AwsCredentials> environment_credentials(const CallScope& scope);
// Static shared-file credentials; never runs credential_process commands.
// Empty arguments use AWS_PROFILE (or "default") and AWS_SHARED_CREDENTIALS_FILE (or ~/.aws/credentials).
[[nodiscard]] CredentialsProvider profile_credentials(std::string profile = {}, std::string filename = {});

// Caches an identity until shortly before it expires. Concurrent callers share one refresh.
class CachedCredentials {
    struct State;
    std::shared_ptr<State> state_;
public:
    explicit CachedCredentials(CredentialsProvider provider, std::chrono::seconds refresh_before = std::chrono::minutes(5));
    [[nodiscard]] std::optional<AwsCredentials> retrieve(const CallScope& scope) const;
    std::optional<AwsCredentials> operator()(const CallScope& scope) const { return retrieve(scope); }
};

struct SigningOptions {
    std::string region;
    std::string service;
    std::optional<std::chrono::system_clock::time_point> time;
    bool unsigned_payload = false;
    std::optional<bool> disable_double_encoding;
    std::optional<bool> disable_normalize_path;
};
// Signs the final request with AWS Signature Version 4 (header form).
void sign_v4(HttpRequest& request, const AwsCredentials& credentials, const SigningOptions& options);
}

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
/// Identities print redacted so logging a value never reveals its secret.
/// Basic-auth identity; stream output redacts both fields.
struct BasicCredentials {
    /// HTTP basic-auth username.
    std::string username;
    /// HTTP basic-auth password; never log the value.
    std::string password;
};
std::ostream& operator<<(std::ostream& stream, const BasicCredentials& value);
/// AWS signing identity; stream output redacts secrets.
struct AwsCredentials {
    /// AWS access key identifier.
    std::string access_key_id;
    /// AWS secret signing key.
    std::string secret_access_key;
    /// Temporary session token, or empty for long-lived credentials.
    std::string session_token;
    /// Optional expiry used by the credential cache.
    std::optional<std::chrono::system_clock::time_point> expires_at;
};
std::ostream& operator<<(std::ostream& stream, const AwsCredentials& value);

/// A provider returns std::nullopt when it has no identity; an exception stops resolution.
using CredentialsProvider = std::function<std::optional<AwsCredentials>(const CallScope&)>;
/// Stops on the first identity or error.
[[nodiscard]] CredentialsProvider credential_chain(std::vector<CredentialsProvider> providers);
/// AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, and AWS_SESSION_TOKEN.
[[nodiscard]] std::optional<AwsCredentials> environment_credentials(const CallScope& scope);
/// Static shared-file credentials; never runs credential_process commands.
/// Empty arguments use AWS_PROFILE (or "default") and AWS_SHARED_CREDENTIALS_FILE (or ~/.aws/credentials).
[[nodiscard]] CredentialsProvider profile_credentials(std::string profile = {}, std::string filename = {});

/// Caches an identity until shortly before it expires. Concurrent callers share one refresh.
class CachedCredentials {
    struct State;
    std::shared_ptr<State> state_;
public:
    /// Constructs a shared cache with single-flight refresh.
    explicit CachedCredentials(CredentialsProvider provider, std::chrono::seconds refresh_before = std::chrono::minutes(5));
    /// Resolves cached credentials within the supplied call scope.
    [[nodiscard]] std::optional<AwsCredentials> retrieve(const CallScope& scope) const;
    std::optional<AwsCredentials> operator()(const CallScope& scope) const { return retrieve(scope); }
};

/// SigV4 signing parameters for the final HTTP request.
struct SigningOptions {
    /// Signing region.
    std::string region;
    /// Signing service name.
    std::string service;
    /// Optional signing time; absent uses the system clock.
    std::optional<std::chrono::system_clock::time_point> time;
    /// Uses UNSIGNED-PAYLOAD instead of hashing the request body.
    bool unsigned_payload = false;
    /// Overrides service-specific double encoding; absent uses the service default.
    std::optional<bool> disable_double_encoding;
    /// Overrides service-specific path normalization; absent uses the service default.
    std::optional<bool> disable_normalize_path;
};
/// Signs the final request with AWS Signature Version 4 (header form).
void sign_v4(HttpRequest& request, const AwsCredentials& credentials, const SigningOptions& options);
}

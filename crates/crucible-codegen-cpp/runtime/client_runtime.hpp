#pragma once
#include "auth.hpp"
#include "checksums.hpp"
#include "codec.hpp"
#include "endpoints.hpp"
#include "query.hpp"
#include "transport.hpp"
#include <atomic>
#include <exception>
#include <functional>
#include <set>

namespace crucible_generated::runtime {
/** Client settings. Defaults are a 30-second call deadline and three attempts. Providers and transport must be thread safe for concurrent calls. */
struct ClientConfig {
    /** HTTP endpoint override; absent uses the generated endpoint or modeled endpoint rules. */
    std::optional<std::string> endpoint;
    /** Bearer token provider invoked for each authenticated attempt. */
    std::function<std::optional<std::string>()> token;
    /** Custom HTTP transport; callbacks must respect the supplied deadline and cancellation scope. */
    std::shared_ptr<Transport> transport;
    /** Total call budget in seconds, including credentials, retries, backoff, and response reads. */
    double timeout = 30.0;
    /** Maximum attempts including the first request (default 3). */
    unsigned max_attempts = 3;
    /** Initial full-jitter exponential backoff bound in seconds. */
    double base_delay = 0.1;
    /** Maximum backoff bound in seconds; Retry-After can override it. */
    double max_delay = 10.0;
    /** Default request headers applied to every attempt. */
    Headers headers;
    /** Request transformations applied on every attempt after authentication. */
    std::vector<std::function<HttpRequest(HttpRequest)>> middleware;
    /// Modeled authentication schemes are tried in the operation's order; caller-supplied
    /// Authorization (or API key) values for a scheme are used without invoking its provider.
    /** HTTP basic authentication provider; used only for a modeled basic-auth scheme. */
    std::function<std::optional<BasicCredentials>()> basic_credentials;
    /** API key provider for the modeled header or query location. */
    std::function<std::optional<std::string>()> api_key;
    /** AWS credentials provider for modeled SigV4 authentication. */
    CredentialsProvider credentials;
    /** SigV4 region and AWS::Region endpoint built-in. */
    std::optional<std::string> region;
    /// Endpoint rule inputs: string, boolean, or string-array values.
    /** Named inputs to modeled endpoint rules. */
    Json::Object endpoint_parameters;
    /** Endpoint-rule built-in overrides. */
    Json::Object endpoint_builtins;
    /** Disables modeled operation host-prefix injection. */
    bool disable_host_prefix = false;
    /** Computes modeled request checksums when supported or only when required. */
    ChecksumPolicy request_checksum_calculation = ChecksumPolicy::when_supported;
    /** Validates modeled response checksums when supported or only when required. */
    ChecksumPolicy response_checksum_validation = ChecksumPolicy::when_supported;
    /** Notification after a response checksum validates. */
    std::function<void(const std::string& operation, const std::string& algorithm)> checksum_validated;
};
/** Optional waiter polling delay overrides in seconds. */
struct WaiterOptions {
    std::optional<double> min_delay;
    /** Maximum backoff bound in seconds; Retry-After can override it. */
    std::optional<double> max_delay;
};
/** Modeled waiter failure, carrying attempts and the last response. */
class WaiterError : public std::runtime_error {
    std::string name_;
    unsigned attempts_;
    Json response_;
    Json reason_;
    std::exception_ptr cause_;
public:
    WaiterError(std::string name, unsigned attempts, Json response, std::exception_ptr cause, Json reason, const std::string& message);
    [[nodiscard]] const std::string& name() const noexcept { return name_; }
    [[nodiscard]] unsigned attempts() const noexcept { return attempts_; }
    /// The last response in its JMESPath view (member names, epoch timestamps), or null.
    [[nodiscard]] const Json& response() const noexcept { return response_; }
    /// The modeled acceptor message, or null.
    [[nodiscard]] const Json& reason() const noexcept { return reason_; }
    [[nodiscard]] std::exception_ptr cause() const noexcept { return cause_; }
};
/** Waiter deadline exhaustion. */
class WaiterTimeoutError : public WaiterError { public: using WaiterError::WaiterError; };
using ErrorFactory = std::function<std::exception_ptr(const std::string&, const Json&, ErrorMetadata)>;
/** Shared HTTP connections, authentication, retries, and modeled error decoding. */
class ClientRuntime {
    ClientConfig config_;
    const Schema* schema_;
    ErrorFactory errors_;
    std::string endpoint_;
    std::shared_ptr<Transport> transport_;
    bool digest_;
    std::atomic<bool> closed_ = false;
public:
    /// A decoded response, and its streaming body when the operation has one.
    struct CallResult {
        Json value;
        std::optional<ByteStream> stream;
    };
private:
    struct Prepared {
        HttpRequest request;
        ChecksumSettings checksum;
    /** HTTP endpoint override; absent uses the generated endpoint or modeled endpoint rules. */
        std::optional<ResolvedEndpoint> endpoint;
    };
    [[nodiscard]] Prepared prepare(const std::string& name, const Json& operation, const Json& input, bool streaming) const;
    [[nodiscard]] std::optional<SigningOptions> authenticate(const Json& operation, HttpRequest& request, const std::optional<ResolvedEndpoint>& endpoint, const CallScope& scope, bool& credentials, std::optional<AwsCredentials>& identity) const;
    [[nodiscard]] std::optional<CallResult> wait_json(const std::string& operation, const std::string& waiter, const Json& input, double timeout, const WaiterOptions& options, std::stop_token stop, const std::function<Json(const Json&)>& view, const std::optional<ByteStream>& body) const;
    [[nodiscard]] Json success(const Json& operation, const HttpResponse& response) const;
    [[nodiscard]] Json apply_headers(const std::string& target, const Json& bindings, const HttpResponse& response, const Json& body) const;
    [[nodiscard]] std::pair<ErrorMetadata, std::exception_ptr> failure(const Json& operation, const HttpResponse& response) const;
    [[nodiscard]] double retry_delay(unsigned attempt, const std::optional<std::string>& header) const;
public:
    /** Constructs a runtime; borrows the immutable schema and retains the transport. */
    ClientRuntime(ClientConfig config, const Schema& schema, ErrorFactory errors, std::string endpoint, bool digest);
    /** Closes the runtime and its owned transport; borrowed transports stay open. */
    void close();
    [[nodiscard]] CallScope scope(std::stop_token stop = {}) const { return CallScope(config_.timeout, stop); }
    [[nodiscard]] const Schema& schema() const noexcept { return *schema_; }
    [[nodiscard]] const Json& operation(const std::string& name) const { return schema_->description().at("operations").at(name); }
    /// Sends one operation, with its streaming request body if it has one.
    [[nodiscard]] CallResult exchange(const std::string& name, const Json& input, const CallScope& scope, std::optional<ByteStream> body = {}) const;
    [[nodiscard]] Json call(const std::string& name, const Json& input, const CallScope& scope) const { return exchange(name, input, scope).value; }
    /// Polls an operation until a modeled waiter acceptor succeeds or fails, or the timeout elapses.
    template<class T> [[nodiscard]] std::optional<T> wait(const std::string& operation, const std::string& waiter, const Json& input, double timeout, const WaiterOptions& options, std::stop_token stop,
            std::optional<ByteStream> body = {}, const std::function<void(T&, std::optional<ByteStream>)>& attach = {}) const {
        const auto& output = this->operation(operation).at("output");
        (void)output;
        const auto view = [&](const Json& value) -> Json {
            if constexpr (std::is_same_v<T, Unit>) return value;
            else { const auto context = schema_->context(output.string()); return Codec<T>::encode(Codec<T>::decode(value, context), context); }
        };
        auto result = wait_json(operation, waiter, input, timeout, options, stop, view, body);
        if (!result) return std::nullopt;
        if constexpr (std::is_same_v<T, Unit>) return T{};
        else {
            auto decoded = Codec<T>::decode(result->value, schema_->context(output.string()));
            if (attach) attach(decoded, std::move(result->stream));
            return decoded;
        }
    }
};
/** Lazy page iterator. Consume from one thread; next returns nullopt on exhaustion. */
template<class T> class Pages {
    std::shared_ptr<ClientRuntime> runtime_;
    std::string operation_;
    Json input_;
    std::stop_token stop_;
    std::set<std::string> seen_;
    bool finished_ = false;
    std::optional<std::string> next_;
public:
    Pages(std::shared_ptr<ClientRuntime> runtime, std::string operation, Json input, std::stop_token stop)
        : runtime_(std::move(runtime)), operation_(std::move(operation)), input_(std::move(input)), stop_(stop) {
        const auto& pagination = runtime_->operation(operation_).at("pagination");
    /** Bearer token provider invoked for each authenticated attempt. */
        const auto* token = input_.find(pagination.at("input").string());
        if (token && !token->is_null() && !token->string().empty()) seen_.insert(token->string());
    }
    /** Fetches the next page; detects repeated continuation tokens. */
    [[nodiscard]] std::optional<T> next() {
        if (finished_) return std::nullopt;
        const auto& operation = runtime_->operation(operation_);
        const auto& pagination = operation.at("pagination");
        if (next_) {
            if (!seen_.insert(*next_).second) throw std::invalid_argument("Pagination token cycle");
            auto object = input_.object();
            object.insert_or_assign(pagination.at("input").string(), Json(*next_));
            input_ = Json(std::move(object));
            next_.reset();
        }
        const auto scope = runtime_->scope(stop_);
        scope.check();
        const auto result = runtime_->call(operation_, input_, scope);
        auto output = Codec<T>::decode(result, runtime_->schema().context(operation.at("output").string()));
    /** Bearer token provider invoked for each authenticated attempt. */
        const Json* token = &result;
        for (const auto& path : pagination.at("output").array()) {
    /** Bearer token provider invoked for each authenticated attempt. */
            if (!token || !std::holds_alternative<Json::Object>(token->value())) { token = nullptr; break; }
    /** Bearer token provider invoked for each authenticated attempt. */
            token = token->find(path.string());
        }
        if (!token || token->is_null() || token->string().empty()) finished_ = true;
        else next_ = token->string();
        scope.check();
        return output;
    }
};
}

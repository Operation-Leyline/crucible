#pragma once
#include "codec.hpp"
#include "transport.hpp"
#include <atomic>
#include <exception>
#include <functional>
#include <set>

namespace crucible_generated::runtime {
/** Client settings. Defaults are a 30-second call deadline and three attempts. Providers and transport must be thread safe for concurrent calls. */
struct ClientConfig {
    /** HTTP endpoint override; absent uses the generated endpoint. */
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
};
using ErrorFactory = std::function<std::exception_ptr(const std::string&, const Json&, ErrorMetadata)>;
/** Shared HTTP connections, authentication, retries, and modeled error decoding. */
class ClientRuntime {
    ClientConfig config_;
    const Schema* schema_;
    ErrorFactory errors_;
    std::string endpoint_;
    std::shared_ptr<Transport> transport_;
    std::atomic<bool> closed_ = false;
    [[nodiscard]] HttpRequest prepare(const Json& operation, const Json& input) const;
    /// Sets the bearer token when the operation takes one; returns whether the request carries an identity.
    [[nodiscard]] bool authenticate(const Json& operation, HttpRequest& request, const CallScope& scope) const;
    [[nodiscard]] Json success(const Json& operation, const HttpResponse& response) const;
    [[nodiscard]] Json apply_headers(const std::string& target, const Json& bindings, const HttpResponse& response, const Json& body) const;
    [[nodiscard]] std::pair<ErrorMetadata, std::exception_ptr> failure(const Json& operation, const HttpResponse& response) const;
    [[nodiscard]] double retry_delay(unsigned attempt, const std::optional<std::string>& header) const;
public:
    /** Constructs a runtime; borrows the immutable schema and retains the transport. */
    ClientRuntime(ClientConfig config, const Schema& schema, ErrorFactory errors, std::string endpoint);
    /** Closes the runtime and its owned transport; borrowed transports stay open. */
    void close();
    [[nodiscard]] CallScope scope(std::stop_token stop = {}) const { return CallScope(config_.timeout, stop); }
    [[nodiscard]] const Schema& schema() const noexcept { return *schema_; }
    [[nodiscard]] const Json& operation(const std::string& name) const { return schema_->description().at("operations").at(name); }
    /// Sends one operation, retrying within the scope's deadline, and returns its decoded response.
    [[nodiscard]] Json call(const std::string& name, const Json& input, const CallScope& scope) const;
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
        const Json* token = &result;
        for (const auto& path : pagination.at("output").array()) {
            if (!token || !std::holds_alternative<Json::Object>(token->value())) { token = nullptr; break; }
            token = token->find(path.string());
        }
        if (!token || token->is_null() || token->string().empty()) finished_ = true;
        else next_ = token->string();
        scope.check();
        return output;
    }
};
}

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
struct ClientConfig {
    std::optional<std::string> endpoint;
    std::function<std::optional<std::string>()> token;
    std::shared_ptr<Transport> transport;
    double timeout = 30.0;
    unsigned max_attempts = 3;
    double base_delay = 0.1;
    double max_delay = 10.0;
    Headers headers;
    std::vector<std::function<HttpRequest(HttpRequest)>> middleware;
    // Modeled authentication schemes are tried in the operation's order; caller-supplied
    // Authorization (or API key) values for a scheme are used without invoking its provider.
    std::function<std::optional<BasicCredentials>()> basic_credentials;
    std::function<std::optional<std::string>()> api_key;
    CredentialsProvider credentials;
    std::optional<std::string> region;
    // Endpoint rule inputs: string, boolean, or string-array values.
    Json::Object endpoint_parameters;
    Json::Object endpoint_builtins;
    bool disable_host_prefix = false;
    ChecksumPolicy request_checksum_calculation = ChecksumPolicy::when_supported;
    ChecksumPolicy response_checksum_validation = ChecksumPolicy::when_supported;
    std::function<void(const std::string& operation, const std::string& algorithm)> checksum_validated;
};
struct WaiterOptions {
    std::optional<double> min_delay;
    std::optional<double> max_delay;
};
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
    // The last response in its JMESPath view (member names, epoch timestamps), or null.
    [[nodiscard]] const Json& response() const noexcept { return response_; }
    // The modeled acceptor message, or null.
    [[nodiscard]] const Json& reason() const noexcept { return reason_; }
    [[nodiscard]] std::exception_ptr cause() const noexcept { return cause_; }
};
class WaiterTimeoutError : public WaiterError { public: using WaiterError::WaiterError; };
using ErrorFactory = std::function<std::exception_ptr(const std::string&, const Json&, ErrorMetadata)>;
class ClientRuntime {
    ClientConfig config_;
    const Schema* schema_;
    ErrorFactory errors_;
    std::string endpoint_;
    std::shared_ptr<Transport> transport_;
    bool digest_;
    std::atomic<bool> closed_ = false;
    struct Prepared {
        HttpRequest request;
        ChecksumSettings checksum;
        std::optional<ResolvedEndpoint> endpoint;
    };
    [[nodiscard]] Prepared prepare(const std::string& name, const Json& operation, const Json& input) const;
    [[nodiscard]] std::optional<SigningOptions> authenticate(const Json& operation, HttpRequest& request, const std::optional<ResolvedEndpoint>& endpoint, const CallScope& scope, bool& credentials, std::optional<AwsCredentials>& identity) const;
    [[nodiscard]] std::optional<Json> wait_json(const std::string& operation, const std::string& waiter, const Json& input, double timeout, const WaiterOptions& options, std::stop_token stop, const std::function<Json(const Json&)>& view) const;
    [[nodiscard]] Json success(const Json& operation, const HttpResponse& response) const;
    [[nodiscard]] Json apply_headers(const std::string& target, const Json& bindings, const HttpResponse& response, const Json& body) const;
    [[nodiscard]] std::pair<ErrorMetadata, std::exception_ptr> failure(const Json& operation, const HttpResponse& response) const;
    [[nodiscard]] double retry_delay(unsigned attempt, const std::optional<std::string>& header) const;
public:
    ClientRuntime(ClientConfig config, const Schema& schema, ErrorFactory errors, std::string endpoint, bool digest);
    void close();
    [[nodiscard]] CallScope scope(std::stop_token stop = {}) const { return CallScope(config_.timeout, stop); }
    [[nodiscard]] const Schema& schema() const noexcept { return *schema_; }
    [[nodiscard]] const Json& operation(const std::string& name) const { return schema_->description().at("operations").at(name); }
    [[nodiscard]] Json call(const std::string& name, const Json& input, const CallScope& scope) const;
    // Polls an operation until a modeled waiter acceptor succeeds or fails, or the timeout elapses.
    template<class T> [[nodiscard]] std::optional<T> wait(const std::string& operation, const std::string& waiter, const Json& input, double timeout, const WaiterOptions& options, std::stop_token stop) const {
        const auto& output = this->operation(operation).at("output");
        (void)output;
        const auto view = [&](const Json& value) -> Json {
            if constexpr (std::is_same_v<T, Unit>) return value;
            else { const auto context = schema_->context(output.string()); return Codec<T>::encode(Codec<T>::decode(value, context), context); }
        };
        const auto result = wait_json(operation, waiter, input, timeout, options, stop, view);
        if (!result) return std::nullopt;
        if constexpr (std::is_same_v<T, Unit>) return T{};
        else return Codec<T>::decode(*result, schema_->context(output.string()));
    }
};
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

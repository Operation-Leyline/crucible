#include "jobs_client/client.hpp"
#include "jobs_client/schema.hpp"
#define SCRIPTED_RUNTIME jobs_sdk::runtime
#include "scripted.hpp"

namespace sdk = jobs_sdk;
using sdk::models::Input;

// Every conjunct of the Functions waiter must hold on its own, so a regression names the function.
void conjuncts(const rt::Json& node, const rt::Json& output, std::vector<std::string>& failures) {
    if (node.at("kind").string() == "and") { conjuncts(node.at("left"), output, failures); conjuncts(node.at("right"), output, failures); return; }
    try { const auto value = rt::query(node, output); if (!std::holds_alternative<bool>(value.value()) || !value.boolean()) failures.push_back(node.dump() + " => " + value.dump()); }
    catch (const std::exception& error) { failures.push_back(node.dump() + " threw " + error.what()); }
}

int main() {
    try {
        const auto& functions = sdk::schema().description().at("operations").at("Read").at("waiters").at("Functions").at("acceptors").array()[0].at("matcher").at("path").at("expression");
        std::vector<std::string> failures;
        conjuncts(functions, rt::Json::parse(R"({"items":[{"status":"READY"},{"status":"READY"}],"updatedAt":1767225600})"), failures);
        for (const auto& failure : failures) std::cerr << failure << "\n";
        check(failures.empty(), "JMESPath standard functions");
        Scripted server;
        rt::ClientConfig config;
        config.endpoint = server.url;
        config.max_attempts = 1;
        const sdk::JobsClient client(config);
        const Input input(std::vector<std::string>{"a", "b"});
        const rt::WaiterOptions immediate{0.0, 0.0};
        const auto missing = reply(404, R"({"__type":"Missing"})");
        const auto pending = reply(200, R"({"wire-status":"PENDING"})");

        server.script({missing, pending, reply(200, R"({"wire-status":"READY"})")});
        const auto ready = client.wait_until_ready(input, 5, immediate);
        check(ready && ready->status && *ready->status == "READY", "Ready waiter output");
        const auto attempts = server.requests();
        check(attempts.size() == 3 && rt::Json::parse(body(attempts[0])).at("names").array().size() == 2, "retry acceptor and pending retries");

        server.script({pending, reply(200, R"({"wire-status":"READY"})", {}, 0.1)});
        const auto final_poll = client.wait_until_ready(input, 1.5, rt::WaiterOptions{1.0, 1.0});
        check(final_poll && final_poll->status && *final_poll->status == "READY", "final poll has time to decode a delayed response");
        check(server.requests().size() == 2, "nonzero delays reserve a final poll before the deadline");

        server.script({missing});
        check(!client.wait_until_deleted(input, 5, immediate), "errorType success returns no output");

        const auto query = reply(200, R"({"items":[{"wire-status":"READY"},{"wire-status":"READY"}],"updatedAt":"2026-01-01T00:00:00Z"})");
        server.script({}, query);
        check(client.wait_until_all_ready(input, 5, immediate).has_value(), "allStringEquals");
        check(client.wait_until_any_ready(input, 5, immediate).has_value(), "anyStringEquals");
        check(client.wait_until_filtered_ready(input, 5, immediate).has_value(), "filter projection");
        check(client.wait_until_counts_match(input, 5, immediate).has_value(), "inputOutput matcher");
        check(client.wait_until_updated(input, 5, immediate).has_value(), "timestamp comparison");
        check(client.wait_until_functions(input, 5, immediate).has_value(), "JMESPath functions");
        check(server.requests().size() == 6, "query waiters succeed on the first attempt");

        server.script({reply(200, R"({"wire-status":"FAILED","reasons":["job rejected"]})")});
        try {
            (void)client.wait_until_ready(input, 5, immediate);
            throw std::runtime_error("failure acceptor ignored");
        } catch (const rt::WaiterTimeoutError&) {
            throw std::runtime_error("failure reported as timeout");
        } catch (const rt::WaiterError& error) {
            check(error.attempts() == 1 && std::string(error.what()).find("job rejected") != std::string::npos && error.reason().array().size() == 1, std::string("failure acceptor: ") + error.what());
            check(error.response().at("status").string() == "FAILED", "failure response view");
        }

        server.script({reply(500, R"({"__type":"Broken"})")});
        try { (void)client.wait_until_ready(input, 5, immediate); throw std::runtime_error("unmatched error ignored"); }
        catch (const rt::WaiterError& error) {
            bool service = false;
            try { std::rethrow_exception(error.cause()); } catch (const rt::ServiceError& cause) { service = cause.metadata().name == "Broken"; }
            check(service, "unmatched errors fail with their cause");
        }

        server.script({}, pending);
        auto start = std::chrono::steady_clock::now();
        try { (void)client.wait_until_ready(input, 0.3); throw std::runtime_error("pending waiter finished"); }
        catch (const rt::WaiterTimeoutError& error) { check(error.attempts() >= 1, "timeout attempts"); }
        const auto elapsed = std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
        check(elapsed >= 0.25 && elapsed < 2, "modeled delays are capped by the waiter deadline");

        std::stop_source stopped;
        stopped.request_stop();
        (void)rejects<rt::CancelledError>([&] { (void)client.wait_until_ready(input, 5, {}, stopped.get_token()); }, "cancelled waiter");

        server.script({}, reply(200, R"({"wire-status":"PENDING"})", {}, 1.0));
        start = std::chrono::steady_clock::now();
        (void)rejects<rt::WaiterTimeoutError>([&] { (void)client.wait_until_ready(input, 0.1); }, "blocked request times out");
        check(std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count() < 0.9, "in-flight request bounded by waiter deadline");

        (void)rejects<std::invalid_argument>([&] { (void)client.wait_until_ready(input, 5, rt::WaiterOptions{2.0, 1.0}); }, "unordered delays");
        (void)rejects<std::invalid_argument>([&] { (void)client.wait_until_ready(input, 0); }, "nonpositive timeout");

        // Direct JMESPath checks that the waiter fixtures do not cover.
        const auto expression = [](const char* text) { return rt::Json::parse(text); };
        check(rt::query(expression(R"({"kind":"slice","start":null,"stop":null,"step":-1})"), rt::Json::parse("[1,2,3]")).dump() == "[3,2,1]", "reverse slice");
        check(rt::query(expression(R"({"kind":"function","function":"avg","arguments":[{"kind":"literal","value":[1,2,2]}]})"), rt::Json()).number().text() == "1." + std::string(33, '6') + "7", "avg precision");
        check(rt::query(expression(R"({"kind":"function","function":"sum","arguments":[{"kind":"literal","value":[0.1,0.2,-0.3]}]})"), rt::Json()).dump() == "0", "exact decimal sum");
        (void)rejects<rt::QueryError>([&] { (void)rt::query(expression(R"({"kind":"function","function":"length","arguments":[{"kind":"literal","value":1}]})"), rt::Json()); }, "invalid argument type");
        std::cout << "C++ waiters passed\n";
    } catch (const std::exception& error) {
        std::cerr << error.what() << "\n";
        return 1;
    }
}

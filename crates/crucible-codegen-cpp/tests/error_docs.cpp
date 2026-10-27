#include "docs_client/client.hpp"
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <iterator>
#include <typeinfo>

using namespace docs;
using namespace docs::runtime;
void check(bool condition, const std::string& label) { if (!condition) throw std::runtime_error("Error documentation: " + label); }
int main() {
    const char* cases_path = std::getenv("ERROR_DOCS_CASES");
    const char* endpoint = std::getenv("ERROR_DOCS_URL");
    check(cases_path && *cases_path, "ERROR_DOCS_CASES must be set");
    check(endpoint && *endpoint, "ERROR_DOCS_URL must be set");
    std::ifstream input(cases_path);
    check(input.is_open(), "cannot read ERROR_DOCS_CASES");
    const auto oracle = Json::parse(std::string(std::istreambuf_iterator<char>(input), {}));
    for (const auto& test : oracle.at("cases").array()) {
        const auto& label = test.at("name").string();
        std::exception_ptr saved;
        {
            ClientConfig config;
            config.endpoint = endpoint; config.max_attempts = 1;
            TroubleshootingClient client(config);
            try { (void)client.inspect(); }
            catch (const TroubleshootingException&) { saved = std::current_exception(); }
            client.close();
        }
        check(saved != nullptr, label);
        try { std::rethrow_exception(saved); }
        catch (const TroubleshootingException& error) {
            const auto copied = error;
            const auto expected = test.at("docUrl").is_null() ? std::nullopt : std::optional<std::string>(test.at("docUrl").string());
            check(copied.doc_url() == expected, label);
            check(copied.metadata().doc_url == expected, label);
            if (!test.at("typed").is_null()) check((typeid(error) != typeid(TroubleshootingException)) == test.at("typed").boolean(), label);
            if (label == "default-known") check(dynamic_cast<const models::DefaultReason&>(error).detail.value == "access/needed", label);
            if (label == "default-empty") check(dynamic_cast<const models::DefaultEmptyReason&>(error).detail.value.empty(), label);
            if (label == "mapped-member") check(dynamic_cast<const models::MappedReasonError&>(error).failure_reason->value == "access/needed", label);
            if (label == "document-empty" || label == "document-whitespace" || label == "document-null") {
                const auto& payload = dynamic_cast<const models::DocumentPayloadError&>(error).data;
                check(payload.is_set() == (label == "document-null") && payload.is_null() == (label == "document-null"), label);
            }
        }
    }
    std::cout << "C++ installed error-documentation HTTP and copied exception lifetime passed\n";
}

#pragma once
#include "json.hpp"
#include <map>
#include <optional>
#include <stdexcept>
#include <string>
#include <vector>

namespace crucible_generated::runtime {
class EndpointError : public std::invalid_argument { public: using std::invalid_argument::invalid_argument; };
struct ResolvedEndpoint {
    std::string url;
    std::map<std::string, std::vector<std::string>> headers;
    Json::Object properties;
};
// Smithy rules-engine standard functions over JSON values; null means absent.
[[nodiscard]] Json endpoint_function(const std::string& name, const Json::Array& arguments);
// parseUrl: scheme, authority, path, normalizedPath, and isIp; null for unusable endpoints.
[[nodiscard]] Json parse_endpoint(const std::string& url);
[[nodiscard]] bool valid_host_label(const std::string& text, bool subdomains);
// Resolves an endpoint rule set from operation parameter sources, client parameters, and built-ins.
[[nodiscard]] ResolvedEndpoint resolve_endpoint(const Json& rules, const Json* sources, const Json& input, const Json::Object& client, const Json::Object& builtins);
// Applies an operation's @endpoint hostPrefix to an endpoint URL.
[[nodiscard]] std::string prefix_endpoint(const std::string& endpoint, const Json& description, const Json& input);
// Endpoint parameter values are strings, booleans, or string arrays.
void validate_endpoint_parameters(const Json::Object& values);
}

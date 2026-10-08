#pragma once
#include "codec.hpp"

namespace crucible_generated::runtime {
class QueryError : public std::invalid_argument { public: using std::invalid_argument::invalid_argument; };
/// Evaluates a JMESPath expression already compiled by crucible-smithy into a portable tree.
[[nodiscard]] Json query(const Json& expression, const Json& input);
/// The JMESPath view of an encoded model: member names, timestamps as epoch seconds, blobs as base64.
[[nodiscard]] Json query_model(const Schema& schema, std::string_view target, const Json& value);
}

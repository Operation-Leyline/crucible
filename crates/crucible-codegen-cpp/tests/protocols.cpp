// Compiled once per AWS JSON protocol: PROTOCOL_HEADER, PROTOCOL_NAMESPACE, and PROTOCOL_VERSION ("rest", "1.0", "1.1").
#include PROTOCOL_HEADER
#define SCRIPTED_RUNTIME PROTOCOL_NAMESPACE::runtime
#include "scripted.hpp"
#include <cmath>
#include <limits>

namespace sdk = PROTOCOL_NAMESPACE;
using sdk::models::ValuesInput;

int main() {
    try {
        const std::string version = PROTOCOL_VERSION;
        const bool rpc = version != "rest";
        Scripted server;
        rt::ClientConfig config;
        config.endpoint = server.url;
        config.max_attempts = 1;
        const sdk::JsonServiceClient client(config);
        const rt::Timestamp instant(-1, 750000000);

        ValuesInput input;
        input.renamed = "hello";
        input.when = instant; input.iso = instant; input.date = instant;
        input.dates = std::vector<rt::Timestamp>{instant};
        input.query = instant;
        input.float_value = std::numeric_limits<float>::quiet_NaN();
        input.double_value = std::numeric_limits<double>::infinity();
        input.nullable = rt::Field<std::string>::null();
        input.document = rt::Field<rt::Json>::null();
        const std::string renamed = rpc ? "renamed" : "wire-name";
        rt::Json::Object headers;
        if (!rpc) { headers.emplace("x-date", rt::Json("Wed, 31 Dec 1969 23:59:59 GMT")); headers.emplace("x-dates", rt::Json("Wed, 31 Dec 1969 23:59:59 GMT, Thu, 01 Jan 1970 00:00:01 GMT")); }
        server.script({reply(200, R"({")" + renamed + R"(":"hello","when":-0.25,"iso":"1969-12-31T23:59:59.75Z","choice":{"text":"ok","__type":"ignored"},"nullable":null,"document":null,"floatValue":"NaN","doubleValue":"-Infinity")" + (rpc ? R"(,"date":-0.25,"dates":[-0.25])" : "") + "}", headers)});
        const auto output = client.echo(input);
        const auto request = server.last();
        const auto sent = rt::Json::parse(body(request));
        check(request.at("method").string() == "POST", "POST");
        check(sent.at("when").number().canonical() == rt::Number("-0.25").canonical(), "epoch-seconds body timestamp");
        check(rt::Timestamp::parse(sent.at("iso").string()) == instant, "date-time body timestamp");
        check(!sent.find("nullable") && sent.at("document").is_null(), "null structure member omitted but null document kept");
        check(sent.at("floatValue").string() == "NaN" && sent.at("doubleValue").string() == "Infinity", "non-finite floats as strings");
        check(sent.at(renamed).string() == "hello", "JSON member name");
        if (rpc) {
            check(request.at("path").string() == "/" && header(request, "x-amz-target") == "JsonService.Echo", "RPC target");
            check(header(request, "content-type") == "application/x-amz-json-" + version, "RPC content type");
            check(sent.at("date").number().canonical() == rt::Number("-0.25").canonical() && sent.at("dates").array().size() == 1, "RPC timestamps stay in the body");
        } else {
            const auto& path = request.at("path").string();
            check(path.starts_with("/echo?time=") && rt::Timestamp::parse(rt::uri_decode(path.substr(11))) == instant, "REST date-time query: " + path);
            check(header(request, "x-date") == "Wed, 31 Dec 1969 23:59:59 GMT", "REST http-date header");
            check(!sent.find("date") && !has_header(request, "x-amz-target"), "REST bindings stay out of the body");
            check(output.dates && output.dates->size() == 2 && (*output.dates)[1] == rt::Timestamp(1), "unquoted HTTP-date list header");
        }
        // HTTP dates carry whole seconds; the RPC body keeps the fraction.
        check(output.date && *output.date == (rpc ? instant : rt::Timestamp(-1)), "decoded bound timestamp");
        check(output.when == instant && output.iso == instant, "decoded body timestamps");
        check(output.choice && std::get<sdk::models::ChoiceTextVariant>(output.choice->value).value == "ok", "union ignores __type");
        check(!output.nullable.is_set() && output.document.is_null(), "AWS JSON null handling");
        check(std::isnan(*output.float_value) && *output.double_value == -std::numeric_limits<double>::infinity(), "decoded non-finite floats");

        const std::vector<std::pair<std::string, std::string>> discriminators{{"x-amzn-errortype", "example.json#Problem:http://internal"}, {"code", "Problem"}, {"__type", "example.json#Problem"}};
        for (const auto& discriminator : discriminators) {
            rt::Json::Object error_headers{{"x-amzn-requestid", rt::Json("request-42")}};
            std::string error_body = R"({"Message":"bad")";
            if (discriminator.first.starts_with("x-")) { error_headers.emplace(discriminator.first, rt::Json(discriminator.second)); error_body += R"(,"__type":"Other")"; }
            else error_body += ",\"" + discriminator.first + "\":\"" + discriminator.second + "\"";
            server.script({reply(409, error_body + "}", error_headers)});
            try { (void)client.echo(ValuesInput()); throw std::runtime_error("modeled error not raised"); }
            catch (const sdk::models::Problem& error) {
                check(error.metadata().name == "Problem" && error.metadata().request_id == "request-42" && error.metadata().message == "bad", "AWS error metadata via " + discriminator.first);
            }
        }

        server.script({reply(200, "")});
        (void)client.empty();
        const auto empty = server.last();
        check(rpc ? body(empty) == "{}" && header(empty, "x-amz-target") == "JsonService.Empty" : body(empty).empty() && empty.at("path").string() == "/empty", "empty operation request");
        std::cout << "C++ AWS JSON protocol " << version << " passed\n";
    } catch (const std::exception& error) {
        std::cerr << error.what() << "\n";
        return 1;
    }
}

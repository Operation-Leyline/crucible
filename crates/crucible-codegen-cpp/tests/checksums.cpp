#include "checksum_client/client.hpp"
#define SCRIPTED_RUNTIME checksum_sdk::runtime
#include "scripted.hpp"
#include <cctype>

namespace sdk = checksum_sdk;
using sdk::models::Algorithm;
using sdk::models::Input;

std::string lower(std::string value) { for (auto& c : value) c = static_cast<char>(std::tolower(static_cast<unsigned char>(c))); return value; }

int main() {
    try {
        const auto vectors = rt::Json::parse(read_file(env("CHECKSUM_CASES")));
        const auto& payload = vectors.at("inputUtf8").string();
        const auto& expected = vectors.at("base64");
        for (const auto& [algorithm, value] : expected.object()) check(rt::checksum(algorithm, payload) == value.string(), "published " + algorithm + " vector");
        check(rt::checksum("MD5", "") == vectors.at("emptyMd5").string(), "empty MD5 vector");
        check(rt::checksum("CRC32", "") == "AAAAAA==" && rt::checksum("CRC64NVME", "") == "AAAAAAAAAAA=", "empty CRC vectors");
        (void)rejects<std::invalid_argument>([] { (void)rt::checksum("XXHASH64", "x"); }, "unsupported algorithm");

        Scripted server;
        std::vector<std::string> validated;
        rt::ClientConfig config;
        config.endpoint = server.url;
        config.checksum_validated = [&](const std::string& operation, const std::string& algorithm) { validated.push_back(operation + ":" + algorithm); };
        const sdk::ChecksumsClient client(config);
        const auto respond = [&](const std::string& algorithm, const std::string& value) { return reply(200, payload, {{"x-amz-checksum-" + lower(algorithm), rt::Json(value)}, {"x-amz-checksum-xxhash3", rt::Json("unmodeled")}}); };
        Input input;
        input.data = blob(payload);

        server.script({}, respond("CRC32", expected.at("CRC32").string()));
        (void)client.legacy(input);
        check(header(server.last(), "content-md5") == expected.at("MD5").string(), "httpChecksumRequired sends Content-MD5");
        (void)client.required(input);
        check(header(server.last(), "content-md5") == expected.at("MD5").string(), "requestChecksumRequired without algorithm uses MD5");
        (void)client.legacy(Input());
        check(header(server.last(), "content-md5") == vectors.at("emptyMd5").string(), "empty body checksum");
        check(validated.empty(), "operations without validation members do not validate");
        const auto output = client.flexible(input);
        const auto flexible = server.last();
        check(header(flexible, "x-algorithm") == "CRC32" && header(flexible, "x-amz-checksum-crc32") == expected.at("CRC32").string(), "default CRC32 request checksum");
        check(header(flexible, "x-validation") == "ENABLED" && !input.algorithm.is_set() && !input.validation.is_set(), "defaults are private to the request");
        check(validated.back() == "Flexible:CRC32" && text(*output.data) == payload, "response checksum validated");

        for (const auto& [algorithm, value] : expected.object()) {
            Input selected(blob(payload), Algorithm(algorithm));
            server.script({}, respond(algorithm, value.string()));
            (void)client.flexible(selected);
            check(header(server.last(), "x-amz-checksum-" + lower(algorithm)) == value.string(), "request " + algorithm);
            check(validated.back() == "Flexible:" + algorithm, "response " + algorithm);
        }

        server.script({}, respond("CRC32", "AAAA"));
        try { (void)client.flexible(input); throw std::runtime_error("corrupt response accepted"); }
        catch (const rt::ChecksumError& error) { check(error.algorithm() == "CRC32", "mismatch algorithm"); }

        server.script({}, reply(200, payload, {{"x-amz-checksum-crc32", rt::Json(expected.at("CRC32").string())}, {"x-amz-checksum-sha256", rt::Json("AAAA")}}));
        (void)client.flexible(input);
        check(validated.back() == "Flexible:CRC32", "first modeled response algorithm wins");

        server.script({}, reply(200, payload, {{"x-amz-checksum-xxhash3", rt::Json("unmodeled")}}));
        const auto count = validated.size();
        (void)client.flexible(input);
        check(validated.size() == count, "unmodeled checksums are not validated");

        Input supplied(blob(payload));
        supplied.checksum = "supplied";
        (void)client.flexible(supplied);
        check(header(server.last(), "x-amz-checksum-crc32") == "supplied", "caller checksum header is preserved");

        auto optout = config;
        optout.request_checksum_calculation = rt::ChecksumPolicy::when_required;
        optout.response_checksum_validation = rt::ChecksumPolicy::when_required;
        (void)sdk::ChecksumsClient(optout).flexible(input);
        const auto skipped = server.last();
        check(!has_header(skipped, "x-algorithm") && !has_header(skipped, "x-validation") && !has_header(skipped, "x-amz-checksum-crc32"), "WHEN_REQUIRED skips optional checksums");
        (void)sdk::ChecksumsClient(optout).required(input);
        check(header(server.last(), "content-md5") == expected.at("MD5").string(), "required checksums ignore WHEN_REQUIRED");

        (void)rejects<std::invalid_argument>([&] { (void)client.flexible(Input({}, Algorithm("XXHASH64"))); }, "unsupported request algorithm");
        check(server.requests().empty(), "unsupported algorithm is rejected before dispatch");

        auto hooked = config;
        hooked.middleware.push_back([&](rt::HttpRequest request) { request.body = payload; return request; });
        (void)sdk::ChecksumsClient(hooked).legacy(Input());
        check(header(server.last(), "content-md5") == expected.at("MD5").string(), "checksum covers the final middleware body");
        std::cout << "C++ HTTP checksums passed\n";
    } catch (const std::exception& error) {
        std::cerr << error.what() << "\n";
        return 1;
    }
}

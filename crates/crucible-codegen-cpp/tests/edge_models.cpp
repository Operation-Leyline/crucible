#ifdef _WIN32
#include <Windows.h>
#endif
#include "edge_client/client.hpp"
#include "edge_client/schema.hpp"
#include "empty_client/client.hpp"
#include <iostream>

int main() {
    namespace rt = edge::runtime;
    using edge::models::CreateCityInput;
    const auto text = std::string("literal\\u0001") + char(0) + "é";
    if (edge::models::Time::known_clear().value != text) throw std::runtime_error("Enum literal");
    if (edge::schema().shape("example.weather#Sky").at("values").array()[0].string() != text) throw std::runtime_error("Embedded schema escapes");
    const auto context = edge::schema().context("example.weather#CreateCityInput");
    const auto value = rt::Json::parse(R"({"node":{"children":[{"children":[]}]}})");
    const auto model = rt::Codec<CreateCityInput>::decode(value, context);
    auto copy = model;
    copy.node.value().children.value()[0].value().children.value().clear();
    copy.node.value().children.value().clear();
    if (model.node.value().children.value().size() != 1) throw std::runtime_error("Recursive copies alias");
    if (rt::Codec<CreateCityInput>::encode(model, context).dump() != value.dump()) throw std::runtime_error("Recursive codec");
    empty::runtime::ClientConfig config;
    config.endpoint = "http://127.0.0.1";
    empty::EmptyClient client(config);
    client.close();
    std::cout << "C++ recursive, renamed, empty, and escaped models passed\n";
}

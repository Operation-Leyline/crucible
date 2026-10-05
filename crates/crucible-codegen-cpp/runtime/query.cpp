#include "query.hpp"
#include <algorithm>
#include <charconv>
#include <numeric>

namespace crucible_generated::runtime {
namespace {
[[noreturn]] void fail(const std::string& message) { throw QueryError(message); }
bool is(const Json& value, std::size_t index) { return value.value().index() == index; }
bool number(const Json& value) { return std::holds_alternative<Number>(value.value()); }
bool text(const Json& value) { return std::holds_alternative<std::string>(value.value()); }
bool list(const Json& value) { return std::holds_alternative<Json::Array>(value.value()); }
bool object(const Json& value) { return std::holds_alternative<Json::Object>(value.value()); }
const std::string& qtext(const Json& value) { if (!text(value)) fail("JMESPath expected string"); return value.string(); }
const Json::Array& qarray(const Json& value) { if (!list(value)) fail("JMESPath expected array"); return value.array(); }
const Json::Object& qobject(const Json& value) { if (!object(value)) fail("JMESPath expected object"); return value.object(); }
const Number& qnumber(const Json& value) { if (!number(value)) fail("JMESPath expected number"); return value.number(); }
long long qinteger(const Json& value) { try { return qnumber(value).integer(); } catch (const QueryError&) { throw; } catch (const std::invalid_argument&) { fail("JMESPath integer out of range"); } }
bool truth(const Json& value) {
    if (value.is_null()) return false;
    if (is(value, 1)) return value.boolean();
    if (text(value)) return !value.string().empty();
    if (list(value)) return !value.array().empty();
    if (object(value)) return !value.object().empty();
    return true;
}
long long exponent(const std::string& text) {
    long long result = 0;
    const auto parsed = std::from_chars(text.data(), text.data() + text.size(), result);
    if (parsed.ec != std::errc() || parsed.ptr != text.data() + text.size() || result > 1000000 || result < -1000000) fail("JMESPath number exponent exceeds supported precision");
    return result;
}
int compare_numbers(const Number& left, const Number& right) {
    auto [a, ae] = left.canonical();
    auto [b, be] = right.canonical();
    const auto sign = [](const std::string& value) { return value == "0" ? 0 : value.front() == '-' ? -1 : 1; };
    const int as = sign(a), bs = sign(b);
    if (as != bs) return as < bs ? -1 : 1;
    if (as == 0) return 0;
    if (as < 0) { a.erase(0, 1); b.erase(0, 1); }
    const auto ao = exponent(ae) + static_cast<long long>(a.size()), bo = exponent(be) + static_cast<long long>(b.size());
    if (ao != bo) return ao < bo ? -as : as;
    const auto size = (std::max)(a.size(), b.size());
    a.resize(size, '0'); b.resize(size, '0');
    const int order = a.compare(b);
    return order == 0 ? 0 : (order < 0 ? -as : as);
}
int compare(const Json& left, const Json& right) {
    if (!number(left)) { const auto order = qtext(left).compare(qtext(right)); return order < 0 ? -1 : order > 0 ? 1 : 0; }
    return compare_numbers(left.number(), qnumber(right));
}

// Exact decimal arithmetic for sum, avg, ceil, and floor: magnitude digits scaled by 10^-scale.
struct Decimal {
    bool negative = false;
    std::string digits = "0";
    long long scale = 0;
    static Decimal of(const Number& value) {
        auto [coefficient, power] = value.canonical();
        Decimal result;
        result.negative = coefficient.front() == '-';
        if (result.negative) coefficient.erase(0, 1);
        result.digits = coefficient;
        result.scale = -exponent(power);
        if (result.scale < 0) { result.digits.append(static_cast<std::size_t>(-result.scale), '0'); result.scale = 0; }
        if (result.digits == "0") result.negative = false;
        return result;
    }
    void rescale(long long target) { if (target > scale) { digits.append(static_cast<std::size_t>(target - scale), '0'); scale = target; } }
    static int magnitude(const std::string& a, const std::string& b) {
        if (a.size() != b.size()) return a.size() < b.size() ? -1 : 1;
        const auto order = a.compare(b);
        return order < 0 ? -1 : order > 0 ? 1 : 0;
    }
    static std::string strip(std::string value) { const auto first = value.find_first_not_of('0'); return first == std::string::npos ? "0" : value.substr(first); }
    static std::string add(const std::string& a, const std::string& b) {
        std::string result;
        int carry = 0;
        for (std::size_t i = 0; i < (std::max)(a.size(), b.size()) || carry; ++i) {
            const int sum = carry + (i < a.size() ? a[a.size() - 1 - i] - '0' : 0) + (i < b.size() ? b[b.size() - 1 - i] - '0' : 0);
            result += static_cast<char>('0' + sum % 10);
            carry = sum / 10;
        }
        std::reverse(result.begin(), result.end());
        return strip(result);
    }
    static std::string subtract(const std::string& a, const std::string& b) {
        std::string result;
        int borrow = 0;
        for (std::size_t i = 0; i < a.size(); ++i) {
            int value = a[a.size() - 1 - i] - '0' - borrow - (i < b.size() ? b[b.size() - 1 - i] - '0' : 0);
            borrow = value < 0;
            if (value < 0) value += 10;
            result += static_cast<char>('0' + value);
        }
        std::reverse(result.begin(), result.end());
        return strip(result);
    }
    Decimal plus(Decimal other) const {
        Decimal self = *this;
        const auto target = (std::max)(self.scale, other.scale);
        self.rescale(target); other.rescale(target);
        Decimal result;
        result.scale = target;
        if (self.negative == other.negative) { result.digits = add(self.digits, other.digits); result.negative = self.negative; }
        else if (magnitude(strip(self.digits), strip(other.digits)) >= 0) { result.digits = subtract(strip(self.digits), strip(other.digits)); result.negative = self.negative; }
        else { result.digits = subtract(strip(other.digits), strip(self.digits)); result.negative = other.negative; }
        if (result.digits == "0") result.negative = false;
        return result;
    }
    Decimal divided(std::size_t divisor, long long precision) const {
        Decimal result = *this;
        result.rescale(precision);
        std::string quotient;
        unsigned long long remainder = 0;
        for (const char digit : result.digits) {
            remainder = remainder * 10 + static_cast<unsigned long long>(digit - '0');
            quotient += static_cast<char>('0' + remainder / divisor);
            remainder %= divisor;
        }
        result.digits = strip(quotient);
        if (remainder * 2 >= divisor) result.digits = add(result.digits, "1");
        if (result.digits == "0") result.negative = false;
        return result;
    }
    [[nodiscard]] Json json() const {
        auto value = strip(digits);
        auto places = scale;
        while (places > 0 && value.size() > 1 && value.back() == '0') { value.pop_back(); --places; }
        if (value == "0") return Json(Number("0"));
        if (places > 0) {
            if (static_cast<long long>(value.size()) <= places) value.insert(0, static_cast<std::size_t>(places) - value.size() + 1, '0');
            value.insert(value.size() - static_cast<std::size_t>(places), ".");
        }
        return Json(Number((negative ? "-" : "") + value));
    }
};

std::size_t points(const std::string& value) { return static_cast<std::size_t>(std::count_if(value.begin(), value.end(), [](char c) { return (static_cast<unsigned char>(c) & 0xc0) != 0x80; })); }
Json eval(const Json& node, const Json& input);
Json reference(const Json::Array& nodes, std::size_t index, const Json& input) {
    const auto& node = nodes[index];
    if (qtext(node.at("kind")) != "expressionReference") fail("JMESPath expected expression reference");
    return eval(node.at("expression"), input);
}
Json::Array ordered(const Json& value) {
    auto result = qarray(value);
    if (!result.empty()) { const bool numeric = number(result[0]); for (const auto& item : result) { if (numeric) (void)qnumber(item); else (void)qtext(item); } }
    std::stable_sort(result.begin(), result.end(), [](const Json& a, const Json& b) { return compare(a, b) < 0; });
    return result;
}
Json::Array values(const Json::Object& value) { Json::Array result; for (const auto& [key, item] : value) result.push_back(item); return result; }
Json function(const std::string& name, const Json::Array& nodes, const Json& input) {
    const bool variadic = name == "merge" || name == "not_null";
    std::size_t arity = 1;
    for (const auto* binary : {"contains", "starts_with", "ends_with", "join", "map", "max_by", "min_by", "sort_by"}) if (name == binary) arity = 2;
    if ((variadic && nodes.empty()) || (!variadic && nodes.size() != arity)) fail("JMESPath invalid function arity");
    const auto arg = [&](std::size_t index) { return eval(nodes[index], input); };
    if (name == "abs") { auto value = qnumber(arg(0)).text(); if (value.starts_with('-')) value.erase(0, 1); return Json(Number(value)); }
    if (name == "ceil" || name == "floor") {
        const auto value = Decimal::of(qnumber(arg(0)));
        const auto integer_digits = static_cast<long long>(value.digits.size()) - value.scale;
        const auto whole = integer_digits > 0 ? value.digits.substr(0, static_cast<std::size_t>(integer_digits)) : std::string("0");
        const bool fraction = value.digits.substr(static_cast<std::size_t>((std::max)(0LL, integer_digits))).find_first_not_of('0') != std::string::npos;
        Decimal result; result.negative = value.negative; result.digits = Decimal::strip(whole);
        if (fraction && ((name == "ceil" && !value.negative) || (name == "floor" && value.negative))) result.digits = Decimal::add(result.digits, "1");
        if (result.digits == "0") result.negative = false;
        return result.json();
    }
    if (name == "sum" || name == "avg") {
        const auto list_value = arg(0);
        const auto& items = qarray(list_value);
        Decimal total;
        for (const auto& item : items) total = total.plus(Decimal::of(qnumber(item)));
        if (name == "sum") return total.json();
        if (items.empty()) return Json();
        return total.divided(items.size(), 34).json();
    }
    if (name == "contains") {
        const auto source = arg(0), wanted = arg(1);
        if (text(source)) return Json(source.string().find(qtext(wanted)) != std::string::npos);
        for (const auto& item : qarray(source)) if (equivalent(item, wanted)) return Json(true);
        return Json(false);
    }
    if (name == "starts_with") return Json(qtext(arg(0)).starts_with(qtext(arg(1))));
    if (name == "ends_with") return Json(qtext(arg(0)).ends_with(qtext(arg(1))));
    if (name == "join") {
        const auto separator = qtext(arg(0));
        const auto list_value = arg(1);
        std::string result;
        bool first = true;
        for (const auto& item : qarray(list_value)) { if (!first) result += separator; first = false; result += qtext(item); }
        return Json(result);
    }
    if (name == "keys") { const auto object_value = arg(0); Json::Array result; for (const auto& [key, item] : qobject(object_value)) result.push_back(Json(key)); return Json(std::move(result)); }
    if (name == "values") return Json(values(qobject(arg(0))));
    if (name == "length") {
        const auto value = arg(0);
        if (list(value)) return Json(static_cast<std::int64_t>(value.array().size()));
        if (object(value)) return Json(static_cast<std::int64_t>(value.object().size()));
        return Json(static_cast<std::int64_t>(points(qtext(value))));
    }
    if (name == "map") { const auto list_value = arg(1); Json::Array result; for (const auto& item : qarray(list_value)) result.push_back(reference(nodes, 0, item)); return Json(std::move(result)); }
    if (name == "max" || name == "min" || name == "sort") {
        auto items = ordered(arg(0));
        if (name == "sort") return Json(std::move(items));
        if (items.empty()) return Json();
        return name == "max" ? items.back() : items.front();
    }
    if (name == "max_by" || name == "min_by" || name == "sort_by") {
        const auto items = qarray(arg(0));
        Json::Array keys;
        for (const auto& item : items) keys.push_back(reference(nodes, 1, item));
        (void)ordered(Json(keys));
        if (items.empty()) return name == "sort_by" ? Json(Json::Array{}) : Json();
        if (name == "sort_by") {
            std::vector<std::size_t> indices(items.size());
            std::iota(indices.begin(), indices.end(), std::size_t{0});
            std::stable_sort(indices.begin(), indices.end(), [&](std::size_t a, std::size_t b) { return compare(keys[a], keys[b]) < 0; });
            Json::Array result;
            for (const auto index : indices) result.push_back(items[index]);
            return Json(std::move(result));
        }
        std::size_t selected = 0;
        for (std::size_t i = 1; i < items.size(); ++i) { const auto order = compare(keys[i], keys[selected]); if ((name == "max_by" && order > 0) || (name == "min_by" && order < 0)) selected = i; }
        return items[selected];
    }
    if (name == "merge") { Json::Object result; for (std::size_t i = 0; i < nodes.size(); ++i) { const auto object_value = arg(i); for (const auto& [key, item] : qobject(object_value)) result.insert_or_assign(key, item); } return Json(std::move(result)); }
    if (name == "not_null") { for (std::size_t i = 0; i < nodes.size(); ++i) if (auto value = arg(i); !value.is_null()) return value; return Json(); }
    if (name == "reverse") {
        const auto value = arg(0);
        if (list(value)) { auto result = value.array(); std::reverse(result.begin(), result.end()); return Json(std::move(result)); }
        const auto& source = qtext(value);
        std::vector<std::string> characters;
        for (std::size_t i = 0; i < source.size();) { std::size_t next = i + 1; while (next < source.size() && (static_cast<unsigned char>(source[next]) & 0xc0) == 0x80) ++next; characters.push_back(source.substr(i, next - i)); i = next; }
        std::string result;
        for (auto it = characters.rbegin(); it != characters.rend(); ++it) result += *it;
        return Json(result);
    }
    if (name == "to_array") { auto value = arg(0); if (list(value)) return value; return Json(Json::Array{value}); }
    if (name == "to_number") {
        auto value = arg(0);
        if (number(value)) return value;
        if (text(value)) { try { return Json(Number(value.string())); } catch (const std::invalid_argument&) {} }
        return Json();
    }
    if (name == "to_string") { auto value = arg(0); return text(value) ? value : Json(value.dump()); }
    if (name == "type") {
        const auto value = arg(0);
        return Json(value.is_null() ? "null" : is(value, 1) ? "boolean" : text(value) ? "string" : number(value) ? "number" : list(value) ? "array" : "object");
    }
    fail("Unknown compiled JMESPath function");
}
Json eval(const Json& node, const Json& input) {
    const auto child = [&](const char* name) { return eval(node.at(name), input); };
    const auto& kind = qtext(node.at("kind"));
    if (kind == "identity") return input;
    if (kind == "literal") return node.at("value");
    if (kind == "field") { if (object(input)) { const auto* value = input.find(qtext(node.at("name"))); return value ? *value : Json(); } return Json(); }
    if (kind == "index") {
        if (!list(input)) return Json();
        const auto& items = input.array();
        auto index = qinteger(node.at("index"));
        if (index < 0) index += static_cast<long long>(items.size());
        if (index < 0 || index >= static_cast<long long>(items.size())) return Json();
        return items[static_cast<std::size_t>(index)];
    }
    if (kind == "subexpression") return eval(node.at("right"), child("left"));
    if (kind == "projection") {
        const auto left = child("left");
        if (!list(left)) return Json();
        Json::Array result;
        for (const auto& item : left.array()) if (auto value = eval(node.at("right"), item); !value.is_null()) result.push_back(std::move(value));
        return Json(std::move(result));
    }
    if (kind == "flatten") {
        const auto value = child("expression");
        if (!list(value)) return Json();
        Json::Array result;
        for (const auto& item : value.array()) { if (list(item)) result.insert(result.end(), item.array().begin(), item.array().end()); else result.push_back(item); }
        return Json(std::move(result));
    }
    if (kind == "objectValues") { const auto value = child("expression"); if (!object(value)) return Json(); return Json(values(value.object())); }
    if (kind == "multiList") { if (input.is_null()) return Json(); Json::Array result; for (const auto& item : qarray(node.at("expressions"))) result.push_back(eval(item, input)); return Json(std::move(result)); }
    if (kind == "multiHash") { if (input.is_null()) return Json(); Json::Object result; for (const auto& entry : qarray(node.at("entries"))) result.insert_or_assign(qtext(entry.at("key")), eval(entry.at("value"), input)); return Json(std::move(result)); }
    if (kind == "and") { auto left = child("left"); return truth(left) ? child("right") : left; }
    if (kind == "or") { auto left = child("left"); return truth(left) ? left : child("right"); }
    if (kind == "not") return Json(!truth(child("expression")));
    if (kind == "condition") return truth(child("predicate")) ? child("then") : Json();
    if (kind == "compare") {
        const auto left = child("left"), right = child("right");
        const auto& comparator = qtext(node.at("comparator"));
        if (comparator == "equal") return Json(equivalent(left, right));
        if (comparator == "notEqual") return Json(!equivalent(left, right));
        if (!number(left) || !number(right)) return Json();
        const auto order = compare(left, right);
        if (comparator == "lessThan") return Json(order < 0);
        if (comparator == "lessThanEqual") return Json(order <= 0);
        if (comparator == "greaterThan") return Json(order > 0);
        if (comparator == "greaterThanEqual") return Json(order >= 0);
        fail("Unknown JMESPath comparator");
    }
    if (kind == "slice") {
        if (!list(input)) return Json();
        const auto& items = input.array();
        const auto size = static_cast<long long>(items.size());
        const auto step = qinteger(node.at("step"));
        if (step == 0) fail("Zero slice step");
        const auto boundary = [&](const char* name, long long fallback) {
            const auto* value = node.find(name);
            if (!value || value->is_null()) return fallback;
            auto index = qinteger(*value);
            if (index < 0) index += size;
            return step > 0 ? (std::min)(size, (std::max)(0LL, index)) : (std::min)(size - 1, (std::max)(-1LL, index));
        };
        const auto start = boundary("start", step > 0 ? 0 : size - 1), stop = boundary("stop", step > 0 ? size : -1);
        Json::Array result;
        for (auto i = start; (step > 0 && i < stop) || (step < 0 && i > stop); i += step) result.push_back(items[static_cast<std::size_t>(i)]);
        return Json(std::move(result));
    }
    if (kind == "function") return function(qtext(node.at("function")), qarray(node.at("arguments")), input);
    fail("Unknown compiled JMESPath node");
}
}
Json query(const Json& expression, const Json& input) {
    try { return eval(expression, input); }
    catch (const QueryError&) { throw; }
    catch (const std::invalid_argument& error) { throw QueryError(error.what()); }
}
Json query_model(const Schema& schema, std::string_view target, const Json& value) {
    if (target.empty() || value.is_null()) return value;
    const auto& shape = schema.shape(target);
    const auto& kind = shape.at("kind").string();
    if (kind == "timestamp") {
        if (number(value)) return value;
        try { return Json(Timestamp::parse(value.string()).epoch()); }
        catch (const std::invalid_argument&) { return Json(Timestamp::from_http_date(value.string()).epoch()); }
    }
    if (kind == "structure" || kind == "union") {
        Json::Object result;
        for (const auto& [name, member] : shape.at("members").object()) if (const auto* item = value.find(name)) result.emplace(name, query_model(schema, member.at("target").string(), *item));
        return Json(std::move(result));
    }
    if (kind == "list") { Json::Array result; for (const auto& item : value.array()) result.push_back(query_model(schema, shape.at("member").at("target").string(), item)); return Json(std::move(result)); }
    if (kind == "map") { Json::Object result; for (const auto& [key, item] : value.object()) result.emplace(key, query_model(schema, shape.at("value").at("target").string(), item)); return Json(std::move(result)); }
    return value;
}
}

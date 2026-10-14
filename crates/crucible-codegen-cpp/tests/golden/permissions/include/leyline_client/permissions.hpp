#pragma once
#include <array>
#include <compare>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <utility>

namespace leyline::permissions {

/** One permission in the registry. */
struct PermissionDescription {
    /** What grants store. */
    std::string_view value;
    /** The model's documentation for it, empty when it has none. */
    std::string_view description;
    /** "public", "permissionOnly" or "notGrantable"; a string, since a newer server may add tiers. */
    std::string_view tier;
    /** False for a bundle the service confers whole: never offer it in a picker. */
    bool grantable;
    bool deprecated;
    /** For a deprecated alias, the value it stands for. */
    std::optional<std::string_view> alias_of;
};

/** One resource type's permissions, sorted by value. */
struct ResourcePermissions {
    std::string_view resource_type;
    std::span<const PermissionDescription> permissions;
};

/**
 * The permissions `Graph` accepts (`graph`), by the values grants store: a known one, or any string a newer server writes, kept as it is.
 */
struct GraphPermission {
    std::string value;
    explicit GraphPermission(std::string value = {}) : value(std::move(value)) {}
    auto operator<=>(const GraphPermission&) const = default;
    /**
     * Read a graph and its catalog.
     */
    static GraphPermission known_read() { return GraphPermission{"graph:read"}; }
    /**
     * Change a graph: *&#47; ends no comment and [bare] links nowhere.
     * &#64;param is not a tag.
     */
    static GraphPermission known_write() { return GraphPermission{"graph:write"}; }
    static GraphPermission known_audit() { return GraphPermission{"graph:audit"}; }
    /**
     * An alias of `graph:read`.
     *
     * @deprecated Renamed to graph:read.
     */
    static GraphPermission known_view() { return GraphPermission{"graph:view"}; }
    /** Returns the permission this value stands for: a deprecated alias's primary, or this value itself, unknown values included. */
    GraphPermission canonical() const {
        if (value == "graph:view") return GraphPermission{"graph:read"};
        return *this;
    }
};

/**
 * The permissions `History` accepts (`history`), by the values grants store: a known one, or any string a newer server writes, kept as it is.
 */
struct HistoryPermission {
    std::string value;
    explicit HistoryPermission(std::string value = {}) : value(std::move(value)) {}
    auto operator<=>(const HistoryPermission&) const = default;
    static HistoryPermission known_read() { return HistoryPermission{"history:read"}; }
    static HistoryPermission known_restore() { return HistoryPermission{"history:restore"}; }
    /** Returns the permission this value stands for: a deprecated alias's primary, or this value itself, unknown values included. */
    HistoryPermission canonical() const {
        return *this;
    }
};

/**
 * The permissions `Organization` accepts (`org`), by the values grants store: a known one, or any string a newer server writes, kept as it is.
 */
struct OrganizationPermission {
    std::string value;
    explicit OrganizationPermission(std::string value = {}) : value(std::move(value)) {}
    auto operator<=>(const OrganizationPermission&) const = default;
    /**
     * Everything in the organization.
     */
    static OrganizationPermission known_admin() { return OrganizationPermission{"organization:admin"}; }
    /** Returns the permission this value stands for: a deprecated alias's primary, or this value itself, unknown values included. */
    OrganizationPermission canonical() const {
        return *this;
    }
};

namespace detail {
inline constexpr PermissionDescription GraphEntries[] = {
    {"graph:audit", "", "permissionOnly", true, false, std::nullopt},
    {"graph:read", "Read a graph and its catalog.", "public", true, false, std::nullopt},
    {"graph:view", "", "public", true, true, "graph:read"},
    {"graph:write", "Change a graph: */ ends no comment and [bare] links nowhere.\012@param is not a tag.", "public", true, false, std::nullopt},
};
inline constexpr PermissionDescription HistoryEntries[] = {
    {"history:read", "", "public", true, false, std::nullopt},
    {"history:restore", "", "permissionOnly", true, false, std::nullopt},
};
inline constexpr PermissionDescription OrganizationEntries[] = {
    {"organization:admin", "Everything in the organization.", "notGrantable", false, false, std::nullopt},
};
}

/** Every permission this client knows, by resource type, deprecated aliases included. A grant picker offers only the grantable ones. */
inline constexpr std::array<ResourcePermissions, 3> registry{{
    {"graph", detail::GraphEntries},
    {"history", detail::HistoryEntries},
    {"org", detail::OrganizationEntries},
}};

}

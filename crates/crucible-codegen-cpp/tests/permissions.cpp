// The typed permissions of tests/fixtures/permissions as a C++ consumer sees
// them (docs/permissions.md §6.3): open value types, canonical(), and the
// constexpr registry a grant picker reads.
#include "leyline_client/permissions.hpp"
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>

namespace permissions = leyline::permissions;

static void check(bool condition, const char* what) {
    if (!condition) throw std::runtime_error(what);
}

// The registry is a compile-time table.
static_assert(permissions::registry.size() == 3);
static_assert(permissions::registry[0].resource_type == "graph");
static_assert(permissions::registry[2].permissions[0].tier == "notGrantable");

static const permissions::PermissionDescription* find(std::string_view type, std::string_view value) {
    for (const auto& resource : permissions::registry) {
        if (resource.resource_type != type) continue;
        for (const auto& permission : resource.permissions) {
            if (permission.value == value) return &permission;
        }
    }
    return nullptr;
}

int main() {
    using permissions::GraphPermission;
    check(GraphPermission::known_read().value == "graph:read", "known constant");
    check(GraphPermission::known_read() == GraphPermission{"graph:read"}, "value equality");

    // A grant a newer server wrote, or one naming an internal permission this
    // client never sees, is still a value of the type, kept as it is.
    for (const char* raw : {"graph:export", "organization:read"}) {
        const std::string text = raw;
        const GraphPermission stored{text};
        check(stored.value == text, "unknown value kept");
        check(stored.canonical() == stored, "unknown value canonical is itself");
        check(GraphPermission{stored.value}.value == text, "unknown value round trip");
    }

    // A deprecated alias keeps its value and canonicalizes to its primary.
    check(GraphPermission::known_view().value == "graph:view", "alias value");
    check(GraphPermission::known_view().canonical() == GraphPermission::known_read(), "alias canonical");
    check(GraphPermission::known_write().canonical() == GraphPermission::known_write(), "primary canonical");
    check(permissions::HistoryPermission{"graph:view"}.canonical().value == "graph:view", "another type's alias");

    // The registry: resource types sorted, no internal permission.
    std::vector<std::string_view> types;
    for (const auto& resource : permissions::registry) {
        types.push_back(resource.resource_type);
        for (const auto& permission : resource.permissions) {
            check(permission.value != "organization:read", "internal permission listed");
        }
    }
    check((types == std::vector<std::string_view>{"graph", "history", "org"}), "resource types");
    const auto* admin = find("org", "organization:admin");
    check(admin && !admin->grantable && admin->tier == "notGrantable", "notGrantable admin");
    const auto* alias = find("graph", "graph:view");
    check(alias && alias->deprecated && alias->alias_of == "graph:read", "alias entry");
    const auto* audit = find("graph", "graph:audit");
    check(audit && !audit->alias_of && audit->tier == "permissionOnly", "permissionOnly entry");

    // A grant picker offers the grantable permissions that are not deprecated.
    std::vector<std::string_view> picker;
    for (const auto& resource : permissions::registry) {
        for (const auto& permission : resource.permissions) {
            if (permission.grantable && !permission.deprecated) picker.push_back(permission.value);
        }
    }
    check((picker == std::vector<std::string_view>{"graph:audit", "graph:read", "graph:write", "history:read", "history:restore"}), "picker");
    std::cout << "C++ typed permissions passed\n";
}

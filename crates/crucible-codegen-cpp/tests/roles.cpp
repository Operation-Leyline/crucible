// The role registry of tests/models/roles-granted-by as a C++ consumer sees
// it (docs/roles.md §7 "Clients"): open role and scope value types, and the
// constexpr registry a role picker reads, in catalogue order with public
// roles, scopes and permissions only.
#include "notes_client/roles.hpp"
#include <iostream>
#include <stdexcept>
#include <string>

namespace roles = notes::roles;

static void check(bool condition, const char* what) {
    if (!condition) throw std::runtime_error(what);
}

// The registry is a compile-time table.
static_assert(roles::registry.size() == 4);
static_assert(roles::registry[0].value == "owner");
static_assert(roles::registry[3].value == "archivist");
static_assert(roles::registry[0].holders.size() == 1 && roles::registry[0].holders[0] == "user");
static_assert(roles::registry[1].holders.empty() && roles::registry[1].assigned_by.empty());

int main() {
    try {
        check(roles::Role::known_owner().value == "owner", "owner");
        check(roles::Scope::known_all_notes().value == "allNotes", "allNotes");
        // A binding a newer server wrote, a custom role, or one naming an
        // internal role is still a value of the type.
        roles::Role unknown{"auditor"};
        check(unknown.value == "auditor", "unknown role");
        check(unknown != roles::Role::known_owner(), "comparison");
        const auto& owner = roles::registry[0];
        check(owner.scopes.size() == 1 && owner.scopes[0] == "workspace", "owner scopes");
        check(owner.assigned_by.size() == 1 && owner.assigned_by[0] == "owner", "owner assigners");
        check(owner.permissions.size() == 5, "owner permissions");
        check(owner.permissions[0].resource_type == "note" && owner.permissions[0].permission == "note:read", "first permission");
        check(owner.permissions[4].permission == "workspace:admin", "last permission");
        for (const auto& role : roles::registry) {
            for (const auto& permission : role.permissions) {
                check(permission.permission != "note:purge", "no internal permission");
            }
        }
    } catch (const std::exception& error) {
        std::cerr << error.what() << "\n";
        return 1;
    }
    std::cout << "roles: ok\n";
    return 0;
}

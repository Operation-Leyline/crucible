/* The typed permissions of tests/fixtures/permissions as a C consumer sees
   them (docs/permissions.md §6.3): open string views, canonical, and the
   static registry a grant picker reads. */
#include "leyline_client/permissions.h"
#include <stdio.h>
#include <string.h>

static int failures = 0;
#define CHECK(condition) do { if (!(condition)) { fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #condition); failures++; } } while (0)

static bool equal(leyline_string value, const char *text) {
    size_t length = strlen(text);
    return value.length == length && value.data && memcmp(value.data, text, length) == 0;
}

static const leyline_resource_permissions *resource(const char *type) {
    for (size_t index = 0; index < leyline_permissions_count; index++) {
        if (equal(leyline_permissions[index].resource_type, type)) return &leyline_permissions[index];
    }
    return NULL;
}

static const leyline_permission_description *find(const leyline_resource_permissions *owner, const char *value) {
    for (size_t index = 0; owner && index < owner->permission_count; index++) {
        if (equal(owner->permissions[index].value, value)) return &owner->permissions[index];
    }
    return NULL;
}

int main(void) {
    leyline_graph_permission read = leyline_graph_permission_known_read();
    CHECK(equal(read, "graph:read"));
    CHECK(read.data[read.length] == '\0');

    /* A grant a newer server wrote, or one naming an internal permission
       this client never sees, is still a value of the type: a borrowed view
       canonical hands back as it is, not even copied. */
    char buffer[] = "graph:exportorganization:read";
    leyline_graph_permission unknown = {buffer, 12};
    leyline_graph_permission internal = {buffer + 12, 17};
    leyline_graph_permission kept = leyline_graph_permission_canonical(unknown);
    CHECK(kept.data == unknown.data && kept.length == unknown.length && equal(kept, "graph:export"));
    kept = leyline_graph_permission_canonical(internal);
    CHECK(kept.data == internal.data && equal(kept, "organization:read"));
    leyline_graph_permission empty = {NULL, 0};
    CHECK(leyline_graph_permission_canonical(empty).data == NULL);

    /* A deprecated alias keeps its value and canonicalizes to its primary;
       a primary, and another type's alias, are themselves. */
    CHECK(equal(leyline_graph_permission_known_view(), "graph:view"));
    CHECK(equal(leyline_graph_permission_canonical(leyline_graph_permission_known_view()), "graph:read"));
    char view[] = "graph:view";
    leyline_graph_permission decoded = {view, 10};
    CHECK(equal(leyline_graph_permission_canonical(decoded), "graph:read"));
    CHECK(equal(leyline_graph_permission_canonical(read), "graph:read"));
    CHECK(equal(leyline_history_permission_canonical(decoded), "graph:view"));
    CHECK(equal(leyline_organization_permission_canonical(leyline_organization_permission_known_admin()), "organization:admin"));

    /* The registry: resource types sorted, no internal permission. */
    CHECK(leyline_permissions_count == 3);
    CHECK(sizeof(leyline_permissions) / sizeof(leyline_permissions[0]) == leyline_permissions_count);
    CHECK(equal(leyline_permissions[0].resource_type, "graph"));
    CHECK(equal(leyline_permissions[1].resource_type, "history"));
    CHECK(equal(leyline_permissions[2].resource_type, "org"));
    for (size_t index = 0; index < leyline_permissions_count; index++) {
        CHECK(find(&leyline_permissions[index], "organization:read") == NULL);
    }
    const leyline_permission_description *admin = find(resource("org"), "organization:admin");
    CHECK(admin && !admin->grantable && equal(admin->tier, "notGrantable"));
    CHECK(admin && equal(admin->description, "Everything in the organization."));
    const leyline_permission_description *alias = find(resource("graph"), "graph:view");
    CHECK(alias && alias->deprecated && equal(alias->alias_of, "graph:read"));
    const leyline_permission_description *audit = find(resource("graph"), "graph:audit");
    CHECK(audit && audit->alias_of.data == NULL && equal(audit->tier, "permissionOnly") && audit->grantable);

    /* A grant picker offers the grantable permissions that are not deprecated. */
    const char *expected[] = {"graph:audit", "graph:read", "graph:write", "history:read", "history:restore"};
    size_t offered = 0;
    for (size_t index = 0; index < leyline_permissions_count; index++) {
        const leyline_resource_permissions *owner = &leyline_permissions[index];
        for (size_t at = 0; at < owner->permission_count; at++) {
            const leyline_permission_description *permission = &owner->permissions[at];
            if (!permission->grantable || permission->deprecated) continue;
            CHECK(offered < sizeof(expected) / sizeof(expected[0]) && equal(permission->value, expected[offered]));
            offered++;
        }
    }
    CHECK(offered == sizeof(expected) / sizeof(expected[0]));

    if (failures) return 1;
    puts("C typed permissions passed");
    return 0;
}

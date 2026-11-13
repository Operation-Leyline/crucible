/* The role registry of tests/models/roles-granted-by as a C consumer sees
   it (docs/roles.md §7 "Clients"): open string views for role and scope
   values, and the static registry a role picker reads, in catalogue order
   with public roles, scopes and permissions only. */
#include "notes_client/roles.h"
#include <stdio.h>
#include <string.h>

static int failures = 0;
#define CHECK(condition) do { if (!(condition)) { fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #condition); failures++; } } while (0)

static bool equal(notes_string value, const char *text) {
    size_t length = strlen(text);
    return value.length == length && value.data && memcmp(value.data, text, length) == 0;
}

int main(void) {
    notes_role_value owner = notes_role_value_known_owner();
    CHECK(equal(owner, "owner"));
    CHECK(owner.data[owner.length] == '\0');
    CHECK(equal(notes_role_scope_value_known_all_notes(), "allNotes"));

    /* A binding a newer server wrote, a custom role, or one naming an
       internal role is still a value of the type: a borrowed view. */
    char buffer[] = "auditor";
    notes_role_value unknown = {buffer, 7};
    CHECK(equal(unknown, "auditor"));

    const char *expected[] = {"owner", "viewer", "editor", "archivist"};
    CHECK(notes_roles_count == 4);
    for (size_t index = 0; index < notes_roles_count && index < 4; index++) {
        CHECK(equal(notes_roles[index].value, expected[index]));
    }
    const notes_role_description *first = &notes_roles[0];
    CHECK(first->scope_count == 1 && equal(first->scopes[0], "workspace"));
    CHECK(first->holder_count == 1 && equal(first->holders[0], "user"));
    CHECK(first->assigned_by_count == 1 && equal(first->assigned_by[0], "owner"));
    CHECK(first->permission_count == 5);
    CHECK(equal(first->permissions[0].resource_type, "note"));
    CHECK(equal(first->permissions[0].permission, "note:read"));
    CHECK(equal(first->permissions[4].permission, "workspace:admin"));
    const notes_role_description *viewer = &notes_roles[1];
    CHECK(viewer->scope_count == 3 && equal(viewer->scopes[1], "allNotes"));
    CHECK(viewer->holder_count == 0 && viewer->holders == NULL);
    CHECK(viewer->assigned_by_count == 0 && viewer->assigned_by == NULL);
    for (size_t index = 0; index < notes_roles_count; index++) {
        for (size_t p = 0; p < notes_roles[index].permission_count; p++) {
            CHECK(!equal(notes_roles[index].permissions[p].permission, "note:purge"));
        }
    }
    if (failures) return 1;
    puts("roles: ok");
    return 0;
}

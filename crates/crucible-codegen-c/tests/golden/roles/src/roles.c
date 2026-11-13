#include "notes_client/roles.h"
#include <stddef.h>

static const notes_string scopes_0[] = {{"workspace", 9}};
static const notes_string holders_0[] = {{"user", 4}};
static const notes_string assigned_by_0[] = {{"owner", 5}};
static const notes_role_permission permissions_0[] = {
    {{"note", 4}, {"note:read", 9}},
    {{"note", 4}, {"note:write", 10}},
    {{"note", 4}, {"note:create", 11}},
    {{"workspace", 9}, {"workspace:read", 14}},
    {{"workspace", 9}, {"workspace:admin", 15}},
};
static const notes_string scopes_1[] = {{"workspace", 9}, {"allNotes", 8}, {"note", 4}};
static const notes_role_permission permissions_1[] = {
    {{"note", 4}, {"note:read", 9}},
};
static const notes_string scopes_2[] = {{"allNotes", 8}, {"note", 4}};
static const notes_role_permission permissions_2[] = {
    {{"note", 4}, {"note:read", 9}},
    {{"note", 4}, {"note:write", 10}},
    {{"note", 4}, {"note:create", 11}},
};
static const notes_string scopes_3[] = {{"allNotes", 8}};
static const notes_role_permission permissions_3[] = {
    {{"note", 4}, {"note:read", 9}},
    {{"note", 4}, {"note:write", 10}},
    {{"note", 4}, {"note:create", 11}},
};
const notes_role_description notes_roles[4] = {
    {{"owner", 5}, {"Runs the workspace.", 19}, 1, scopes_0, 1, holders_0, 1, assigned_by_0, 5, permissions_0},
    {{"viewer", 6}, {"Reads notes.", 12}, 3, scopes_1, 0, NULL, 0, NULL, 1, permissions_1},
    {{"editor", 6}, {"Reads, changes and writes notes.", 32}, 2, scopes_2, 0, NULL, 0, NULL, 3, permissions_2},
    {{"archivist", 9}, {"Edits notes and reads the archives they came from.", 50}, 1, scopes_3, 0, NULL, 0, NULL, 3, permissions_3},
};
const size_t notes_roles_count = 4;

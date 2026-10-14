#include "leyline_client/permissions.h"
#include <string.h>

leyline_graph_permission leyline_graph_permission_canonical(leyline_graph_permission value) {
    if (value.length == 10 && memcmp(value.data, "graph:view", 10) == 0) { leyline_graph_permission primary = {"graph:read", 10}; return primary; }
    return value;
}
static const leyline_permission_description entries_0[] = {
    {{"graph:audit", 11}, {"", 0}, {"permissionOnly", 14}, true, false, {NULL, 0}},
    {{"graph:read", 10}, {"Read a graph and its catalog.", 29}, {"public", 6}, true, false, {NULL, 0}},
    {{"graph:view", 10}, {"", 0}, {"public", 6}, true, true, {"graph:read", 10}},
    {{"graph:write", 11}, {"Change a graph: */ ends no comment and [bare] links nowhere.\012@param is not a tag.", 81}, {"public", 6}, true, false, {NULL, 0}},
};
leyline_history_permission leyline_history_permission_canonical(leyline_history_permission value) {
    return value;
}
static const leyline_permission_description entries_1[] = {
    {{"history:read", 12}, {"", 0}, {"public", 6}, true, false, {NULL, 0}},
    {{"history:restore", 15}, {"", 0}, {"permissionOnly", 14}, true, false, {NULL, 0}},
};
leyline_organization_permission leyline_organization_permission_canonical(leyline_organization_permission value) {
    return value;
}
static const leyline_permission_description entries_2[] = {
    {{"organization:admin", 18}, {"Everything in the organization.", 31}, {"notGrantable", 12}, false, false, {NULL, 0}},
};
const leyline_resource_permissions leyline_permissions[3] = {
    {{"graph", 5}, 4, entries_0},
    {{"history", 7}, 2, entries_1},
    {{"org", 3}, 1, entries_2},
};
const size_t leyline_permissions_count = 3;

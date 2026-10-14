#ifndef LEYLINE_PERMISSIONS_H
#define LEYLINE_PERMISSIONS_H
#include "runtime/types.h"

#ifdef __cplusplus
extern "C" {
#endif

/** One permission in the registry. Every view has static storage and is NUL-terminated; never free one. */
typedef struct {
    /** What grants store. */
    leyline_string value;
    /** The model's documentation for it, empty when it has none. */
    leyline_string description;
    /** "public", "permissionOnly" or "notGrantable"; a string, since a newer server may add tiers. */
    leyline_string tier;
    /** False for a bundle the service confers whole: never offer it in a picker. */
    bool grantable;
    bool deprecated;
    /** For a deprecated alias, the value it stands for; data is NULL otherwise. */
    leyline_string alias_of;
} leyline_permission_description;

/** One resource type's permissions, sorted by value. */
typedef struct {
    leyline_string resource_type;
    size_t permission_count;
    const leyline_permission_description *permissions;
} leyline_resource_permissions;

/**
 * The permissions `Graph` accepts (`graph`), by the values grants store: a known one, or any string a newer server writes, kept as it is. A value is a borrowed view.
 */
typedef leyline_string leyline_graph_permission;
/**
 * Read a graph and its catalog.
 */
static inline leyline_graph_permission leyline_graph_permission_known_read(void) { leyline_graph_permission result = {"graph:read", 10}; return result; }
/**
 * Change a graph: *&#47; ends no comment and [bare] links nowhere.
 * &#64;param is not a tag.
 */
static inline leyline_graph_permission leyline_graph_permission_known_write(void) { leyline_graph_permission result = {"graph:write", 11}; return result; }
static inline leyline_graph_permission leyline_graph_permission_known_audit(void) { leyline_graph_permission result = {"graph:audit", 11}; return result; }
/**
 * An alias of `graph:read`.
 *
 * @deprecated Renamed to graph:read.
 */
static inline leyline_graph_permission leyline_graph_permission_known_view(void) { leyline_graph_permission result = {"graph:view", 10}; return result; }
/** Returns the permission value stands for: a deprecated alias's primary, or value itself, unknown values included. The result is value or a view of static storage. */
leyline_graph_permission leyline_graph_permission_canonical(leyline_graph_permission value);

/**
 * The permissions `History` accepts (`history`), by the values grants store: a known one, or any string a newer server writes, kept as it is. A value is a borrowed view.
 */
typedef leyline_string leyline_history_permission;
static inline leyline_history_permission leyline_history_permission_known_read(void) { leyline_history_permission result = {"history:read", 12}; return result; }
static inline leyline_history_permission leyline_history_permission_known_restore(void) { leyline_history_permission result = {"history:restore", 15}; return result; }
/** Returns the permission value stands for: a deprecated alias's primary, or value itself, unknown values included. The result is value or a view of static storage. */
leyline_history_permission leyline_history_permission_canonical(leyline_history_permission value);

/**
 * The permissions `Organization` accepts (`org`), by the values grants store: a known one, or any string a newer server writes, kept as it is. A value is a borrowed view.
 */
typedef leyline_string leyline_organization_permission;
/**
 * Everything in the organization.
 */
static inline leyline_organization_permission leyline_organization_permission_known_admin(void) { leyline_organization_permission result = {"organization:admin", 18}; return result; }
/** Returns the permission value stands for: a deprecated alias's primary, or value itself, unknown values included. The result is value or a view of static storage. */
leyline_organization_permission leyline_organization_permission_canonical(leyline_organization_permission value);

/** Every permission this client knows, by resource type, deprecated aliases included; static storage, never freed. A grant picker offers only the grantable ones. */
extern const leyline_resource_permissions leyline_permissions[3];
/** The number of resource types in leyline_permissions. */
extern const size_t leyline_permissions_count;

#ifdef __cplusplus
}
#endif
#endif

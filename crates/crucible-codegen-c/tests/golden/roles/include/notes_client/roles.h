#ifndef NOTES_ROLES_H
#define NOTES_ROLES_H
#include "runtime/types.h"

#ifdef __cplusplus
extern "C" {
#endif

/**
 * The role values of `Notes`, by the values bindings store: a known one, or any string a newer server writes, kept as it is. A value is a borrowed view.
 */
typedef notes_string notes_role_value;
/**
 * Runs the workspace.
 *
 * `owner`.
 */
static inline notes_role_value notes_role_value_known_owner(void) { notes_role_value result = {"owner", 5}; return result; }
/**
 * Reads notes.
 *
 * `viewer`.
 */
static inline notes_role_value notes_role_value_known_viewer(void) { notes_role_value result = {"viewer", 6}; return result; }
/**
 * Reads, changes and writes notes.
 *
 * `editor`.
 */
static inline notes_role_value notes_role_value_known_editor(void) { notes_role_value result = {"editor", 6}; return result; }
/**
 * Edits notes and reads the archives they came from.
 *
 * `archivist`.
 */
static inline notes_role_value notes_role_value_known_archivist(void) { notes_role_value result = {"archivist", 9}; return result; }

/**
 * The scope values of `Notes`, by the values bindings store: a known one, or any string a newer server writes, kept as it is. A value is a borrowed view.
 */
typedef notes_string notes_role_scope_value;
/**
 * The workspace and everything in it.
 *
 * `workspace`.
 */
static inline notes_role_scope_value notes_role_scope_value_known_workspace(void) { notes_role_scope_value result = {"workspace", 9}; return result; }
/**
 * Every note in the workspace, including notes written later.
 *
 * `allNotes`.
 */
static inline notes_role_scope_value notes_role_scope_value_known_all_notes(void) { notes_role_scope_value result = {"allNotes", 8}; return result; }
/**
 * One note.
 *
 * `note`.
 */
static inline notes_role_scope_value notes_role_scope_value_known_note(void) { notes_role_scope_value result = {"note", 4}; return result; }

/** One permission a role grants. Every view has static storage and is NUL-terminated; never free one. */
typedef struct {
    /** The resource type a grant names. */
    notes_string resource_type;
    /** What grants store. */
    notes_string permission;
} notes_role_permission;

/** One role in the registry. Every view and array has static storage; never free one. An empty list has a count of 0 and a NULL array. */
typedef struct {
    /** What bindings store. */
    notes_string value;
    /** The model's documentation for it, empty when it has none. */
    notes_string description;
    size_t scope_count;
    /** The scope values it may be bound at, in catalogue order. */
    const notes_string *scopes;
    size_t holder_count;
    /** The kinds of principal that may hold it; none: any. */
    const notes_string *holders;
    size_t assigned_by_count;
    /** The roles whose holders alone may assign it; none: whoever may write bindings. */
    const notes_string *assigned_by;
    size_t permission_count;
    /** What it grants: the permissions this client knows. */
    const notes_role_permission *permissions;
} notes_role_description;

/** Every role, in the service's catalogue order: what a role picker offers; static storage, never freed. The service also accepts custom roles, which are not listed. */
extern const notes_role_description notes_roles[4];
/** The number of roles in notes_roles. */
extern const size_t notes_roles_count;

#ifdef __cplusplus
}
#endif
#endif

#pragma once
#include <array>
#include <compare>
#include <span>
#include <string>
#include <string_view>
#include <utility>

namespace notes::roles {

/**
 * The role values of `Notes`, by the values bindings store: a known one, or any string a newer server writes, kept as it is.
 */
struct Role {
    std::string value;
    explicit Role(std::string value = {}) : value(std::move(value)) {}
    auto operator<=>(const Role&) const = default;
    /**
     * Runs the workspace.
     *
     * `owner`.
     */
    static Role known_owner() { return Role{"owner"}; }
    /**
     * Reads notes.
     *
     * `viewer`.
     */
    static Role known_viewer() { return Role{"viewer"}; }
    /**
     * Reads, changes and writes notes.
     *
     * `editor`.
     */
    static Role known_editor() { return Role{"editor"}; }
    /**
     * Edits notes and reads the archives they came from.
     *
     * `archivist`.
     */
    static Role known_archivist() { return Role{"archivist"}; }
};

/**
 * The scope values of `Notes`, by the values bindings store: a known one, or any string a newer server writes, kept as it is.
 */
struct Scope {
    std::string value;
    explicit Scope(std::string value = {}) : value(std::move(value)) {}
    auto operator<=>(const Scope&) const = default;
    /**
     * The workspace and everything in it.
     *
     * `workspace`.
     */
    static Scope known_workspace() { return Scope{"workspace"}; }
    /**
     * Every note in the workspace, including notes written later.
     *
     * `allNotes`.
     */
    static Scope known_all_notes() { return Scope{"allNotes"}; }
    /**
     * One note.
     *
     * `note`.
     */
    static Scope known_note() { return Scope{"note"}; }
};

/** One permission a role grants. */
struct RolePermission {
    /** The resource type a grant names. */
    std::string_view resource_type;
    /** What grants store. */
    std::string_view permission;
};

/** One role in the registry. */
struct RoleDescription {
    /** What bindings store. */
    std::string_view value;
    /** The model's documentation for it, empty when it has none. */
    std::string_view description;
    /** The scope values it may be bound at, in catalogue order. */
    std::span<const std::string_view> scopes;
    /** The kinds of principal that may hold it; empty: any. */
    std::span<const std::string_view> holders;
    /** The roles whose holders alone may assign it; empty: whoever may write bindings. */
    std::span<const std::string_view> assigned_by;
    /** What it grants: the permissions this client knows. */
    std::span<const RolePermission> permissions;
};

namespace detail {
inline constexpr std::string_view Scopes0[] = {"workspace"};
inline constexpr std::string_view Holders0[] = {"user"};
inline constexpr std::string_view AssignedBy0[] = {"owner"};
inline constexpr RolePermission Permissions0[] = {RolePermission{"note", "note:read"}, RolePermission{"note", "note:write"}, RolePermission{"note", "note:create"}, RolePermission{"workspace", "workspace:read"}, RolePermission{"workspace", "workspace:admin"}};
inline constexpr std::string_view Scopes1[] = {"workspace", "allNotes", "note"};
inline constexpr RolePermission Permissions1[] = {RolePermission{"note", "note:read"}};
inline constexpr std::string_view Scopes2[] = {"allNotes", "note"};
inline constexpr RolePermission Permissions2[] = {RolePermission{"note", "note:read"}, RolePermission{"note", "note:write"}, RolePermission{"note", "note:create"}};
inline constexpr std::string_view Scopes3[] = {"allNotes"};
inline constexpr RolePermission Permissions3[] = {RolePermission{"note", "note:read"}, RolePermission{"note", "note:write"}, RolePermission{"note", "note:create"}};
}

/** Every role, in the service's catalogue order: what a role picker offers. The service also accepts custom roles, which are not listed. */
inline constexpr std::array<RoleDescription, 4> registry{{
    {"owner", "Runs the workspace.", detail::Scopes0, detail::Holders0, detail::AssignedBy0, detail::Permissions0},
    {"viewer", "Reads notes.", detail::Scopes1, {}, {}, detail::Permissions1},
    {"editor", "Reads, changes and writes notes.", detail::Scopes2, {}, {}, detail::Permissions2},
    {"archivist", "Edits notes and reads the archives they came from.", detail::Scopes3, {}, {}, detail::Permissions3},
}};

}

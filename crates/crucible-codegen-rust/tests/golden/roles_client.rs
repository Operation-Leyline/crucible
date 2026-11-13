/// The model's roles and the scopes they bind at, as the values bindings
/// store.
#[allow(deprecated, dead_code)]
pub mod roles {

    /// The service's roles. Open: a value this client does not know is
    /// kept as `UnknownValue` and written back unchanged. Ordered as the service
    /// lists them (`UnknownValue` last), not by value.
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[non_exhaustive]
    pub enum Role {
        /// Runs the workspace.
        ///
        /// `owner`.
        Owner,
        /// Reads notes.
        ///
        /// `viewer`.
        Viewer,
        /// Reads, changes and writes notes.
        ///
        /// `editor`.
        Editor,
        /// Edits notes and reads the archives they came from.
        ///
        /// `archivist`.
        Archivist,
        /// A value this client does not know.
        UnknownValue(::std::string::String),
    }

    impl Role {
        /// Every value this client knows, in catalogue order.
        pub const ALL: &'static [Self] = &[Self::Owner, Self::Viewer, Self::Editor, Self::Archivist];

        /// The value, as bindings store it.
        pub fn as_str(&self) -> &str {
            match self {
                Self::Owner => "owner",
                Self::Viewer => "viewer",
                Self::Editor => "editor",
                Self::Archivist => "archivist",
                Self::UnknownValue(value) => value,
            }
        }
    }

    impl ::crucible_client_runtime_rust::__generated::open::Enum for Role {
        fn as_str(&self) -> &str {
            self.as_str()
        }

        fn from_string(value: ::std::string::String) -> Self {
            match value.as_str() {
                "owner" => Self::Owner,
                "viewer" => Self::Viewer,
                "editor" => Self::Editor,
                "archivist" => Self::Archivist,
                _ => Self::UnknownValue(value),
            }
        }
    }

    impl ::std::convert::From<&str> for Role {
        fn from(value: &str) -> Self {
            <Self as ::crucible_client_runtime_rust::__generated::open::Enum>::from_string(value.to_owned())
        }
    }

    impl ::std::convert::From<::std::string::String> for Role {
        fn from(value: ::std::string::String) -> Self {
            <Self as ::crucible_client_runtime_rust::__generated::open::Enum>::from_string(value)
        }
    }

    impl ::std::fmt::Display for Role {
        fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
            f.write_str(self.as_str())
        }
    }

    impl ::crucible_client_runtime_rust::__generated::serde::Serialize for Role {
        fn serialize<S: ::crucible_client_runtime_rust::__generated::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {
            ::crucible_client_runtime_rust::__generated::open::serialize_enum(self, serializer)
        }
    }

    impl<'de> ::crucible_client_runtime_rust::__generated::serde::Deserialize<'de> for Role {
        fn deserialize<D: ::crucible_client_runtime_rust::__generated::serde::Deserializer<'de>>(deserializer: D) -> ::std::result::Result<Self, D::Error> {
            ::crucible_client_runtime_rust::__generated::open::deserialize_enum(deserializer)
        }
    }

    /// The service's scopes. Open: a value this client does not know is
    /// kept as `UnknownValue` and written back unchanged. Ordered as the service
    /// lists them (`UnknownValue` last), not by value.
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[non_exhaustive]
    pub enum Scope {
        /// The workspace and everything in it.
        ///
        /// `workspace`.
        Workspace,
        /// Every note in the workspace, including notes written later.
        ///
        /// `allNotes`.
        AllNotes,
        /// One note.
        ///
        /// `note`.
        Note,
        /// A value this client does not know.
        UnknownValue(::std::string::String),
    }

    impl Scope {
        /// Every value this client knows, in catalogue order.
        pub const ALL: &'static [Self] = &[Self::Workspace, Self::AllNotes, Self::Note];

        /// The value, as bindings store it.
        pub fn as_str(&self) -> &str {
            match self {
                Self::Workspace => "workspace",
                Self::AllNotes => "allNotes",
                Self::Note => "note",
                Self::UnknownValue(value) => value,
            }
        }
    }

    impl ::crucible_client_runtime_rust::__generated::open::Enum for Scope {
        fn as_str(&self) -> &str {
            self.as_str()
        }

        fn from_string(value: ::std::string::String) -> Self {
            match value.as_str() {
                "workspace" => Self::Workspace,
                "allNotes" => Self::AllNotes,
                "note" => Self::Note,
                _ => Self::UnknownValue(value),
            }
        }
    }

    impl ::std::convert::From<&str> for Scope {
        fn from(value: &str) -> Self {
            <Self as ::crucible_client_runtime_rust::__generated::open::Enum>::from_string(value.to_owned())
        }
    }

    impl ::std::convert::From<::std::string::String> for Scope {
        fn from(value: ::std::string::String) -> Self {
            <Self as ::crucible_client_runtime_rust::__generated::open::Enum>::from_string(value)
        }
    }

    impl ::std::fmt::Display for Scope {
        fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
            f.write_str(self.as_str())
        }
    }

    impl ::crucible_client_runtime_rust::__generated::serde::Serialize for Scope {
        fn serialize<S: ::crucible_client_runtime_rust::__generated::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {
            ::crucible_client_runtime_rust::__generated::open::serialize_enum(self, serializer)
        }
    }

    impl<'de> ::crucible_client_runtime_rust::__generated::serde::Deserialize<'de> for Scope {
        fn deserialize<D: ::crucible_client_runtime_rust::__generated::serde::Deserializer<'de>>(deserializer: D) -> ::std::result::Result<Self, D::Error> {
            ::crucible_client_runtime_rust::__generated::open::deserialize_enum(deserializer)
        }
    }

    /// Every role, in the service's catalogue order: what a role picker
    /// offers.
    /// The service also accepts custom roles, which are not listed.
    pub static ROLES: &[::crucible_client_runtime_rust::__generated::roles::RoleDescription] = &[
        ::crucible_client_runtime_rust::__generated::roles::RoleDescription {
            value: "owner",
            description: "Runs the workspace.",
            scopes: &["workspace"],
            holders: &["user"],
            assigned_by: &["owner"],
            permissions: &[::crucible_client_runtime_rust::__generated::roles::RolePermission { resource_type: "note", permission: "note:read" }, ::crucible_client_runtime_rust::__generated::roles::RolePermission { resource_type: "note", permission: "note:write" }, ::crucible_client_runtime_rust::__generated::roles::RolePermission { resource_type: "note", permission: "note:create" }, ::crucible_client_runtime_rust::__generated::roles::RolePermission { resource_type: "workspace", permission: "workspace:read" }, ::crucible_client_runtime_rust::__generated::roles::RolePermission { resource_type: "workspace", permission: "workspace:admin" }],
        },
        ::crucible_client_runtime_rust::__generated::roles::RoleDescription {
            value: "viewer",
            description: "Reads notes.",
            scopes: &["workspace", "allNotes", "note"],
            holders: &[],
            assigned_by: &[],
            permissions: &[::crucible_client_runtime_rust::__generated::roles::RolePermission { resource_type: "note", permission: "note:read" }],
        },
        ::crucible_client_runtime_rust::__generated::roles::RoleDescription {
            value: "editor",
            description: "Reads, changes and writes notes.",
            scopes: &["allNotes", "note"],
            holders: &[],
            assigned_by: &[],
            permissions: &[::crucible_client_runtime_rust::__generated::roles::RolePermission { resource_type: "note", permission: "note:read" }, ::crucible_client_runtime_rust::__generated::roles::RolePermission { resource_type: "note", permission: "note:write" }, ::crucible_client_runtime_rust::__generated::roles::RolePermission { resource_type: "note", permission: "note:create" }],
        },
        ::crucible_client_runtime_rust::__generated::roles::RoleDescription {
            value: "archivist",
            description: "Edits notes and reads the archives they came from.",
            scopes: &["allNotes"],
            holders: &[],
            assigned_by: &[],
            permissions: &[::crucible_client_runtime_rust::__generated::roles::RolePermission { resource_type: "note", permission: "note:read" }, ::crucible_client_runtime_rust::__generated::roles::RolePermission { resource_type: "note", permission: "note:write" }, ::crucible_client_runtime_rust::__generated::roles::RolePermission { resource_type: "note", permission: "note:create" }],
        },
    ];
}

use std::fmt;

use super::{MissingResource, ResourceInfo, Target};
use crate::error::Error;

/// One resource instance a check is made against: the resource and its
/// identifiers, root to leaf, so the parent chain (the tenant at its root)
/// travels with it. The leaf may be [`Target::All`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceRef {
    resource: &'static ResourceInfo,
    identifiers: Vec<Target>,
}

impl ResourceRef {
    /// `resource` named by `identifiers`, root to leaf: one per identifier
    /// of the resource. A check whose count differs is refused by the
    /// `Permissions` interceptor as a wiring error.
    pub fn new<I>(resource: &'static ResourceInfo, identifiers: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<Target>,
    {
        Self {
            resource,
            identifiers: identifiers.into_iter().map(Into::into).collect(),
        }
    }

    /// Every instance of `resource` under `parents` (root to leaf: every
    /// identifier but the resource's own): `Inbox(*)` in a workspace.
    pub fn all<I>(resource: &'static ResourceInfo, parents: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<Target>,
    {
        let mut reference = Self::new(resource, parents);
        reference.identifiers.push(Target::All);
        reference
    }

    pub fn resource(&self) -> &'static ResourceInfo {
        self.resource
    }

    /// Root to leaf.
    pub fn identifiers(&self) -> &[Target] {
        &self.identifiers
    }

    /// The identifier the resource calls `name` (`"workspaceId"`).
    pub fn identifier(&self, name: &str) -> Option<&Target> {
        self.resource
            .identifiers
            .iter()
            .position(|identifier| *identifier == name)
            .and_then(|index| self.identifiers.get(index))
    }

    /// Whether every identifier of the resource is bound, once, with only
    /// the leaf possibly `All`.
    pub fn is_complete(&self) -> bool {
        let Some((_, parents)) = self.identifiers.split_last() else {
            return false;
        };
        self.identifiers.len() == self.resource.identifiers.len()
            && parents.iter().all(|target| *target != Target::All)
    }

    /// Whether nothing of the request is bound: a root resource with `*`.
    /// There is nothing to hide then, so a Hidden verdict on it is Denied.
    pub fn binds_nothing(&self) -> bool {
        matches!(self.identifiers.as_slice(), [Target::All])
    }

    /// The 404 for this instance, identical whether it is hidden or
    /// genuinely missing: a [`MissingResource`], which the server answers as
    /// the framework's NotFound, or as the service's own not-found error
    /// when the model designates one.
    pub fn not_found(&self) -> Error {
        Error::new(MissingResource::new(self.clone()))
    }
}

/// `inbox(w-1, i-1)`, for logs.
impl fmt::Display for ResourceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(", self.resource.name)?;
        for (index, target) in self.identifiers.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{target}")?;
        }
        f.write_str(")")
    }
}

#[cfg(test)]
mod tests {
    use super::{MissingResource, ResourceInfo, ResourceRef, Target};
    use crate::error::FrameworkError;

    static INBOX: ResourceInfo = ResourceInfo {
        name: "inbox",
        identifiers: &["workspaceId", "inboxId"],
        not_found: "No such inbox.",
    };
    static WORKSPACE: ResourceInfo = ResourceInfo {
        name: "workspace",
        identifiers: &["workspaceId"],
        not_found: "No such workspace.",
    };

    #[test]
    fn a_reference_binds_every_identifier_root_to_leaf() {
        let workspace = "w-1".to_owned();
        let inbox = ResourceRef::new(&INBOX, [&workspace, &"i-1".to_owned()]);
        assert!(inbox.is_complete());
        assert_eq!(inbox.identifier("inboxId"), Some(&Target::from("i-1")));
        assert_eq!(inbox.identifier("domainId"), None);
        assert_eq!(inbox.to_string(), "inbox(w-1, i-1)");

        let every = ResourceRef::all(&INBOX, [&workspace]);
        assert!(every.is_complete());
        assert_eq!(every.identifiers()[1], Target::All);
        assert_eq!(every.to_string(), "inbox(w-1, *)");
        assert!(!every.binds_nothing());

        assert!(!ResourceRef::new(&INBOX, ["w-1"]).is_complete());
        assert!(!ResourceRef::new(&INBOX, [Target::All, Target::from("i")]).is_complete());
        assert!(ResourceRef::all(&WORKSPACE, Vec::<String>::new()).binds_nothing());
    }

    #[test]
    fn not_found_names_only_the_resource_type() {
        let error = ResourceRef::new(&INBOX, ["w-1", "secret-id"]).not_found();
        let missing = error.downcast_ref::<MissingResource>().unwrap();
        assert_eq!(
            missing.reference().identifiers()[1],
            Target::from("secret-id")
        );
        assert_eq!(error.to_string(), "No such inbox.");
        let framework = missing.framework_error();
        assert_eq!(framework, FrameworkError::NotFound { resource: &INBOX });
        assert!(!framework.message().contains("secret-id"));
    }
}

//! The static description of a model's permissions, as generated code
//! states it: the resources that declare permissions, each permission, and
//! what each operation requires (`Operation::REQUIREMENTS`), plus the
//! per-request [`Check`]s that `Operation::CHECKS` evaluates from the
//! decoded input. A projection of the IR's `ResourcePermissions` and
//! `Requirements` (docs/permissions.md, sections 6.1 and 6.2); the server
//! runtime's `permissions` module enforces them through a service's
//! `Authorizer`.

mod check;
mod missing_resource;
mod mode;
mod obligation;
mod origin;
mod permission_info;
mod registry;
mod requirement_info;
mod requirements;
mod resource_info;
mod resource_ref;
mod target;
mod tier;

pub use check::Check;
pub use missing_resource::MissingResource;
pub use mode::Mode;
pub use obligation::Obligation;
pub use origin::Origin;
pub use permission_info::PermissionInfo;
pub use registry::{PermissionDescription, ResourcePermissions};
pub use requirement_info::RequirementInfo;
pub use requirements::Requirements;
pub use resource_info::ResourceInfo;
pub use resource_ref::ResourceRef;
pub use target::Target;
pub use tier::Tier;

/// `Operation::CHECKS`: pushes one [`Check`] per `Check` or `Filter` clause
/// that applies to `input`, in clause order.
pub type Checks<I> = fn(&I, &mut Vec<Check>);

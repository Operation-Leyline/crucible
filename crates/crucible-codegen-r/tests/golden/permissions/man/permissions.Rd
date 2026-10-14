\name{permissions}
\alias{permissions}
\alias{PERMISSIONS}
\alias{GraphPermission}
\alias{graph_permission_canonical}
\alias{HistoryPermission}
\alias{history_permission_canonical}
\alias{OrganizationPermission}
\alias{organization_permission_canonical}
\title{Permissions}
\description{The model's permissions, as the values grants store, and the registry a grant picker reads. Internal permissions never appear.

}
\details{GraphPermission: The permissions \code{Graph} accepts (\code{graph}), by the values grants store. A permission value is a character string: a known one, or any string a newer server writes, kept as it is.

GraphPermission$read = graph:read: Read a graph and its catalog.

GraphPermission$write = graph:write: Change a graph: */ ends no comment and [bare] links nowhere.
@param is not a tag.

GraphPermission$audit = graph:audit: Known permission.

GraphPermission$view = graph:view: An alias of \code{graph:read}.

Deprecated: Renamed to graph:read.

graph_permission_canonical(value): Returns the permission each element of \code{value}, a character vector, stands for on \code{Graph}: a deprecated alias's primary, or the element itself, unknown values included.

HistoryPermission: The permissions \code{History} accepts (\code{history}), by the values grants store. A permission value is a character string: a known one, or any string a newer server writes, kept as it is.

HistoryPermission$read = history:read: Known permission.

HistoryPermission$restore = history:restore: Known permission.

history_permission_canonical(value): Returns the permission each element of \code{value}, a character vector, stands for on \code{History}: a deprecated alias's primary, or the element itself, unknown values included.

OrganizationPermission: The permissions \code{Organization} accepts (\code{org}), by the values grants store. A permission value is a character string: a known one, or any string a newer server writes, kept as it is.

OrganizationPermission$admin = organization:admin: Everything in the organization.

organization_permission_canonical(value): Returns the permission each element of \code{value}, a character vector, stands for on \code{Organization}: a deprecated alias's primary, or the element itself, unknown values included.

PERMISSIONS: Every permission this client knows, by resource type, deprecated aliases included: a list of \code{resource_type} and \code{permissions}, each permission a list of \code{value}, \code{description}, \code{tier} ("public", "permissionOnly" or "notGrantable"; a string, since a newer server may add tiers), \code{grantable} (FALSE for a bundle the service confers whole: never offer it in a picker), \code{deprecated} and \code{alias_of} (a deprecated alias's primary, or NULL).

}

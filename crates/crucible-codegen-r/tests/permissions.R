# The typed permissions of tests/fixtures/permissions as an R consumer sees
# them (docs/permissions.md §6.3): known values, canonical functions, and the
# registry a grant picker reads.
args <- commandArgs(TRUE)
library(leyline.client, lib.loc = args[1L])
check <- function(value, label) if (!isTRUE(value)) stop(label, call. = FALSE)

check(identical(GraphPermission$read, 'graph:read'), 'Known constant')
check(identical(OrganizationPermission$admin, 'organization:admin'), 'Known constant of another type')

# A grant a newer server wrote, or one naming an internal permission this
# client never sees, is still a permission value, kept as it is through JSON.
text <- '["graph:read","graph:export","organization:read"]'
stored <- unlist(json_parse(text))
check(identical(stored, c('graph:read', 'graph:export', 'organization:read')), 'Unknown values decode')
check(identical(json_encode(as.list(stored)), text), 'Unknown values round trip')
check(identical(graph_permission_canonical(stored), stored), 'Canonical keeps unknown values')
check(identical(graph_permission_canonical(c(a = 'graph:export', b = NA)), c(a = 'graph:export', b = NA)), 'Canonical keeps names and NA')

# A deprecated alias keeps its value and canonicalizes to its primary.
check(identical(GraphPermission$view, 'graph:view'), 'Alias value')
check(identical(graph_permission_canonical(GraphPermission$view), GraphPermission$read), 'Alias canonical')
check(identical(graph_permission_canonical(c('graph:view', 'graph:write', 'graph:view')), c('graph:read', 'graph:write', 'graph:read')), 'Canonical is vectorized')
check(identical(history_permission_canonical('graph:view'), 'graph:view'), 'Another type has no such alias')
check(identical(graph_permission_canonical(character()), character()), 'Empty vector')
check(inherits(tryCatch(graph_permission_canonical(1), error = identity), 'error'), 'Canonical refuses non-character')

# The registry: resource types sorted, no internal permission.
types <- vapply(PERMISSIONS, function(resource) resource$resource_type, character(1))
check(identical(types, c('graph', 'history', 'org')), 'Resource types')
entries <- unlist(lapply(PERMISSIONS, function(resource) resource$permissions), recursive = FALSE)
values <- vapply(entries, function(entry) entry$value, character(1))
check(!('organization:read' %in% values), 'No internal permission')
find <- function(value) entries[[match(value, values)]]
admin <- find('organization:admin')
check(identical(admin$grantable, FALSE) && identical(admin$tier, 'notGrantable'), 'notGrantable admin')
alias <- find('graph:view')
check(identical(alias$alias_of, 'graph:read') && identical(alias$deprecated, TRUE), 'Alias entry')
check(is.null(find('graph:audit')$alias_of) && identical(find('graph:audit')$tier, 'permissionOnly'), 'permissionOnly entry')

# A grant picker offers the grantable permissions that are not deprecated.
offered <- Filter(function(entry) entry$grantable && !entry$deprecated, entries)
check(identical(vapply(offered, function(entry) entry$value, character(1)),
  c('graph:audit', 'graph:read', 'graph:write', 'history:read', 'history:restore')), 'Grant picker')
cat('R typed permissions passed\n')

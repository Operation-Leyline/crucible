# The role registry of tests/models/roles-granted-by as an R consumer sees
# it (docs/roles.md §7 "Clients"): known role and scope values, and the
# registry a role picker reads, in catalogue order with public roles,
# scopes and permissions only.
args <- commandArgs(TRUE)
library(notes.client, lib.loc = args[1L])
check <- function(value, label) if (!isTRUE(value)) stop(label, call. = FALSE)

check(identical(NotesRole$owner, 'owner'), 'Known role')
check(identical(NotesScope$all_notes, 'allNotes'), 'Known scope')

# A binding a newer server wrote, a custom role, or one naming an internal
# role is still a role value, kept as it is through JSON.
text <- '["viewer","auditor","custom/reviewer"]'
stored <- unlist(json_parse(text))
check(identical(stored, c('viewer', 'auditor', 'custom/reviewer')), 'Unknown values decode')
check(identical(json_encode(as.list(stored)), text), 'Unknown values round trip')

values <- vapply(ROLES, function(role) role$value, character(1))
check(identical(values, c('owner', 'viewer', 'editor', 'archivist')), 'Catalogue order')
owner <- ROLES[[1L]]
check(identical(owner$scopes, 'workspace'), 'Owner scopes')
check(identical(owner$holders, 'user'), 'Owner holders')
check(identical(owner$assigned_by, 'owner'), 'Owner assigners')
granted <- vapply(owner$permissions, function(p) paste(p$resource_type, p$permission), character(1))
check(identical(granted, c('note note:read', 'note note:write', 'note note:create',
  'workspace workspace:read', 'workspace workspace:admin')), 'Owner permissions')
viewer <- ROLES[[2L]]
check(identical(viewer$scopes, c('workspace', 'allNotes', 'note')), 'Viewer scopes')
check(identical(viewer$holders, character()) && identical(viewer$assigned_by, character()), 'Empty lists')
all <- unlist(lapply(ROLES, function(role) vapply(role$permissions, function(p) p$permission, character(1))))
check(!('note:purge' %in% all) && !('workspace:audit' %in% all), 'No internal permission')
cat('R roles passed\n')

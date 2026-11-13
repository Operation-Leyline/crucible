\name{roles}
\alias{roles}
\alias{ROLES}
\alias{NotesRole}
\alias{NotesScope}
\title{Roles}
\description{The model's roles and the scopes they bind at, as the values bindings store, and the registry a role picker reads. Internal roles, scopes and permissions never appear.

}
\details{NotesRole: The role values of \code{Notes}, by the values bindings store. A role value is a character string: a known one, or any string a newer server writes, kept as it is.

NotesRole$owner = owner: Runs the workspace.

NotesRole$viewer = viewer: Reads notes.

NotesRole$editor = editor: Reads, changes and writes notes.

NotesRole$archivist = archivist: Edits notes and reads the archives they came from.

NotesScope: The scope values of \code{Notes}, by the values bindings store. A scope value is a character string: a known one, or any string a newer server writes, kept as it is.

NotesScope$workspace = workspace: The workspace and everything in it.

NotesScope$all_notes = allNotes: Every note in the workspace, including notes written later.

NotesScope$note = note: One note.

ROLES: Every role, in the service's catalogue order: what a role picker offers. The service also accepts custom roles, which are not listed. A list of roles, each a list of \code{value} (what bindings store), \code{description}, \code{scopes} (the scope values it may be bound at), \code{holders} (the kinds of principal that may hold it; empty: any), \code{assigned_by} (the roles whose holders alone may assign it; empty: whoever may write bindings) and \code{permissions} (what it grants, each a list of \code{resource_type} and \code{permission}).

}

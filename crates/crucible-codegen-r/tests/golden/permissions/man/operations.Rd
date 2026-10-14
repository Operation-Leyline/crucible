\name{operations}
\alias{operations}
\title{Operation contracts}
\description{archive_graph
A mixed list: clients see the public clause and that more are internal,
never the internal clause or the internal member its \code{when} reads.

Requires \code{graph:write} on the graph; and further permissions internal to the service.

Input: ArchiveGraphInput.

Returns: ArchiveGraphOutput.

Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.

create_graph
Call CreateGraph.

Requires \code{graph:write} on every graph in the organization.

Input: CreateGraphInput.

Returns: CreateGraphOutput.

Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.

execute_query
Call ExecuteQuery.

Requires \code{graph:read} on the graph; \code{graph:write} on the graph, checked by the service (only when the parsed query writes); \code{history:read} on the history, when \code{has target.history}.

Input: ExecuteQueryInput.

Returns: ExecuteQueryOutput.

Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.

get_graph
Call GetGraph.

Requires \code{graph:read} on the graph.

Input: GetGraphInput.

Returns: GetGraphOutput.

Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.

This operation is safe to retry under the configured retry policy.

get_organization
Call GetOrganization.

Requires permissions internal to the service.

Input: GetOrganizationInput.

Returns: GetOrganizationOutput.

Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.

This operation is safe to retry under the configured retry policy.

get_user
Answers about the caller's own credential.

Requires no permission.

Input: no modeled input.

Returns: GetUserOutput.

Modeled errors: ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.

This operation is safe to retry under the configured retry policy.

health
No authentication: nothing to authorize.

Input: no modeled input.

Returns: HealthOutput.

Modeled errors: ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.

This operation is safe to retry under the configured retry policy.

list_graphs
Returns only the graphs the caller may read.

Returns only the graphs the caller holds \code{graph:read} on in the organization.

Input: ListGraphsInput.

Returns: ListGraphsOutput.

Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.

This operation is safe to retry under the configured retry policy.

restore_backup
Call RestoreBackup.

Input: RestoreBackupInput.

Returns: RestoreBackupOutput.

Modeled errors: ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.

rewrite_graph
Hostile documentation: a backtick in a \code{when} literal, and handler
clause documentation that must keep its case, its paragraphs and its
code spans.

Requires \code{graph:read} on the graph, when \verb{mode == "a`b"}; \code{graph:write} on the graph, checked by the service (HTTP rewrites only, when \code{mode} is \verb{a`b}, and [ops] approve); \code{history:read} on the history, checked by the service (see below).

\code{history:read} on the history, checked by the service:

GraphQL plans are checked twice:

\itemize{
\item once when parsed, against */ text;
\item once when run, see [bare] and \href{https://example.com/guide}{the guide}.
}@see the runbook; @param is no tag either.

Input: RewriteGraphInput.

Returns: RewriteGraphOutput.

Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.

}

#' @description
#' Organizations, their graphs and the graphs' histories.
#' Construct with client_config(). Call client$close() after use. Clients and page functions are mutable; use them from one R execution thread.
#' @md
# Generated native operation methods.
`LeylineClient` <- function(config = client_config()) {
  client <- .crucible_client(config, NULL, TRUE)
#' @description
#' Call CreateGraph.
#'
#' Requires `graph:write` on every graph in the organization.
#'
#' Input: CreateGraphInput.
#'
#' Returns: CreateGraphOutput.
#'
#' Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
#' @md
  client$`create_graph` <- function(input, cancellation = NULL) client$call("CreateGraph", input, cancellation)
#' @description
#' Call ExecuteQuery.
#'
#' Requires `graph:read` on the graph; `graph:write` on the graph, checked by the service (only when the parsed query writes); `history:read` on the history, when `has target.history`.
#'
#' Input: ExecuteQueryInput.
#'
#' Returns: ExecuteQueryOutput.
#'
#' Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
#' @md
  client$`execute_query` <- function(input, cancellation = NULL) client$call("ExecuteQuery", input, cancellation)
#' @description
#' Call GetGraph.
#'
#' Requires `graph:read` on the graph.
#'
#' Input: GetGraphInput.
#'
#' Returns: GetGraphOutput.
#'
#' Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
#'
#' This operation is safe to retry under the configured retry policy.
#' @md
  client$`get_graph` <- function(input, cancellation = NULL) client$call("GetGraph", input, cancellation)
#' @description
#' Call GetOrganization.
#'
#' Input: GetOrganizationInput.
#'
#' Returns: GetOrganizationOutput.
#'
#' Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
#'
#' This operation is safe to retry under the configured retry policy.
#' @md
  client$`get_organization` <- function(input, cancellation = NULL) client$call("GetOrganization", input, cancellation)
#' @description
#' Answers about the caller's own credential.
#'
#' Requires no permission.
#'
#' Input: no modeled input.
#'
#' Returns: GetUserOutput.
#'
#' Modeled errors: ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
#'
#' This operation is safe to retry under the configured retry policy.
#' @md
  client$`get_user` <- function(input = NULL, cancellation = NULL) client$call("GetUser", input, cancellation)
#' @description
#' No authentication: nothing to authorize.
#'
#' Input: no modeled input.
#'
#' Returns: HealthOutput.
#'
#' Modeled errors: ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
#'
#' This operation is safe to retry under the configured retry policy.
#' @md
  client$`health` <- function(input = NULL, cancellation = NULL) client$call("Health", input, cancellation)
#' @description
#' Returns only the graphs the caller may read.
#'
#' Returns only the graphs the caller holds `graph:read` on in the organization.
#'
#' Input: ListGraphsInput.
#'
#' Returns: ListGraphsOutput.
#'
#' Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
#'
#' This operation is safe to retry under the configured retry policy.
#' @md
  client$`list_graphs` <- function(input, cancellation = NULL) client$call("ListGraphs", input, cancellation)
#' @description
#' Call RestoreBackup.
#'
#' Input: RestoreBackupInput.
#'
#' Returns: RestoreBackupOutput.
#'
#' Modeled errors: ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
#' @md
  client$`restore_backup` <- function(input, cancellation = NULL) client$call("RestoreBackup", input, cancellation)
#' @description
#' Hostile documentation: a backtick in a `when` literal, and handler
#' clause documentation that must keep its case, its paragraphs and its
#' code spans.
#'
#' Requires `graph:read` on the graph, when ``mode == "a`b"``; `graph:write` on the graph, checked by the service (HTTP rewrites only, when `mode` is ``a`b``, and \[ops] approve); `history:read` on the history, checked by the service (see below).
#'
#' `history:read` on the history, checked by the service:
#'
#' GraphQL plans are checked twice:
#'
#' - once when parsed, against */ text;
#' - once when run, see \[bare] and [the guide](https://example.com/guide).
#'
#' \@@see the runbook; @@param is no tag either.
#'
#' Input: RewriteGraphInput.
#'
#' Returns: RewriteGraphOutput.
#'
#' Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
#' @md
  client$`rewrite_graph` <- function(input, cancellation = NULL) client$call("RewriteGraph", input, cancellation)
  client
}

#ifndef LEYLINE_CLIENT_H
#define LEYLINE_CLIENT_H
#include "models.h"
#include "runtime/client_runtime.h"

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Organizations, their graphs and the graphs' histories.
 */
/**
 * Construct with client_defaults; free after calls and iterators finish. Concurrent calls require thread-safe callbacks and borrowed user data.
 */
leyline_status leyline_client_new(const leyline_client_config *config, leyline_client **output, leyline_error *error);

/** Zero-initialize results. Free before reuse; value borrows base.arena. */
typedef struct { leyline_result base; const leyline_models_archive_graph_output *value; } leyline_operations_archive_graph_result;
/**
 * A mixed list: clients see the public clause and that more are internal,
 * never the internal clause or the internal member its `when` reads.
 *
 * Requires `graph:write` on the graph; and further permissions internal to the service.
 *
 * Input: ArchiveGraphInput.
 *
 * Returns: ArchiveGraphOutput.
 *
 * Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
 */
leyline_status leyline_operations_archive_graph(leyline_client *client, const leyline_models_archive_graph_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_archive_graph_result *result, leyline_error *error);
void leyline_operations_archive_graph_result_free(leyline_operations_archive_graph_result *result);

/** Zero-initialize results. Free before reuse; value borrows base.arena. */
typedef struct { leyline_result base; const leyline_models_create_graph_output *value; } leyline_operations_create_graph_result;
/**
 * Call CreateGraph.
 *
 * Requires `graph:write` on every graph in the organization.
 *
 * Input: CreateGraphInput.
 *
 * Returns: CreateGraphOutput.
 *
 * Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
 */
leyline_status leyline_operations_create_graph(leyline_client *client, const leyline_models_create_graph_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_create_graph_result *result, leyline_error *error);
void leyline_operations_create_graph_result_free(leyline_operations_create_graph_result *result);

/** Zero-initialize results. Free before reuse; value borrows base.arena. */
typedef struct { leyline_result base; const leyline_models_execute_query_output *value; } leyline_operations_execute_query_result;
/**
 * Call ExecuteQuery.
 *
 * Requires `graph:read` on the graph; `graph:write` on the graph, checked by the service (only when the parsed query writes); `history:read` on the history, when `has target.history`.
 *
 * Input: ExecuteQueryInput.
 *
 * Returns: ExecuteQueryOutput.
 *
 * Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
 */
leyline_status leyline_operations_execute_query(leyline_client *client, const leyline_models_execute_query_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_execute_query_result *result, leyline_error *error);
void leyline_operations_execute_query_result_free(leyline_operations_execute_query_result *result);

/** Zero-initialize results. Free before reuse; value borrows base.arena. */
typedef struct { leyline_result base; const leyline_models_get_graph_output *value; } leyline_operations_get_graph_result;
/**
 * Call GetGraph.
 *
 * Requires `graph:read` on the graph.
 *
 * Input: GetGraphInput.
 *
 * Returns: GetGraphOutput.
 *
 * Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
 *
 * This operation is safe to retry under the configured retry policy.
 */
leyline_status leyline_operations_get_graph(leyline_client *client, const leyline_models_get_graph_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_get_graph_result *result, leyline_error *error);
void leyline_operations_get_graph_result_free(leyline_operations_get_graph_result *result);

/** Zero-initialize results. Free before reuse; value borrows base.arena. */
typedef struct { leyline_result base; const leyline_models_get_organization_output *value; } leyline_operations_get_organization_result;
/**
 * Call GetOrganization.
 *
 * Requires permissions internal to the service.
 *
 * Input: GetOrganizationInput.
 *
 * Returns: GetOrganizationOutput.
 *
 * Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
 *
 * This operation is safe to retry under the configured retry policy.
 */
leyline_status leyline_operations_get_organization(leyline_client *client, const leyline_models_get_organization_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_get_organization_result *result, leyline_error *error);
void leyline_operations_get_organization_result_free(leyline_operations_get_organization_result *result);

/** Zero-initialize results. Free before reuse; value borrows base.arena. */
typedef struct { leyline_result base; const leyline_models_get_user_output *value; } leyline_operations_get_user_result;
/**
 * Answers about the caller's own credential.
 *
 * Requires no permission.
 *
 * Input: no modeled input.
 *
 * Returns: GetUserOutput.
 *
 * Modeled errors: ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
 *
 * This operation is safe to retry under the configured retry policy.
 */
leyline_status leyline_operations_get_user(leyline_client *client, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_get_user_result *result, leyline_error *error);
void leyline_operations_get_user_result_free(leyline_operations_get_user_result *result);

/** Zero-initialize results. Free before reuse; value borrows base.arena. */
typedef struct { leyline_result base; const leyline_models_health_output *value; } leyline_operations_health_result;
/**
 * No authentication: nothing to authorize.
 *
 * Input: no modeled input.
 *
 * Returns: HealthOutput.
 *
 * Modeled errors: ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
 *
 * This operation is safe to retry under the configured retry policy.
 */
leyline_status leyline_operations_health(leyline_client *client, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_health_result *result, leyline_error *error);
void leyline_operations_health_result_free(leyline_operations_health_result *result);

/** Zero-initialize results. Free before reuse; value borrows base.arena. */
typedef struct { leyline_result base; const leyline_models_list_graphs_output *value; } leyline_operations_list_graphs_result;
/**
 * Returns only the graphs the caller may read.
 *
 * Returns only the graphs the caller holds `graph:read` on in the organization.
 *
 * Input: ListGraphsInput.
 *
 * Returns: ListGraphsOutput.
 *
 * Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
 *
 * This operation is safe to retry under the configured retry policy.
 */
leyline_status leyline_operations_list_graphs(leyline_client *client, const leyline_models_list_graphs_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_list_graphs_result *result, leyline_error *error);
void leyline_operations_list_graphs_result_free(leyline_operations_list_graphs_result *result);

/** Zero-initialize results. Free before reuse; value borrows base.arena. */
typedef struct { leyline_result base; const leyline_models_restore_backup_output *value; } leyline_operations_restore_backup_result;
/**
 * Call RestoreBackup.
 *
 * Input: RestoreBackupInput.
 *
 * Returns: RestoreBackupOutput.
 *
 * Modeled errors: ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
 */
leyline_status leyline_operations_restore_backup(leyline_client *client, const leyline_models_restore_backup_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_restore_backup_result *result, leyline_error *error);
void leyline_operations_restore_backup_result_free(leyline_operations_restore_backup_result *result);

/** Zero-initialize results. Free before reuse; value borrows base.arena. */
typedef struct { leyline_result base; const leyline_models_rewrite_graph_output *value; } leyline_operations_rewrite_graph_result;
/**
 * Hostile documentation: a backtick in a `when` literal, and handler
 * clause documentation that must keep its case, its paragraphs and its
 * code spans.
 *
 * Requires `graph:read` on the graph, when ``mode == "a`b"``; `graph:write` on the graph, checked by the service (HTTP rewrites only, when `mode` is ``a`b``, and &#92;[ops] approve); `history:read` on the history, checked by the service (see below).
 *
 * `history:read` on the history, checked by the service:
 *
 * GraphQL plans are checked twice:
 *
 * - once when parsed, against *&#47; text;
 * - once when run, see &#92;[bare] and [the guide](https://example.com/guide).
 *
 * &#92;&#64;see the runbook; &#64;param is no tag either.
 *
 * Input: RewriteGraphInput.
 *
 * Returns: RewriteGraphOutput.
 *
 * Modeled errors: NotFoundException, ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
 */
leyline_status leyline_operations_rewrite_graph(leyline_client *client, const leyline_models_rewrite_graph_input *input, leyline_cancelled_fn cancelled, void *cancel_user, leyline_operations_rewrite_graph_result *result, leyline_error *error);
void leyline_operations_rewrite_graph_result_free(leyline_operations_rewrite_graph_result *result);
const leyline_models_internal_server_exception *leyline_error_as_internal_server_exception(const leyline_result *result);
const leyline_models_not_found_exception *leyline_error_as_not_found_exception(const leyline_result *result);
const leyline_models_validation_exception *leyline_error_as_validation_exception(const leyline_result *result);

#ifdef __cplusplus
}
#endif
#endif

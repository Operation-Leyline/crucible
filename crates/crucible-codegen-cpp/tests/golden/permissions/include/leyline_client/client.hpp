#pragma once
#include "models.hpp"
#include "runtime/client_runtime.hpp"

namespace leyline {
/**
 * Organizations, their graphs and the graphs' histories.
 * Construct with ClientConfig. Concurrent calls require thread-safe callbacks and transport. Copies share connections and close state; close after calls finish.
 */
class LeylineClient {
    std::shared_ptr<runtime::ClientRuntime> runtime_;
public:
    /** Creates a client; copies share runtime and connections. */
    explicit LeylineClient(runtime::ClientConfig config = {});
    /** Closes the shared runtime; finish active calls first. */
    void close() { runtime_->close(); }
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
    [[nodiscard]] ::leyline::models::ArchiveGraphOutput archive_graph(const ::leyline::models::ArchiveGraphInput& input, std::stop_token stop = {}) const;
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
    [[nodiscard]] ::leyline::models::CreateGraphOutput create_graph(const ::leyline::models::CreateGraphInput& input, std::stop_token stop = {}) const;
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
    [[nodiscard]] ::leyline::models::ExecuteQueryOutput execute_query(const ::leyline::models::ExecuteQueryInput& input, std::stop_token stop = {}) const;
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
    [[nodiscard]] ::leyline::models::GetGraphOutput get_graph(const ::leyline::models::GetGraphInput& input, std::stop_token stop = {}) const;
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
    [[nodiscard]] ::leyline::models::GetOrganizationOutput get_organization(const ::leyline::models::GetOrganizationInput& input, std::stop_token stop = {}) const;
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
    [[nodiscard]] ::leyline::models::GetUserOutput get_user(std::stop_token stop = {}) const;
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
    [[nodiscard]] ::leyline::models::HealthOutput health(std::stop_token stop = {}) const;
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
    [[nodiscard]] ::leyline::models::ListGraphsOutput list_graphs(const ::leyline::models::ListGraphsInput& input, std::stop_token stop = {}) const;
    /**
     * Call RestoreBackup.
     *
     * Input: RestoreBackupInput.
     *
     * Returns: RestoreBackupOutput.
     *
     * Modeled errors: ValidationException, InternalServerException. Transport, timeout and decoding failures can also occur.
     */
    [[nodiscard]] ::leyline::models::RestoreBackupOutput restore_backup(const ::leyline::models::RestoreBackupInput& input, std::stop_token stop = {}) const;
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
    [[nodiscard]] ::leyline::models::RewriteGraphOutput rewrite_graph(const ::leyline::models::RewriteGraphInput& input, std::stop_token stop = {}) const;
};
}

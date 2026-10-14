<?php
declare(strict_types=1);
namespace Example\Weather;

/**
 * Client for the Leyline service.
 *
 * Organizations, their graphs and the graphs' histories.
 *
 * Create one `LeylineClient` per configuration and reuse it: it keeps its HTTP connections open between calls. A client is not safe to share across threads or fibers that run calls at the same time; give each its own. Call `close()` when finished. Every operation blocks until it completes and reports failure as a ServiceException for a service response, a NetworkException when no response arrives, a TimeoutException when the call outlasts `ClientConfig::$timeout`, or a CancelledException.
 */
final class LeylineClient
{
    private readonly Runtime\ClientRuntime $runtime;
    /**
     * Creates a client. Without a `$config`, the generated endpoint is used and no credentials are sent; see ClientConfig for credentials, retries, timeouts and the transport.
     *
     * @throws \InvalidArgumentException when the endpoint or another setting is invalid
     */
    public function __construct(Runtime\ClientConfig $config = new Runtime\ClientConfig())
    {
        $this->runtime = new Runtime\ClientRuntime($config, Schema::codec(), Decoder::error(...), '', true);
    }
    /** Releases the HTTP transport this client created. A transport passed in the configuration is left open. */
    public function close(): void { $this->runtime->close(); }
    /**
     * Not idempotent: not retried after a network failure or server error, since repeating it could repeat its effect. Throttling responses are still retried.
     *
     * Requires `graph:write` on every graph in the organization.
     *
     * @param \Example\Weather\CreateGraphInput $input the request to send
     * @param ?Runtime\CancellationToken $cancellation cancels the call when cancelled
     * @return \Example\Weather\CreateGraphOutput the service's response
     * @throws \Example\Weather\InternalServerException
     * @throws \Example\Weather\NotFoundException
     * @throws \Example\Weather\ValidationException
     * @throws \Example\Weather\LeylineException for any other error response
     */
    public function createGraph(\Example\Weather\CreateGraphInput $input, ?Runtime\CancellationToken $cancellation = null): \Example\Weather\CreateGraphOutput
    {
        return $this->runtime->call('CreateGraph', $input, $cancellation);
    }
    /**
     * Not idempotent: not retried after a network failure or server error, since repeating it could repeat its effect. Throttling responses are still retried.
     *
     * Requires `graph:read` on the graph; `graph:write` on the graph, checked by the service (only when the parsed query writes); `history:read` on the history, when `has target.history`.
     *
     * @param \Example\Weather\ExecuteQueryInput $input the request to send
     * @param ?Runtime\CancellationToken $cancellation cancels the call when cancelled
     * @return \Example\Weather\ExecuteQueryOutput the service's response
     * @throws \Example\Weather\InternalServerException
     * @throws \Example\Weather\NotFoundException
     * @throws \Example\Weather\ValidationException
     * @throws \Example\Weather\LeylineException for any other error response
     */
    public function executeQuery(\Example\Weather\ExecuteQueryInput $input, ?Runtime\CancellationToken $cancellation = null): \Example\Weather\ExecuteQueryOutput
    {
        return $this->runtime->call('ExecuteQuery', $input, $cancellation);
    }
    /**
     * Read-only: retried automatically after transient failures.
     *
     * Requires `graph:read` on the graph.
     *
     * @param \Example\Weather\GetGraphInput $input the request to send
     * @param ?Runtime\CancellationToken $cancellation cancels the call when cancelled
     * @return \Example\Weather\GetGraphOutput the service's response
     * @throws \Example\Weather\InternalServerException
     * @throws \Example\Weather\NotFoundException
     * @throws \Example\Weather\ValidationException
     * @throws \Example\Weather\LeylineException for any other error response
     */
    public function getGraph(\Example\Weather\GetGraphInput $input, ?Runtime\CancellationToken $cancellation = null): \Example\Weather\GetGraphOutput
    {
        return $this->runtime->call('GetGraph', $input, $cancellation);
    }
    /**
     * Read-only: retried automatically after transient failures.
     *
     * @param \Example\Weather\GetOrganizationInput $input the request to send
     * @param ?Runtime\CancellationToken $cancellation cancels the call when cancelled
     * @return \Example\Weather\GetOrganizationOutput the service's response
     * @throws \Example\Weather\InternalServerException
     * @throws \Example\Weather\NotFoundException
     * @throws \Example\Weather\ValidationException
     * @throws \Example\Weather\LeylineException for any other error response
     */
    public function getOrganization(\Example\Weather\GetOrganizationInput $input, ?Runtime\CancellationToken $cancellation = null): \Example\Weather\GetOrganizationOutput
    {
        return $this->runtime->call('GetOrganization', $input, $cancellation);
    }
    /**
     * Answers about the caller's own credential.
     *
     * Read-only: retried automatically after transient failures.
     *
     * Requires no permission.
     *
     * @param ?Runtime\CancellationToken $cancellation cancels the call when cancelled
     * @return \Example\Weather\GetUserOutput the service's response
     * @throws \Example\Weather\InternalServerException
     * @throws \Example\Weather\ValidationException
     * @throws \Example\Weather\LeylineException for any other error response
     */
    public function getUser(?Runtime\CancellationToken $cancellation = null): \Example\Weather\GetUserOutput
    {
        return $this->runtime->call('GetUser', new Runtime\Unit(), $cancellation);
    }
    /**
     * No authentication: nothing to authorize.
     *
     * Read-only: retried automatically after transient failures.
     *
     * @param ?Runtime\CancellationToken $cancellation cancels the call when cancelled
     * @return \Example\Weather\HealthOutput the service's response
     * @throws \Example\Weather\InternalServerException
     * @throws \Example\Weather\ValidationException
     * @throws \Example\Weather\LeylineException for any other error response
     */
    public function health(?Runtime\CancellationToken $cancellation = null): \Example\Weather\HealthOutput
    {
        return $this->runtime->call('Health', new Runtime\Unit(), $cancellation);
    }
    /**
     * Returns only the graphs the caller may read.
     *
     * Read-only: retried automatically after transient failures.
     *
     * Returns only the graphs the caller holds `graph:read` on in the organization.
     *
     * @param \Example\Weather\ListGraphsInput $input the request to send
     * @param ?Runtime\CancellationToken $cancellation cancels the call when cancelled
     * @return \Example\Weather\ListGraphsOutput the service's response
     * @throws \Example\Weather\InternalServerException
     * @throws \Example\Weather\NotFoundException
     * @throws \Example\Weather\ValidationException
     * @throws \Example\Weather\LeylineException for any other error response
     */
    public function listGraphs(\Example\Weather\ListGraphsInput $input, ?Runtime\CancellationToken $cancellation = null): \Example\Weather\ListGraphsOutput
    {
        return $this->runtime->call('ListGraphs', $input, $cancellation);
    }
    /**
     * Not idempotent: not retried after a network failure or server error, since repeating it could repeat its effect. Throttling responses are still retried.
     *
     * @param \Example\Weather\RestoreBackupInput $input the request to send
     * @param ?Runtime\CancellationToken $cancellation cancels the call when cancelled
     * @return \Example\Weather\RestoreBackupOutput the service's response
     * @throws \Example\Weather\InternalServerException
     * @throws \Example\Weather\ValidationException
     * @throws \Example\Weather\LeylineException for any other error response
     */
    public function restoreBackup(\Example\Weather\RestoreBackupInput $input, ?Runtime\CancellationToken $cancellation = null): \Example\Weather\RestoreBackupOutput
    {
        return $this->runtime->call('RestoreBackup', $input, $cancellation);
    }
    /**
     * Hostile documentation: a backtick in a `when` literal, and handler
     * clause documentation that must keep its case, its paragraphs and its
     * code spans.
     *
     * Not idempotent: not retried after a network failure or server error, since repeating it could repeat its effect. Throttling responses are still retried.
     *
     * Requires `graph:read` on the graph, when ``mode == "a`b"``; `graph:write` on the graph, checked by the service (HTTP rewrites only, when `mode` is ``a`b``, and \[ops] approve); `history:read` on the history, checked by the service (see below).
     *
     * `history:read` on the history, checked by the service:
     *
     * GraphQL plans are checked twice:
     *
     * - once when parsed, against *\/ text;
     * - once when run, see \[bare] and [the guide](https://example.com/guide).
     *
     * \@see the runbook; @param is no tag either.
     *
     * @param \Example\Weather\RewriteGraphInput $input the request to send
     * @param ?Runtime\CancellationToken $cancellation cancels the call when cancelled
     * @return \Example\Weather\RewriteGraphOutput the service's response
     * @throws \Example\Weather\InternalServerException
     * @throws \Example\Weather\NotFoundException
     * @throws \Example\Weather\ValidationException
     * @throws \Example\Weather\LeylineException for any other error response
     */
    public function rewriteGraph(\Example\Weather\RewriteGraphInput $input, ?Runtime\CancellationToken $cancellation = null): \Example\Weather\RewriteGraphOutput
    {
        return $this->runtime->call('RewriteGraph', $input, $cancellation);
    }
}

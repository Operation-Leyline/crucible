# frozen_string_literal: true
module Weather
  # Client for the Leyline service.
  #
  # Organizations, their graphs and the graphs' histories.
  #
  # Create one `LeylineClient` and share it: it is safe to call from many threads at once, and it keeps its HTTP connection open between calls. Call {#close} when finished. Every operation blocks until it completes and reports failure as a `LeylineException` for a service response, a {Runtime::NetworkError} when no response arrives, a {Runtime::DeadlineError} when the call outlasts {Runtime::ClientConfig#timeout}, or a {Runtime::CancelledError}.
  class LeylineClient
    # Creates a client. Without a `config`, the generated endpoint is used and no credentials are sent; see {Runtime::ClientConfig} for credentials, retries, timeouts and the transport.
    #
    # @param config [Runtime::ClientConfig]
    # @raise [ArgumentError] when the endpoint or another setting is invalid
    def initialize(config = Runtime::ClientConfig.new)
      @runtime = Runtime::ClientRuntime.new(config, ::Weather.codec, endpoint: '', digest: true)
    end
    # Releases the HTTP connection this client created. A transport passed in the configuration is left open.
    def close = @runtime.close
    # A mixed list: clients see the public clause and that more are internal,
    # never the internal clause or the internal member its `when` reads.
    #
    # Not idempotent: not retried after a network failure or server error, since repeating it could repeat its effect. Throttling responses are still retried.
    #
    # Requires `graph:write` on the graph; and further permissions internal to the service.
    #
    # @param input [ArchiveGraphInput] the request to send
    # @param cancellation [Runtime::CancellationToken, nil] cancels the call when cancelled
    # @return [ArchiveGraphOutput] the service's response
    # @raise [InternalServerException]
    # @raise [NotFoundException]
    # @raise [ValidationException]
    # @raise [Runtime::LeylineException] for any other error response
    def archive_graph(input, cancellation: nil)
      @runtime.call('ArchiveGraph', input, cancellation: cancellation)
    end
    # Not idempotent: not retried after a network failure or server error, since repeating it could repeat its effect. Throttling responses are still retried.
    #
    # Requires `graph:write` on every graph in the organization.
    #
    # @param input [CreateGraphInput] the request to send
    # @param cancellation [Runtime::CancellationToken, nil] cancels the call when cancelled
    # @return [CreateGraphOutput] the service's response
    # @raise [InternalServerException]
    # @raise [NotFoundException]
    # @raise [ValidationException]
    # @raise [Runtime::LeylineException] for any other error response
    def create_graph(input, cancellation: nil)
      @runtime.call('CreateGraph', input, cancellation: cancellation)
    end
    # Not idempotent: not retried after a network failure or server error, since repeating it could repeat its effect. Throttling responses are still retried.
    #
    # Requires `graph:read` on the graph; `graph:write` on the graph, checked by the service (only when the parsed query writes); `history:read` on the history, when `has target.history`.
    #
    # @param input [ExecuteQueryInput] the request to send
    # @param cancellation [Runtime::CancellationToken, nil] cancels the call when cancelled
    # @return [ExecuteQueryOutput] the service's response
    # @raise [InternalServerException]
    # @raise [NotFoundException]
    # @raise [ValidationException]
    # @raise [Runtime::LeylineException] for any other error response
    def execute_query(input, cancellation: nil)
      @runtime.call('ExecuteQuery', input, cancellation: cancellation)
    end
    # Read-only: retried automatically after transient failures.
    #
    # Requires `graph:read` on the graph.
    #
    # @param input [GetGraphInput] the request to send
    # @param cancellation [Runtime::CancellationToken, nil] cancels the call when cancelled
    # @return [GetGraphOutput] the service's response
    # @raise [InternalServerException]
    # @raise [NotFoundException]
    # @raise [ValidationException]
    # @raise [Runtime::LeylineException] for any other error response
    def get_graph(input, cancellation: nil)
      @runtime.call('GetGraph', input, cancellation: cancellation)
    end
    # Read-only: retried automatically after transient failures.
    #
    # Requires permissions internal to the service.
    #
    # @param input [GetOrganizationInput] the request to send
    # @param cancellation [Runtime::CancellationToken, nil] cancels the call when cancelled
    # @return [GetOrganizationOutput] the service's response
    # @raise [InternalServerException]
    # @raise [NotFoundException]
    # @raise [ValidationException]
    # @raise [Runtime::LeylineException] for any other error response
    def get_organization(input, cancellation: nil)
      @runtime.call('GetOrganization', input, cancellation: cancellation)
    end
    # Answers about the caller's own credential.
    #
    # Read-only: retried automatically after transient failures.
    #
    # Requires no permission.
    #
    # @param cancellation [Runtime::CancellationToken, nil] cancels the call when cancelled
    # @return [GetUserOutput] the service's response
    # @raise [InternalServerException]
    # @raise [ValidationException]
    # @raise [Runtime::LeylineException] for any other error response
    def get_user(cancellation: nil)
      @runtime.call('GetUser', Runtime::Unit.new, cancellation: cancellation)
    end
    # No authentication: nothing to authorize.
    #
    # Read-only: retried automatically after transient failures.
    #
    # @param cancellation [Runtime::CancellationToken, nil] cancels the call when cancelled
    # @return [HealthOutput] the service's response
    # @raise [InternalServerException]
    # @raise [ValidationException]
    # @raise [Runtime::LeylineException] for any other error response
    def health(cancellation: nil)
      @runtime.call('Health', Runtime::Unit.new, cancellation: cancellation)
    end
    # Returns only the graphs the caller may read.
    #
    # Read-only: retried automatically after transient failures.
    #
    # Returns only the graphs the caller holds `graph:read` on in the organization.
    #
    # @param input [ListGraphsInput] the request to send
    # @param cancellation [Runtime::CancellationToken, nil] cancels the call when cancelled
    # @return [ListGraphsOutput] the service's response
    # @raise [InternalServerException]
    # @raise [NotFoundException]
    # @raise [ValidationException]
    # @raise [Runtime::LeylineException] for any other error response
    def list_graphs(input, cancellation: nil)
      @runtime.call('ListGraphs', input, cancellation: cancellation)
    end
    # Not idempotent: not retried after a network failure or server error, since repeating it could repeat its effect. Throttling responses are still retried.
    #
    # @param input [RestoreBackupInput] the request to send
    # @param cancellation [Runtime::CancellationToken, nil] cancels the call when cancelled
    # @return [RestoreBackupOutput] the service's response
    # @raise [InternalServerException]
    # @raise [ValidationException]
    # @raise [Runtime::LeylineException] for any other error response
    def restore_backup(input, cancellation: nil)
      @runtime.call('RestoreBackup', input, cancellation: cancellation)
    end
    # Hostile documentation: a backtick in a `when` literal, and handler
    # clause documentation that must keep its case, its paragraphs and its
    # code spans.
    #
    # Not idempotent: not retried after a network failure or server error, since repeating it could repeat its effect. Throttling responses are still retried.
    #
    # Requires `graph:read` on the graph, when ``mode == "a`b"``; `graph:write` on the graph, checked by the service (HTTP rewrites only, when `mode` is ``a`b``, and \[ops] approve); `history:read` on the history, checked by the service (see below).
    #
    # `history:read` on the history, checked by the service:
    #
    # GraphQL plans are checked twice:
    #
    # - once when parsed, against */ text;
    # - once when run, see \[bare] and [the guide](https://example.com/guide).
    #
    # \@see the runbook; @param is no tag either.
    #
    # @param input [RewriteGraphInput] the request to send
    # @param cancellation [Runtime::CancellationToken, nil] cancels the call when cancelled
    # @return [RewriteGraphOutput] the service's response
    # @raise [InternalServerException]
    # @raise [NotFoundException]
    # @raise [ValidationException]
    # @raise [Runtime::LeylineException] for any other error response
    def rewrite_graph(input, cancellation: nil)
      @runtime.call('RewriteGraph', input, cancellation: cancellation)
    end
  end
end

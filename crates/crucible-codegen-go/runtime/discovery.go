package sdk

// Endpoint discovery: a per-client cache of discovered endpoints with a
// single-flight refresh. docs/sdk-runtime-policy.md is the contract.

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"sort"
	"strings"
	"sync"
)

// EndpointDiscoveryMode is the client's endpoint discovery setting.
type EndpointDiscoveryMode string

const (
	// EndpointDiscoveryAuto, the default (also the zero value), discovers only
	// for operations that require it.
	EndpointDiscoveryAuto EndpointDiscoveryMode = "auto"
	// EndpointDiscoveryEnabled also discovers for operations where it is optional.
	EndpointDiscoveryEnabled EndpointDiscoveryMode = "enabled"
	// EndpointDiscoveryDisabled never discovers; operations that require it fail.
	EndpointDiscoveryDisabled EndpointDiscoveryMode = "disabled"
)

// ErrEndpointDiscovery matches, through errors.Is, a call that required
// endpoint discovery and did not get a usable endpoint from it.
var ErrEndpointDiscovery = errors.New("endpoint discovery failed")

// discoveryFailureTTL is how long, in seconds, an optional operation keeps
// using the configured endpoint after discovery failed.
const discoveryFailureTTL = 60.0

type endpointDiscoveryDescription struct {
	Operation         string
	Error             string
	OperationMember   string
	IdentifiersMember string
	EndpointsMember   string
	AddressMember     string
	CachePeriodMember string
}

type discoveredEndpointDescription struct {
	Required    bool
	Identifiers []string
}

// discoveredAddress is a discovered endpoint URL and the clock reading
// (seconds) at which it expires. Its identity marks the entry an attempt used.
type discoveredAddress struct {
	url     string
	expires float64
}

// discoveryRequest is what one call sends to the discovery operation; the
// cache key is exactly what is sent.
type discoveryRequest struct {
	key       string
	input     map[string]any
	required  bool
	operation string
}

type discoveryFlight struct {
	done   chan struct{}
	result *discoveredAddress
	err    error
}

type endpointCache struct {
	mu       sync.Mutex
	now      func() float64
	entries  map[string]*discoveredAddress
	flights  map[string]*discoveryFlight
	failures map[string]float64
}

func newEndpointCache(now func() float64) *endpointCache {
	return &endpointCache{now: now, entries: map[string]*discoveredAddress{}, flights: map[string]*discoveryFlight{}, failures: map[string]float64{}}
}

// failedRecently reports whether an optional operation should keep using the
// configured endpoint because discovery for key failed less than a minute ago.
func (c *endpointCache) failedRecently(key string) bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	expires, present := c.failures[key]
	if present && c.now() >= expires {
		delete(c.failures, key)
		return false
	}
	return present
}

func (c *endpointCache) recordFailure(key string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.failures[key] = c.now() + discoveryFailureTTL
}

// invalidate evicts an entry a response rejected, unless a newer one replaced it.
func (c *endpointCache) invalidate(key string, entry *discoveredAddress) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.entries[key] == entry {
		delete(c.entries, key)
	}
}

// resolve returns a valid cached entry, or the result of the one discovery
// shared by every caller asking for key. The discovery runs on its own
// goroutine, so a caller whose ctx ends stops waiting without cancelling it
// for the others.
func (c *endpointCache) resolve(ctx context.Context, key string, fetch func() (*discoveredAddress, error)) (*discoveredAddress, error) {
	c.mu.Lock()
	if entry := c.entries[key]; entry != nil && c.now() < entry.expires {
		c.mu.Unlock()
		return entry, nil
	}
	flight := c.flights[key]
	if flight == nil {
		flight = &discoveryFlight{done: make(chan struct{})}
		c.flights[key] = flight
		go c.run(key, flight, fetch)
	}
	c.mu.Unlock()
	select {
	case <-flight.done:
		return flight.result, flight.err
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

func (c *endpointCache) run(key string, flight *discoveryFlight, fetch func() (*discoveredAddress, error)) {
	var result *discoveredAddress
	var err error
	defer func() {
		if recovered := recover(); recovered != nil {
			result, err = nil, fmt.Errorf("%w: discovery panicked: %v", ErrEndpointDiscovery, recovered)
		}
		c.mu.Lock()
		if err == nil {
			// A zero cache period serves only the callers that waited for it.
			if c.now() < result.expires {
				c.entries[key] = result
			} else {
				delete(c.entries, key)
			}
		}
		if c.flights[key] == flight {
			delete(c.flights, key)
		}
		flight.result, flight.err = result, err
		c.mu.Unlock()
		close(flight.done)
	}()
	result, err = fetch()
}

func discoveryKey(operation *string, identifiers map[string]string) string {
	names := make([]string, 0, len(identifiers))
	for name := range identifiers {
		names = append(names, name)
	}
	sort.Strings(names)
	pairs := make([][2]string, len(names))
	for i, name := range names {
		pairs[i] = [2]string{name, identifiers[name]}
	}
	data, _ := json.Marshal([]any{operation, pairs})
	return string(data)
}

func lastSegment(id string) string {
	if index := strings.LastIndex(id, "#"); index >= 0 {
		return id[index+1:]
	}
	return id
}

// discoveryRequest returns what this call sends to the discovery operation, or
// nil when the call uses the configured endpoint.
func (c *clientRuntime) discoveryRequest(name string, op operationDescription, inputShape shapeDescription, values map[string]any) (*discoveryRequest, error) {
	config, discovered := c.schema.EndpointDiscovery, op.DiscoveredEndpoint
	if config == nil || discovered == nil {
		return nil, nil
	}
	switch c.config.EndpointDiscovery {
	case EndpointDiscoveryDisabled:
		if discovered.Required {
			return nil, fmt.Errorf("%w: %s requires endpoint discovery, which is disabled", ErrEndpointDiscovery, name)
		}
		return nil, nil
	case EndpointDiscoveryAuto:
		if !discovered.Required {
			return nil, nil
		}
	}
	discovery := c.schema.Operations[lastSegment(config.Operation)]
	discoveryInput := c.schema.Shapes[discovery.Input]
	var operation *string
	input := map[string]any{}
	if config.OperationMember != "" {
		operation = &name
		input[discoveryInput.wireName(config.OperationMember)] = name
	}
	identifiers := map[string]string{}
	if config.IdentifiersMember != "" {
		for _, member := range discovered.Identifiers {
			text, err := boundValue(values[inputShape.wireName(member)])
			if err != nil {
				return nil, fmt.Errorf("%s: %w", member, err)
			}
			identifiers[member] = text
		}
		if len(identifiers) > 0 {
			wire := map[string]any{}
			for key, value := range identifiers {
				wire[key] = value
			}
			input[discoveryInput.wireName(config.IdentifiersMember)] = wire
		}
	}
	return &discoveryRequest{key: discoveryKey(operation, identifiers), input: input, required: discovered.Required, operation: lastSegment(config.Operation)}, nil
}

// discoveredEndpoint returns the first usable endpoint of a discovery response.
func (c *clientRuntime) discoveredEndpoint(output map[string]any) (*discoveredAddress, error) {
	config := c.schema.EndpointDiscovery
	operation := c.schema.Operations[lastSegment(config.Operation)]
	shape := c.schema.Shapes[operation.Output]
	list := c.schema.Shapes[shape.Members[config.EndpointsMember].Target]
	entry := c.schema.Shapes[list.Member.Target]
	scheme := "https"
	if configured, err := url.Parse(c.config.Endpoint); err == nil && configured.Scheme != "" {
		scheme = configured.Scheme
	}
	endpoints, _ := output[shape.wireName(config.EndpointsMember)].([]any)
	for _, item := range endpoints {
		fields, ok := item.(map[string]any)
		if !ok {
			continue
		}
		address, _ := fields[entry.wireName(config.AddressMember)].(string)
		number, ok := fields[entry.wireName(config.CachePeriodMember)].(json.Number)
		if address == "" || !ok {
			continue
		}
		minutes, err := number.Int64()
		if err != nil {
			continue
		}
		// Discovered addresses are usually bare hosts; they keep the configured scheme.
		location := address
		if !strings.Contains(address, "://") {
			location = scheme + "://" + address
		}
		if parseEndpoint(location) == nil {
			continue
		}
		return &discoveredAddress{url: strings.TrimRight(location, "/"), expires: c.retry.now() + float64(max(minutes, 0))*60}, nil
	}
	return nil, fmt.Errorf("%w: the response has no usable endpoint", ErrEndpointDiscovery)
}

// invalidates reports whether a failure says the discovered endpoint is no longer valid.
func (c *clientRuntime) invalidates(err error) bool {
	var service *ServiceError
	if !errors.As(err, &service) {
		return false
	}
	if service.Metadata.Status == 421 {
		return true
	}
	config := c.schema.EndpointDiscovery
	return config != nil && config.Error != "" && service.Name == c.schema.Shapes[config.Error].errorCode()
}

// discover returns this attempt's discovered endpoint; nil means the configured one.
func (c *clientRuntime) discover(ctx context.Context, request *discoveryRequest) (*discoveredAddress, error) {
	if request == nil {
		return nil, nil
	}
	if !request.required && c.discovery.failedRecently(request.key) {
		return nil, nil
	}
	// The shared discovery is bounded by the deadline of the call that starts
	// it, but not cancelled with it: other callers may be waiting.
	deadline, bounded := ctx.Deadline()
	detached := context.WithoutCancel(ctx)
	address, err := c.discovery.resolve(ctx, request.key, func() (*discoveredAddress, error) {
		fetchCtx := detached
		if bounded {
			var cancel context.CancelFunc
			fetchCtx, cancel = context.WithDeadline(detached, deadline)
			defer cancel()
		}
		output, err := c.callValues(fetchCtx, request.operation, request.input)
		if err != nil {
			return nil, err
		}
		return c.discoveredEndpoint(output)
	})
	if err == nil {
		return address, nil
	}
	// Cancellation is never swallowed.
	if ctxErr := ctx.Err(); ctxErr != nil {
		return nil, ctxErr
	}
	if request.required {
		return nil, err
	}
	c.discovery.recordFailure(request.key)
	return nil, nil
}

package sdk

import (
	"context"
	"net/http"
	"strings"
)

// hasBearer reports whether the request already carries a Bearer credential,
// as when a middleware-free caller set the header through Config.Headers.
func hasBearer(request *http.Request) bool {
	value := request.Header.Get("Authorization")
	return len(value) > len("Bearer ") && strings.EqualFold(value[:len("Bearer ")], "Bearer ")
}

// authenticate attaches the bearer token to operations that require one and
// reports whether a credential is on the request. A client without a token
// sends the request unauthenticated and lets the service answer.
func (c *clientRuntime) authenticate(ctx context.Context, request *http.Request, op operationDescription) (bool, error) {
	if !op.Bearer {
		return false, nil
	}
	if hasBearer(request) {
		return true, nil
	}
	if c.config.Token == nil {
		return false, nil
	}
	token, err := c.config.Token(ctx)
	if err != nil {
		return false, err
	}
	if token == "" {
		return false, nil
	}
	request.Header.Set("Authorization", "Bearer "+token)
	return true, nil
}

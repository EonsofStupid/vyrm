package rrd

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"time"
)

const (
	defaultResponseLimit = int64(4 * 1024 * 1024)
	maximumResponseLimit = int64(16 * 1024 * 1024)
)

var (
	correlationPattern = regexp.MustCompile(`^[A-Za-z0-9._:-]+$`)
	canonicalPattern   = regexp.MustCompile(`^[a-z0-9][a-z0-9._-]*$`)
	pathParameter      = regexp.MustCompile(`\{([a-z]+)\}`)
	resourceKinds      = map[string]struct{}{
		"organization": {}, "estate": {}, "project": {}, "instance": {}, "node": {},
		"shard": {}, "collection": {}, "table": {}, "record": {}, "transaction": {},
		"snapshot": {}, "backup": {}, "operation": {},
	}
)

type Config struct {
	BaseURL          string
	Instance         string
	RequestTimeout   time.Duration
	MaxAttempts      int
	MaxResponseBytes int64
	Transport        http.RoundTripper
}

type Client struct {
	baseURL          *url.URL
	instance         string
	requestTimeout   time.Duration
	maxAttempts      int
	maxResponseBytes int64
	http             *http.Client
}

func NewClient(config Config) (*Client, error) {
	baseURL, err := loopbackURL(config.BaseURL)
	if err != nil {
		return nil, err
	}
	instance, err := canonical(config.Instance, "instance")
	if err != nil {
		return nil, err
	}
	timeout := config.RequestTimeout
	if timeout == 0 {
		timeout = 5 * time.Second
	}
	if timeout < time.Millisecond || timeout > 5*time.Minute {
		return nil, errors.New("request timeout must be in [1ms, 5m]")
	}
	attempts := config.MaxAttempts
	if attempts == 0 {
		attempts = 2
	}
	if attempts < 1 || attempts > 8 {
		return nil, errors.New("max attempts must be in 1..=8")
	}
	limit := config.MaxResponseBytes
	if limit == 0 {
		limit = defaultResponseLimit
	}
	if limit < 1 || limit > maximumResponseLimit {
		return nil, errors.New("response limit must be in 1..=16777216 bytes")
	}
	transport := config.Transport
	if transport == nil {
		transport = http.DefaultTransport
	}
	return &Client{
		baseURL:          baseURL,
		instance:         instance,
		requestTimeout:   timeout,
		maxAttempts:      attempts,
		maxResponseBytes: limit,
		http: &http.Client{
			Transport: transport,
			CheckRedirect: func(_ *http.Request, _ []*http.Request) error {
				return errors.New("RRD redirects are disabled")
			},
		},
	}, nil
}

func (c *Client) Capabilities(ctx context.Context) (map[string]any, error) {
	result, err := c.Call(ctx, OperationCapabilitiesRead, nil, RequestOptions{})
	if err != nil {
		return nil, err
	}
	instance, ok := result["instance"].(map[string]any)
	if result["protocol"] != "rrd" || result["protocol_version"] != float64(1) ||
		!ok || instance["id"] != c.instance {
		return nil, errors.New("RRD capability protocol or instance identity differs")
	}
	return result, nil
}

func (c *Client) EndpointCatalogue(ctx context.Context) (map[string]any, error) {
	return c.Call(ctx, OperationEndpointCatalogue, nil, RequestOptions{})
}

func (c *Client) OpenAPI(ctx context.Context) (map[string]any, error) {
	return c.Call(ctx, OperationOpenapiRead, nil, RequestOptions{})
}

func (c *Client) CreateSession(
	ctx context.Context,
	principalID string,
	credential string,
	payload any,
	options RequestOptions,
) (*Session, error) {
	principal, err := canonical(principalID, "principal")
	if err != nil {
		return nil, err
	}
	if credential == "" {
		return nil, errors.New("API-key credential must not be empty")
	}
	options.APIKey = &APIKeyCredentials{PrincipalID: principal, Credential: credential}
	result, err := c.Call(ctx, OperationSessionCreate, payload, options)
	if err != nil {
		return nil, err
	}
	raw, err := json.Marshal(result)
	if err != nil {
		return nil, fmt.Errorf("encode session lease: %w", err)
	}
	var lease SessionLease
	if err := decodeStrict(raw, &lease); err != nil {
		return nil, fmt.Errorf("invalid RRD session lease: %w", err)
	}
	if lease.SessionID == "" || lease.Token == "" {
		return nil, errors.New("invalid RRD session lease identity")
	}
	return &Session{PrincipalID: principal, Lease: lease}, nil
}

func (c *Client) Call(
	ctx context.Context,
	operation OperationID,
	payload any,
	options RequestOptions,
) (map[string]any, error) {
	descriptor, ok := endpoints[operation]
	if !ok {
		return nil, fmt.Errorf("unknown RRD operation %q", operation)
	}
	requestContext, err := makeContext(options, descriptor)
	if err != nil {
		return nil, err
	}
	path, err := resolvePath(descriptor.Path, options.PathParameters)
	if err != nil {
		return nil, err
	}
	target := c.baseURL.ResolveReference(&url.URL{Path: strings.TrimPrefix(path, "/")})
	headers := http.Header{"Accept": []string{"application/json"}}
	switch descriptor.Authentication {
	case "api_key":
		if options.APIKey == nil || options.APIKey.Credential == "" {
			return nil, fmt.Errorf("%s requires API-key authentication", operation)
		}
		principal, err := canonical(options.APIKey.PrincipalID, "principal")
		if err != nil {
			return nil, err
		}
		headers.Set("X-RRD-Principal", principal)
		headers.Set("Authorization", "ApiKey "+options.APIKey.Credential)
	case "session_bearer":
		if options.Session == nil || options.Session.Lease.SessionID == "" || options.Session.Lease.Token == "" {
			return nil, fmt.Errorf("%s requires a valid session", operation)
		}
		headers.Set("X-RRD-Session", options.Session.Lease.SessionID)
		headers.Set("Authorization", "Bearer "+options.Session.Lease.Token)
	}
	var body []byte
	if descriptor.Method != http.MethodGet {
		resource := options.Resource
		if resource == nil {
			resource, err = defaultResource(c.instance, options.PathParameters)
		}
		if err != nil {
			return nil, err
		}
		resource, err = validateResource(resource)
		if err != nil {
			return nil, err
		}
		body, err = json.Marshal(map[string]any{
			"protocol":         "rrd",
			"protocol_version": 1,
			"context":          requestContext,
			"resource":         map[string]any{"segments": resource},
			"payload":          payload,
		})
		if err != nil {
			return nil, fmt.Errorf("encode RRD request: %w", err)
		}
		headers.Set("Content-Type", "application/json")
	}
	attempts := 1
	if descriptor.Method == http.MethodGet || !descriptor.Mutation || options.IdempotencyKey != "" {
		attempts = c.maxAttempts
	}
	var lastErr error
	for attempt := 0; attempt < attempts; attempt++ {
		attemptContext, cancel, err := c.attemptContext(ctx, options.Deadline)
		if err != nil {
			return nil, err
		}
		request, err := http.NewRequestWithContext(
			attemptContext, descriptor.Method, target.String(), bytes.NewReader(body),
		)
		if err != nil {
			cancel()
			return nil, fmt.Errorf("construct RRD request: %w", err)
		}
		request.Header = headers.Clone()
		response, requestErr := c.http.Do(request)
		if requestErr != nil {
			cancel()
			lastErr = requestErr
			if ctx.Err() != nil {
				return nil, fmt.Errorf("RRD request cancelled: %w", ctx.Err())
			}
			continue
		}
		encoded, readErr := readBounded(response, c.maxResponseBytes)
		cancel()
		if readErr != nil {
			return nil, readErr
		}
		result, decodeErr := decodeResponse(response.StatusCode, encoded, requestContext)
		if decodeErr != nil {
			return nil, decodeErr
		}
		return result, nil
	}
	return nil, fmt.Errorf("RRD transport failed: %w", lastErr)
}

type requestEnvelopeContext struct {
	RequestID      string `json:"request_id"`
	OperationID    string `json:"operation_id"`
	IdempotencyKey string `json:"idempotency_key,omitempty"`
	DeadlineUnixMS int64  `json:"deadline_unix_ms,omitempty"`
}

func makeContext(options RequestOptions, endpoint Endpoint) (*requestEnvelopeContext, error) {
	if endpoint.Method == http.MethodGet {
		return nil, nil
	}
	requestID, err := correlation(options.RequestID, "request ID")
	if err != nil {
		return nil, err
	}
	operationID, err := correlation(options.OperationID, "operation ID")
	if err != nil {
		return nil, err
	}
	var key string
	if options.IdempotencyKey != "" {
		key, err = correlation(options.IdempotencyKey, "idempotency key")
		if err != nil {
			return nil, err
		}
	}
	if endpoint.Mutation && key == "" {
		return nil, errors.New("mutating requests require an idempotency key")
	}
	deadline := int64(0)
	if !options.Deadline.IsZero() {
		deadline = options.Deadline.UnixMilli()
		if deadline <= 0 {
			return nil, errors.New("deadline must be a positive Unix millisecond instant")
		}
	}
	return &requestEnvelopeContext{
		RequestID: requestID, OperationID: operationID, IdempotencyKey: key, DeadlineUnixMS: deadline,
	}, nil
}

type responseEnvelope struct {
	Protocol        string          `json:"protocol"`
	ProtocolVersion int             `json:"protocol_version"`
	RequestID       string          `json:"request_id"`
	OperationID     string          `json:"operation_id"`
	Outcome         responseOutcome `json:"outcome"`
}

type responseOutcome struct {
	Status  string          `json:"status"`
	Payload json.RawMessage `json:"payload,omitempty"`
	Error   *APIError       `json:"error,omitempty"`
}

func decodeResponse(
	status int,
	encoded []byte,
	requestContext *requestEnvelopeContext,
) (map[string]any, error) {
	var envelope responseEnvelope
	if err := decodeStrict(encoded, &envelope); err != nil {
		return nil, fmt.Errorf("RRD response envelope is invalid: %w", err)
	}
	if envelope.Protocol != "rrd" || envelope.ProtocolVersion != 1 {
		return nil, errors.New("RRD response protocol differs")
	}
	if requestContext != nil && (envelope.RequestID != requestContext.RequestID ||
		envelope.OperationID != requestContext.OperationID) {
		return nil, errors.New("RRD response request/operation identity differs")
	}
	success := status >= 200 && status < 300
	if success != (envelope.Outcome.Status == "ok") {
		return nil, errors.New("RRD HTTP status and typed outcome disagree")
	}
	if envelope.Outcome.Status == "error" {
		if envelope.Outcome.Error == nil || envelope.Outcome.Payload != nil {
			return nil, errors.New("RRD error outcome is missing its error")
		}
		if envelope.Outcome.Error.Code == "" || envelope.Outcome.Error.Message == "" {
			return nil, errors.New("RRD error outcome is incomplete")
		}
		if envelope.Outcome.Error.Details == nil {
			envelope.Outcome.Error.Details = map[string]string{}
		}
		envelope.Outcome.Error.Status = status
		return nil, envelope.Outcome.Error
	}
	if envelope.Outcome.Status != "ok" {
		return nil, errors.New("RRD response outcome status is invalid")
	}
	if envelope.Outcome.Error != nil || envelope.Outcome.Payload == nil {
		return nil, errors.New("RRD success outcome is invalid")
	}
	var payload map[string]any
	if err := decodeStrict(envelope.Outcome.Payload, &payload); err != nil || payload == nil {
		return nil, errors.New("RRD success payload must be an object")
	}
	return payload, nil
}

func decodeStrict(encoded []byte, output any) error {
	decoder := json.NewDecoder(bytes.NewReader(encoded))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(output); err != nil {
		return err
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		return errors.New("JSON contains trailing data")
	}
	return nil
}

func (c *Client) attemptContext(
	ctx context.Context,
	deadline time.Time,
) (context.Context, context.CancelFunc, error) {
	remaining := c.requestTimeout
	if !deadline.IsZero() {
		until := time.Until(deadline)
		if until <= 0 {
			return nil, nil, errors.New("RRD request deadline has expired")
		}
		if until < remaining {
			remaining = until
		}
	}
	attempt, cancel := context.WithTimeout(ctx, remaining)
	return attempt, cancel, nil
}

func readBounded(response *http.Response, maximum int64) ([]byte, error) {
	defer response.Body.Close()
	if response.ContentLength > maximum {
		return nil, errors.New("RRD response exceeded the configured byte limit")
	}
	encoded, err := io.ReadAll(io.LimitReader(response.Body, maximum+1))
	if err != nil {
		return nil, fmt.Errorf("read RRD response: %w", err)
	}
	if int64(len(encoded)) > maximum {
		return nil, errors.New("RRD response exceeded the configured byte limit")
	}
	return encoded, nil
}

func defaultResource(instance string, parameters map[string]string) ([]ResourceSegment, error) {
	segments := make([]ResourceSegment, 0, 2)
	if estate := parameters["estate"]; estate != "" {
		identifier, err := canonical(estate, "estate")
		if err != nil {
			return nil, err
		}
		segments = append(segments, ResourceSegment{Kind: "estate", ID: identifier})
	}
	return append(segments, ResourceSegment{Kind: "instance", ID: instance}), nil
}

func validateResource(segments []ResourceSegment) ([]ResourceSegment, error) {
	if len(segments) < 1 || len(segments) > 16 {
		return nil, errors.New("resource paths must contain 1..=16 segments")
	}
	seen := make(map[string]struct{}, len(segments))
	result := make([]ResourceSegment, 0, len(segments))
	for _, segment := range segments {
		if _, known := resourceKinds[segment.Kind]; !known {
			return nil, errors.New("resource kind is unknown")
		}
		if _, duplicate := seen[segment.Kind]; duplicate {
			return nil, errors.New("resource kind is repeated")
		}
		identifier, err := canonical(segment.ID, "resource")
		if err != nil {
			return nil, err
		}
		seen[segment.Kind] = struct{}{}
		result = append(result, ResourceSegment{Kind: segment.Kind, ID: identifier})
	}
	return result, nil
}

func resolvePath(template string, parameters map[string]string) (string, error) {
	var replacementErr error
	result := pathParameter.ReplaceAllStringFunc(template, func(match string) string {
		name := match[1 : len(match)-1]
		value := parameters[name]
		if value == "" {
			replacementErr = fmt.Errorf("missing path parameter %s", name)
			return match
		}
		identifier, err := correlation(value, name+" path parameter")
		if err != nil {
			replacementErr = err
			return match
		}
		return url.PathEscape(identifier)
	})
	return result, replacementErr
}

func correlation(value, label string) (string, error) {
	if len(value) < 1 || len(value) > 128 || !correlationPattern.MatchString(value) {
		return "", fmt.Errorf("%s is not a canonical RRD correlation ID", label)
	}
	return value, nil
}

func canonical(value, label string) (string, error) {
	if len(value) < 1 || len(value) > 128 || !canonicalPattern.MatchString(value) {
		return "", fmt.Errorf("%s is not a canonical RRD identifier", label)
	}
	return value, nil
}

func loopbackURL(value string) (*url.URL, error) {
	parsed, err := url.Parse(value)
	if err != nil {
		return nil, fmt.Errorf("parse RRD URL: %w", err)
	}
	host := parsed.Hostname()
	loopback := host == "localhost"
	if address := net.ParseIP(host); address != nil {
		loopback = address.IsLoopback()
	}
	if parsed.Scheme != "http" || !loopback || parsed.User != nil || parsed.RawQuery != "" ||
		parsed.Fragment != "" {
		return nil, errors.New(
			"RRD Go client permits only credential-free loopback HTTP before TLS qualification",
		)
	}
	if !strings.HasSuffix(parsed.Path, "/") {
		parsed.Path += "/"
	}
	return parsed, nil
}

func statusText(status int) string {
	return strconv.Itoa(status)
}

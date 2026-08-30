package rrd

import "time"

type Endpoint struct {
	Method         string
	Path           string
	Authentication string
	Mutation       bool
}

type ResourceSegment struct {
	Kind string `json:"kind"`
	ID   string `json:"id"`
}

type APIKeyCredentials struct {
	PrincipalID string
	Credential  string
}

type SessionLease struct {
	SessionID               string         `json:"session_id"`
	Token                   string         `json:"token"`
	IssuedAtUnixMS          uint64         `json:"issued_at_unix_ms"`
	IdleExpiresAtUnixMS     uint64         `json:"idle_expires_at_unix_ms"`
	AbsoluteExpiresAtUnixMS uint64         `json:"absolute_expires_at_unix_ms"`
	Limits                  map[string]any `json:"limits"`
}

type Session struct {
	PrincipalID string
	Lease       SessionLease
}

type RequestOptions struct {
	RequestID      string
	OperationID    string
	IdempotencyKey string
	Deadline       time.Time
	PathParameters map[string]string
	Resource       []ResourceSegment
	Session        *Session
	APIKey         *APIKeyCredentials
}

type APIError struct {
	Status    int
	Code      string            `json:"code"`
	Message   string            `json:"message"`
	Retryable bool              `json:"retryable"`
	Details   map[string]string `json:"details"`
}

func (e *APIError) Error() string {
	return "RRD API " + statusText(e.Status) + ": " + e.Code + ": " + e.Message
}

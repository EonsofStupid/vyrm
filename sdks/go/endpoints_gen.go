// Code generated from rrd-contract; DO NOT EDIT.

package rrd

type OperationID string

const (
	OperationAuditRead          OperationID = "audit-read"
	OperationBackupCreate       OperationID = "backup-create"
	OperationBackupList         OperationID = "backup-list"
	OperationCapabilitiesRead   OperationID = "capabilities-read"
	OperationChangefeedFollow   OperationID = "changefeed-follow"
	OperationChangefeedRead     OperationID = "changefeed-read"
	OperationEndpointCatalogue  OperationID = "endpoint-catalogue"
	OperationEstateRead         OperationID = "estate-read"
	OperationHealthLive         OperationID = "health-live"
	OperationHealthReady        OperationID = "health-ready"
	OperationOpenapiRead        OperationID = "openapi-read"
	OperationQueryExecute       OperationID = "query-execute"
	OperationQueryLivePoll      OperationID = "query-live-poll"
	OperationRestoreCreate      OperationID = "restore-create"
	OperationSessionClose       OperationID = "session-close"
	OperationSessionCreate      OperationID = "session-create"
	OperationSessionRenew       OperationID = "session-renew"
	OperationTransactionAbort   OperationID = "transaction-abort"
	OperationTransactionBegin   OperationID = "transaction-begin"
	OperationTransactionCommit  OperationID = "transaction-commit"
	OperationTransactionPreview OperationID = "transaction-preview"
	OperationVectorSearch       OperationID = "vector-search"
)

var endpoints = map[OperationID]Endpoint{
	OperationAuditRead:          {Method: "POST", Path: "/v1/audit/read", Authentication: "session_bearer", Mutation: false},
	OperationBackupCreate:       {Method: "POST", Path: "/v1/backups", Authentication: "session_bearer", Mutation: true},
	OperationBackupList:         {Method: "POST", Path: "/v1/backups/list", Authentication: "session_bearer", Mutation: false},
	OperationCapabilitiesRead:   {Method: "GET", Path: "/v1/capabilities", Authentication: "public", Mutation: false},
	OperationChangefeedFollow:   {Method: "POST", Path: "/v1/changes/follow", Authentication: "session_bearer", Mutation: false},
	OperationChangefeedRead:     {Method: "POST", Path: "/v1/changes/read", Authentication: "session_bearer", Mutation: false},
	OperationEndpointCatalogue:  {Method: "GET", Path: "/v1/schema/endpoints", Authentication: "public", Mutation: false},
	OperationEstateRead:         {Method: "POST", Path: "/v1/estates/{estate}/read", Authentication: "session_bearer", Mutation: false},
	OperationHealthLive:         {Method: "GET", Path: "/v1/health/live", Authentication: "public", Mutation: false},
	OperationHealthReady:        {Method: "GET", Path: "/v1/health/ready", Authentication: "public", Mutation: false},
	OperationOpenapiRead:        {Method: "GET", Path: "/v1/schema/openapi", Authentication: "public", Mutation: false},
	OperationQueryExecute:       {Method: "POST", Path: "/v1/query", Authentication: "session_bearer", Mutation: false},
	OperationQueryLivePoll:      {Method: "POST", Path: "/v1/query/live/poll", Authentication: "session_bearer", Mutation: false},
	OperationRestoreCreate:      {Method: "POST", Path: "/v1/restores", Authentication: "session_bearer", Mutation: true},
	OperationSessionClose:       {Method: "DELETE", Path: "/v1/sessions/{session}", Authentication: "session_bearer", Mutation: true},
	OperationSessionCreate:      {Method: "POST", Path: "/v1/sessions", Authentication: "api_key", Mutation: true},
	OperationSessionRenew:       {Method: "POST", Path: "/v1/sessions/{session}/renew", Authentication: "session_bearer", Mutation: true},
	OperationTransactionAbort:   {Method: "DELETE", Path: "/v1/transactions/{transaction}", Authentication: "session_bearer", Mutation: true},
	OperationTransactionBegin:   {Method: "POST", Path: "/v1/transactions", Authentication: "session_bearer", Mutation: true},
	OperationTransactionCommit:  {Method: "POST", Path: "/v1/transactions/{transaction}/commit", Authentication: "session_bearer", Mutation: true},
	OperationTransactionPreview: {Method: "POST", Path: "/v1/transactions/{transaction}/preview", Authentication: "session_bearer", Mutation: false},
	OperationVectorSearch:       {Method: "POST", Path: "/v1/vector/search", Authentication: "session_bearer", Mutation: false},
}

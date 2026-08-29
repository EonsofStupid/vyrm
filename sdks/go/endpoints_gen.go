// Code generated from rrd-contract; DO NOT EDIT.

package rrd

// OpenAPI SHA-256: 74432cc158bed22d9db4216684654f763f7a915ae05084356fa70ffb4f2653d0

type OperationID string

const (
	OperationAuditExport              OperationID = "audit-export"
	OperationAuditRead                OperationID = "audit-read"
	OperationBackupCreate             OperationID = "backup-create"
	OperationBackupList               OperationID = "backup-list"
	OperationCapabilitiesRead         OperationID = "capabilities-read"
	OperationChangefeedFollow         OperationID = "changefeed-follow"
	OperationChangefeedRead           OperationID = "changefeed-read"
	OperationDiagnosticsRead          OperationID = "diagnostics-read"
	OperationEndpointCatalogue        OperationID = "endpoint-catalogue"
	OperationEstateRead               OperationID = "estate-read"
	OperationHealthLive               OperationID = "health-live"
	OperationHealthReady              OperationID = "health-ready"
	OperationOpenapiRead              OperationID = "openapi-read"
	OperationQueryExecute             OperationID = "query-execute"
	OperationQueryIndexEnsure         OperationID = "query-index-ensure"
	OperationQueryIndexList           OperationID = "query-index-list"
	OperationQueryLivePoll            OperationID = "query-live-poll"
	OperationRestoreCreate            OperationID = "restore-create"
	OperationRuntimeToolCatalogueRead OperationID = "runtime-tool-catalogue-read"
	OperationRuntimeToolInvoke        OperationID = "runtime-tool-invoke"
	OperationSessionClose             OperationID = "session-close"
	OperationSessionCreate            OperationID = "session-create"
	OperationSessionRenew             OperationID = "session-renew"
	OperationSubscriptionClose        OperationID = "subscription-close"
	OperationSubscriptionOpen         OperationID = "subscription-open"
	OperationTransactionAbort         OperationID = "transaction-abort"
	OperationTransactionBegin         OperationID = "transaction-begin"
	OperationTransactionCommit        OperationID = "transaction-commit"
	OperationTransactionPreview       OperationID = "transaction-preview"
	OperationVectorCollectionEnsure   OperationID = "vector-collection-ensure"
	OperationVectorCollectionList     OperationID = "vector-collection-list"
	OperationVectorPointRetrieve      OperationID = "vector-point-retrieve"
	OperationVectorPointScroll        OperationID = "vector-point-scroll"
	OperationVectorSearch             OperationID = "vector-search"
)

var endpoints = map[OperationID]Endpoint{
	OperationAuditExport:              {Method: "POST", Path: "/v1/audit/export", Authentication: "session_bearer", Mutation: false},
	OperationAuditRead:                {Method: "POST", Path: "/v1/audit/read", Authentication: "session_bearer", Mutation: false},
	OperationBackupCreate:             {Method: "POST", Path: "/v1/backups", Authentication: "session_bearer", Mutation: true},
	OperationBackupList:               {Method: "POST", Path: "/v1/backups/list", Authentication: "session_bearer", Mutation: false},
	OperationCapabilitiesRead:         {Method: "GET", Path: "/v1/capabilities", Authentication: "public", Mutation: false},
	OperationChangefeedFollow:         {Method: "POST", Path: "/v1/changes/follow", Authentication: "session_bearer", Mutation: false},
	OperationChangefeedRead:           {Method: "POST", Path: "/v1/changes/read", Authentication: "session_bearer", Mutation: false},
	OperationDiagnosticsRead:          {Method: "POST", Path: "/v1/diagnostics/read", Authentication: "session_bearer", Mutation: false},
	OperationEndpointCatalogue:        {Method: "GET", Path: "/v1/schema/endpoints", Authentication: "public", Mutation: false},
	OperationEstateRead:               {Method: "POST", Path: "/v1/estates/{estate}/read", Authentication: "session_bearer", Mutation: false},
	OperationHealthLive:               {Method: "GET", Path: "/v1/health/live", Authentication: "public", Mutation: false},
	OperationHealthReady:              {Method: "GET", Path: "/v1/health/ready", Authentication: "public", Mutation: false},
	OperationOpenapiRead:              {Method: "GET", Path: "/v1/schema/openapi", Authentication: "public", Mutation: false},
	OperationQueryExecute:             {Method: "POST", Path: "/v1/query", Authentication: "session_bearer", Mutation: false},
	OperationQueryIndexEnsure:         {Method: "POST", Path: "/v1/query/indexes/ensure", Authentication: "session_bearer", Mutation: true},
	OperationQueryIndexList:           {Method: "POST", Path: "/v1/query/indexes/list", Authentication: "session_bearer", Mutation: false},
	OperationQueryLivePoll:            {Method: "POST", Path: "/v1/query/live/poll", Authentication: "session_bearer", Mutation: false},
	OperationRestoreCreate:            {Method: "POST", Path: "/v1/restores", Authentication: "session_bearer", Mutation: true},
	OperationRuntimeToolCatalogueRead: {Method: "POST", Path: "/v1/runtime/tools/list", Authentication: "session_bearer", Mutation: false},
	OperationRuntimeToolInvoke:        {Method: "POST", Path: "/v1/runtime/tools/invoke", Authentication: "session_bearer", Mutation: true},
	OperationSessionClose:             {Method: "DELETE", Path: "/v1/sessions/{session}", Authentication: "session_bearer", Mutation: true},
	OperationSessionCreate:            {Method: "POST", Path: "/v1/sessions", Authentication: "api_key", Mutation: true},
	OperationSessionRenew:             {Method: "POST", Path: "/v1/sessions/{session}/renew", Authentication: "session_bearer", Mutation: true},
	OperationSubscriptionClose:        {Method: "POST", Path: "/v1/subscriptions/close", Authentication: "session_bearer", Mutation: true},
	OperationSubscriptionOpen:         {Method: "POST", Path: "/v1/subscriptions/open", Authentication: "session_bearer", Mutation: true},
	OperationTransactionAbort:         {Method: "DELETE", Path: "/v1/transactions/{transaction}", Authentication: "session_bearer", Mutation: true},
	OperationTransactionBegin:         {Method: "POST", Path: "/v1/transactions", Authentication: "session_bearer", Mutation: true},
	OperationTransactionCommit:        {Method: "POST", Path: "/v1/transactions/{transaction}/commit", Authentication: "session_bearer", Mutation: true},
	OperationTransactionPreview:       {Method: "POST", Path: "/v1/transactions/{transaction}/preview", Authentication: "session_bearer", Mutation: true},
	OperationVectorCollectionEnsure:   {Method: "POST", Path: "/v1/vector/collections/ensure", Authentication: "session_bearer", Mutation: true},
	OperationVectorCollectionList:     {Method: "POST", Path: "/v1/vector/collections/list", Authentication: "session_bearer", Mutation: false},
	OperationVectorPointRetrieve:      {Method: "POST", Path: "/v1/vector/points/retrieve", Authentication: "session_bearer", Mutation: false},
	OperationVectorPointScroll:        {Method: "POST", Path: "/v1/vector/points/scroll", Authentication: "session_bearer", Mutation: false},
	OperationVectorSearch:             {Method: "POST", Path: "/v1/vector/search", Authentication: "session_bearer", Mutation: false},
}

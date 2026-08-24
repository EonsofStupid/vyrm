// Generated from rrd-contract; do not edit.
namespace Rrflow.Rrd;

public enum OperationId
{
    AuditRead,
    BackupCreate,
    BackupList,
    CapabilitiesRead,
    ChangefeedFollow,
    ChangefeedRead,
    EndpointCatalogue,
    EstateRead,
    HealthLive,
    HealthReady,
    OpenapiRead,
    QueryExecute,
    QueryIndexEnsure,
    QueryIndexList,
    QueryLivePoll,
    RestoreCreate,
    SessionClose,
    SessionCreate,
    SessionRenew,
    TransactionAbort,
    TransactionBegin,
    TransactionCommit,
    TransactionPreview,
    VectorSearch,
}

public enum Authentication
{
    Public,
    ApiKey,
    SessionBearer,
}

public sealed record Endpoint(
    string WireName,
    string Method,
    string Path,
    Authentication Authentication,
    bool Mutation);

public static class EndpointCatalog
{
    public const int Count = 24;

    public static Endpoint Get(OperationId operation) => operation switch
    {
        OperationId.AuditRead => new("audit-read", "POST", "/v1/audit/read", Authentication.SessionBearer, false),
        OperationId.BackupCreate => new("backup-create", "POST", "/v1/backups", Authentication.SessionBearer, true),
        OperationId.BackupList => new("backup-list", "POST", "/v1/backups/list", Authentication.SessionBearer, false),
        OperationId.CapabilitiesRead => new("capabilities-read", "GET", "/v1/capabilities", Authentication.Public, false),
        OperationId.ChangefeedFollow => new("changefeed-follow", "POST", "/v1/changes/follow", Authentication.SessionBearer, false),
        OperationId.ChangefeedRead => new("changefeed-read", "POST", "/v1/changes/read", Authentication.SessionBearer, false),
        OperationId.EndpointCatalogue => new("endpoint-catalogue", "GET", "/v1/schema/endpoints", Authentication.Public, false),
        OperationId.EstateRead => new("estate-read", "POST", "/v1/estates/{estate}/read", Authentication.SessionBearer, false),
        OperationId.HealthLive => new("health-live", "GET", "/v1/health/live", Authentication.Public, false),
        OperationId.HealthReady => new("health-ready", "GET", "/v1/health/ready", Authentication.Public, false),
        OperationId.OpenapiRead => new("openapi-read", "GET", "/v1/schema/openapi", Authentication.Public, false),
        OperationId.QueryExecute => new("query-execute", "POST", "/v1/query", Authentication.SessionBearer, false),
        OperationId.QueryIndexEnsure => new("query-index-ensure", "POST", "/v1/query/indexes/ensure", Authentication.SessionBearer, true),
        OperationId.QueryIndexList => new("query-index-list", "POST", "/v1/query/indexes/list", Authentication.SessionBearer, false),
        OperationId.QueryLivePoll => new("query-live-poll", "POST", "/v1/query/live/poll", Authentication.SessionBearer, false),
        OperationId.RestoreCreate => new("restore-create", "POST", "/v1/restores", Authentication.SessionBearer, true),
        OperationId.SessionClose => new("session-close", "DELETE", "/v1/sessions/{session}", Authentication.SessionBearer, true),
        OperationId.SessionCreate => new("session-create", "POST", "/v1/sessions", Authentication.ApiKey, true),
        OperationId.SessionRenew => new("session-renew", "POST", "/v1/sessions/{session}/renew", Authentication.SessionBearer, true),
        OperationId.TransactionAbort => new("transaction-abort", "DELETE", "/v1/transactions/{transaction}", Authentication.SessionBearer, true),
        OperationId.TransactionBegin => new("transaction-begin", "POST", "/v1/transactions", Authentication.SessionBearer, true),
        OperationId.TransactionCommit => new("transaction-commit", "POST", "/v1/transactions/{transaction}/commit", Authentication.SessionBearer, true),
        OperationId.TransactionPreview => new("transaction-preview", "POST", "/v1/transactions/{transaction}/preview", Authentication.SessionBearer, false),
        OperationId.VectorSearch => new("vector-search", "POST", "/v1/vector/search", Authentication.SessionBearer, false),
        _ => throw new System.ArgumentOutOfRangeException(nameof(operation)),
    };
}

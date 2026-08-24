// Generated from rrd-contract; do not edit.
package io.rrflow.rrd;

public enum OperationId {
    AUDIT_READ("audit-read", "POST", "/v1/audit/read", Authentication.SESSION_BEARER, false),
    BACKUP_CREATE("backup-create", "POST", "/v1/backups", Authentication.SESSION_BEARER, true),
    BACKUP_LIST("backup-list", "POST", "/v1/backups/list", Authentication.SESSION_BEARER, false),
    CAPABILITIES_READ("capabilities-read", "GET", "/v1/capabilities", Authentication.PUBLIC, false),
    CHANGEFEED_FOLLOW("changefeed-follow", "POST", "/v1/changes/follow", Authentication.SESSION_BEARER, false),
    CHANGEFEED_READ("changefeed-read", "POST", "/v1/changes/read", Authentication.SESSION_BEARER, false),
    ENDPOINT_CATALOGUE("endpoint-catalogue", "GET", "/v1/schema/endpoints", Authentication.PUBLIC, false),
    ESTATE_READ("estate-read", "POST", "/v1/estates/{estate}/read", Authentication.SESSION_BEARER, false),
    HEALTH_LIVE("health-live", "GET", "/v1/health/live", Authentication.PUBLIC, false),
    HEALTH_READY("health-ready", "GET", "/v1/health/ready", Authentication.PUBLIC, false),
    OPENAPI_READ("openapi-read", "GET", "/v1/schema/openapi", Authentication.PUBLIC, false),
    QUERY_EXECUTE("query-execute", "POST", "/v1/query", Authentication.SESSION_BEARER, false),
    QUERY_LIVE_POLL("query-live-poll", "POST", "/v1/query/live/poll", Authentication.SESSION_BEARER, false),
    RESTORE_CREATE("restore-create", "POST", "/v1/restores", Authentication.SESSION_BEARER, true),
    SESSION_CLOSE("session-close", "DELETE", "/v1/sessions/{session}", Authentication.SESSION_BEARER, true),
    SESSION_CREATE("session-create", "POST", "/v1/sessions", Authentication.API_KEY, true),
    SESSION_RENEW("session-renew", "POST", "/v1/sessions/{session}/renew", Authentication.SESSION_BEARER, true),
    TRANSACTION_ABORT("transaction-abort", "DELETE", "/v1/transactions/{transaction}", Authentication.SESSION_BEARER, true),
    TRANSACTION_BEGIN("transaction-begin", "POST", "/v1/transactions", Authentication.SESSION_BEARER, true),
    TRANSACTION_COMMIT("transaction-commit", "POST", "/v1/transactions/{transaction}/commit", Authentication.SESSION_BEARER, true),
    TRANSACTION_PREVIEW("transaction-preview", "POST", "/v1/transactions/{transaction}/preview", Authentication.SESSION_BEARER, false),
    VECTOR_SEARCH("vector-search", "POST", "/v1/vector/search", Authentication.SESSION_BEARER, false);

    public enum Authentication { PUBLIC, API_KEY, SESSION_BEARER }

    private final String wireName;
    private final String method;
    private final String path;
    private final Authentication authentication;
    private final boolean mutation;

    OperationId(String wireName, String method, String path, Authentication authentication, boolean mutation) {
        this.wireName = wireName;
        this.method = method;
        this.path = path;
        this.authentication = authentication;
        this.mutation = mutation;
    }

    public String wireName() { return wireName; }
    public String method() { return method; }
    public String path() { return path; }
    public Authentication authentication() { return authentication; }
    public boolean mutation() { return mutation; }
}

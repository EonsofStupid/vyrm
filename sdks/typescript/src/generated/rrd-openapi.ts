export interface paths {
    "/v1/audit/read": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD audit-read */
        post: operations["audit-read"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/backups": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD backup-create */
        post: operations["backup-create"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/backups/list": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD backup-list */
        post: operations["backup-list"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/capabilities": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** RRD capabilities-read */
        get: operations["capabilities-read"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/changes/follow": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD changefeed-follow */
        post: operations["changefeed-follow"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/changes/read": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD changefeed-read */
        post: operations["changefeed-read"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/estates/{estate}/read": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD estate-read */
        post: operations["estate-read"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/health/live": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** RRD health-live */
        get: operations["health-live"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/health/ready": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** RRD health-ready */
        get: operations["health-ready"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/query": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD query-execute */
        post: operations["query-execute"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/query/indexes/ensure": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD query-index-ensure */
        post: operations["query-index-ensure"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/query/indexes/list": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD query-index-list */
        post: operations["query-index-list"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/query/live/poll": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD query-live-poll */
        post: operations["query-live-poll"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/restores": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD restore-create */
        post: operations["restore-create"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/schema/endpoints": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** RRD endpoint-catalogue */
        get: operations["endpoint-catalogue"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/schema/openapi": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** RRD openapi-read */
        get: operations["openapi-read"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD session-create */
        post: operations["session-create"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** RRD session-close */
        delete: operations["session-close"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session}/renew": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD session-renew */
        post: operations["session-renew"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/transactions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD transaction-begin */
        post: operations["transaction-begin"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/transactions/{transaction}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** RRD transaction-abort */
        delete: operations["transaction-abort"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/transactions/{transaction}/commit": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD transaction-commit */
        post: operations["transaction-commit"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/transactions/{transaction}/preview": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD transaction-preview */
        post: operations["transaction-preview"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/vector/collections/ensure": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD vector-collection-ensure */
        post: operations["vector-collection-ensure"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/vector/collections/list": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD vector-collection-list */
        post: operations["vector-collection-list"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/vector/search": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** RRD vector-search */
        post: operations["vector-search"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
}
export type webhooks = Record<string, never>;
export interface components {
    schemas: {
        /** QueryValue */
        QueryValue: {
            /** @constant */
            type: "null";
        } | {
            /** @constant */
            type: "bool";
            value: boolean;
        } | {
            /** @constant */
            type: "integer";
            /** Format: int64 */
            value: number;
        } | {
            /** @constant */
            type: "unsigned";
            /** Format: uint64 */
            value: number;
        } | {
            /** @constant */
            type: "decimal";
            value: string;
        } | {
            /** @constant */
            type: "string";
            value: string;
        } | {
            /** @constant */
            type: "digest";
            value: string;
        } | {
            /** @constant */
            type: "list";
            value: components["schemas"]["QueryValue"][];
        } | {
            /** @constant */
            type: "map";
            value: {
                [key: string]: components["schemas"]["QueryValue"];
            };
        };
    };
    responses: never;
    parameters: never;
    requestBodies: never;
    headers: never;
    pathItems: never;
}
export type $defs = Record<string, never>;
export interface operations {
    "audit-read": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        /** Format: uint64 */
                        after_sequence: number;
                        /** Format: uint16 */
                        limit: number;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                records: {
                                    /** @enum {string} */
                                    action: "service_inspect" | "unknown_request" | "session_create" | "session_renew" | "session_close" | "query_execute" | "query_live_poll" | "query_index_ensure" | "query_index_list" | "transaction_begin" | "transaction_preview" | "transaction_commit" | "transaction_abort" | "changefeed_read" | "changefeed_follow" | "vector_collection_ensure" | "vector_collection_list" | "vector_search" | "backup_create" | "backup_list" | "restore_create" | "estate_read" | "audit_read" | "security_admin";
                                    /** Format: uint64 */
                                    at_unix_ms: number;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    audit_id: string;
                                    /** @enum {string} */
                                    decision: "allowed" | "denied" | "failed";
                                    operation_id: string;
                                    /** @enum {string} */
                                    phase: "authorized" | "completed";
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    principal_id?: string | null;
                                    request_id: string;
                                    request_sha256: string;
                                    /**
                                     * @description A fully explicit hierarchical identity. No field is inferred from process
                                     *     cwd, connection state, or a human label.
                                     */
                                    resource: {
                                        segments: {
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            id: string;
                                            /** @enum {string} */
                                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                                        }[];
                                    };
                                    response_sha256: string;
                                    /** Format: uint64 */
                                    sequence: number;
                                    /** Format: uint16 */
                                    status_code: number;
                                }[];
                                /** Format: uint64 */
                                requested_after_sequence: number;
                                /** Format: uint64 */
                                through_sequence: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                records: {
                                    /** @enum {string} */
                                    action: "service_inspect" | "unknown_request" | "session_create" | "session_renew" | "session_close" | "query_execute" | "query_live_poll" | "query_index_ensure" | "query_index_list" | "transaction_begin" | "transaction_preview" | "transaction_commit" | "transaction_abort" | "changefeed_read" | "changefeed_follow" | "vector_collection_ensure" | "vector_collection_list" | "vector_search" | "backup_create" | "backup_list" | "restore_create" | "estate_read" | "audit_read" | "security_admin";
                                    /** Format: uint64 */
                                    at_unix_ms: number;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    audit_id: string;
                                    /** @enum {string} */
                                    decision: "allowed" | "denied" | "failed";
                                    operation_id: string;
                                    /** @enum {string} */
                                    phase: "authorized" | "completed";
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    principal_id?: string | null;
                                    request_id: string;
                                    request_sha256: string;
                                    /**
                                     * @description A fully explicit hierarchical identity. No field is inferred from process
                                     *     cwd, connection state, or a human label.
                                     */
                                    resource: {
                                        segments: {
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            id: string;
                                            /** @enum {string} */
                                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                                        }[];
                                    };
                                    response_sha256: string;
                                    /** Format: uint64 */
                                    sequence: number;
                                    /** Format: uint16 */
                                    status_code: number;
                                }[];
                                /** Format: uint64 */
                                requested_after_sequence: number;
                                /** Format: uint64 */
                                through_sequence: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "backup-create": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        /** Format: uint64 */
                        created_at_unix_ms: number;
                        label: string;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                backup: {
                                    application_complete: boolean;
                                    archive: {
                                        /** Format: uint64 */
                                        action_count: number;
                                        archive_sha256: string;
                                        /** Format: uint64 */
                                        claim_sequence: number;
                                        /** Format: uint16 */
                                        contract_version: number;
                                        /** Format: uint16 */
                                        format_version: number;
                                        /** Format: uint64 */
                                        payload_bytes: number;
                                        /** Format: uint64 */
                                        runtime_commits: number;
                                        /** Format: uint64 */
                                        runtime_cursor: number;
                                        /** Format: uint64 */
                                        runtime_mutations: number;
                                        /** Format: uint64 */
                                        standalone_claims: number;
                                    };
                                    backup_sha256: string;
                                    /** @enum {string} */
                                    claims: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** Format: uint64 */
                                    created_at_unix_ms: number;
                                    /** @enum {string} */
                                    invocation_telemetry: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    label: string;
                                    /** @enum {string} */
                                    object_payloads: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** @enum {string} */
                                    projections: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** @enum {string} */
                                    snapshot_leases: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** @enum {string} */
                                    typed_runtime: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                };
                                /** Format: uint64 */
                                catalogue_revision: number;
                                catalogue_sha256: string;
                                idempotent_replay: boolean;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                backup: {
                                    application_complete: boolean;
                                    archive: {
                                        /** Format: uint64 */
                                        action_count: number;
                                        archive_sha256: string;
                                        /** Format: uint64 */
                                        claim_sequence: number;
                                        /** Format: uint16 */
                                        contract_version: number;
                                        /** Format: uint16 */
                                        format_version: number;
                                        /** Format: uint64 */
                                        payload_bytes: number;
                                        /** Format: uint64 */
                                        runtime_commits: number;
                                        /** Format: uint64 */
                                        runtime_cursor: number;
                                        /** Format: uint64 */
                                        runtime_mutations: number;
                                        /** Format: uint64 */
                                        standalone_claims: number;
                                    };
                                    backup_sha256: string;
                                    /** @enum {string} */
                                    claims: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** Format: uint64 */
                                    created_at_unix_ms: number;
                                    /** @enum {string} */
                                    invocation_telemetry: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    label: string;
                                    /** @enum {string} */
                                    object_payloads: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** @enum {string} */
                                    projections: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** @enum {string} */
                                    snapshot_leases: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** @enum {string} */
                                    typed_runtime: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                };
                                /** Format: uint64 */
                                catalogue_revision: number;
                                catalogue_sha256: string;
                                idempotent_replay: boolean;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "backup-list": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        /** @default false */
                        verify_archives?: boolean;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                archives_verified: boolean;
                                backups: {
                                    application_complete: boolean;
                                    archive: {
                                        /** Format: uint64 */
                                        action_count: number;
                                        archive_sha256: string;
                                        /** Format: uint64 */
                                        claim_sequence: number;
                                        /** Format: uint16 */
                                        contract_version: number;
                                        /** Format: uint16 */
                                        format_version: number;
                                        /** Format: uint64 */
                                        payload_bytes: number;
                                        /** Format: uint64 */
                                        runtime_commits: number;
                                        /** Format: uint64 */
                                        runtime_cursor: number;
                                        /** Format: uint64 */
                                        runtime_mutations: number;
                                        /** Format: uint64 */
                                        standalone_claims: number;
                                    };
                                    backup_sha256: string;
                                    /** @enum {string} */
                                    claims: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** Format: uint64 */
                                    created_at_unix_ms: number;
                                    /** @enum {string} */
                                    invocation_telemetry: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    label: string;
                                    /** @enum {string} */
                                    object_payloads: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** @enum {string} */
                                    projections: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** @enum {string} */
                                    snapshot_leases: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** @enum {string} */
                                    typed_runtime: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                }[];
                                catalogue_sha256: string;
                                /** Format: uint16 */
                                format_version: number;
                                /** Format: uint64 */
                                revision: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                archives_verified: boolean;
                                backups: {
                                    application_complete: boolean;
                                    archive: {
                                        /** Format: uint64 */
                                        action_count: number;
                                        archive_sha256: string;
                                        /** Format: uint64 */
                                        claim_sequence: number;
                                        /** Format: uint16 */
                                        contract_version: number;
                                        /** Format: uint16 */
                                        format_version: number;
                                        /** Format: uint64 */
                                        payload_bytes: number;
                                        /** Format: uint64 */
                                        runtime_commits: number;
                                        /** Format: uint64 */
                                        runtime_cursor: number;
                                        /** Format: uint64 */
                                        runtime_mutations: number;
                                        /** Format: uint64 */
                                        standalone_claims: number;
                                    };
                                    backup_sha256: string;
                                    /** @enum {string} */
                                    claims: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** Format: uint64 */
                                    created_at_unix_ms: number;
                                    /** @enum {string} */
                                    invocation_telemetry: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    label: string;
                                    /** @enum {string} */
                                    object_payloads: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** @enum {string} */
                                    projections: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** @enum {string} */
                                    snapshot_leases: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                    /** @enum {string} */
                                    typed_runtime: "included" | "referenced_only" | "rebuild_required" | "excluded";
                                }[];
                                catalogue_sha256: string;
                                /** Format: uint16 */
                                format_version: number;
                                /** Format: uint64 */
                                revision: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "capabilities-read": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                capabilities: {
                                    /** Format: uint16 */
                                    contract_version: number;
                                    limitation?: string | null;
                                    limits?: {
                                        [key: string]: number;
                                    };
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    name: string;
                                    /** @enum {string} */
                                    status: "unavailable" | "experimental" | "available";
                                }[];
                                /** @enum {string} */
                                deployment_mode: "embedded" | "local_server" | "distributed";
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                implementation: string;
                                implementation_version: string;
                                instance: {
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    id: string;
                                    /** @enum {string} */
                                    kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                                };
                                protocol: string;
                                /** Format: uint16 */
                                protocol_version: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                capabilities: {
                                    /** Format: uint16 */
                                    contract_version: number;
                                    limitation?: string | null;
                                    limits?: {
                                        [key: string]: number;
                                    };
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    name: string;
                                    /** @enum {string} */
                                    status: "unavailable" | "experimental" | "available";
                                }[];
                                /** @enum {string} */
                                deployment_mode: "embedded" | "local_server" | "distributed";
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                implementation: string;
                                implementation_version: string;
                                instance: {
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    id: string;
                                    /** @enum {string} */
                                    kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                                };
                                protocol: string;
                                /** Format: uint16 */
                                protocol_version: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "changefeed-follow": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        read: {
                            /** Format: uint64 */
                            after_cursor: number;
                            /** Format: uint64 */
                            limit: number;
                            scope: string;
                        };
                        /** Format: uint64 */
                        wait_timeout_ms: number;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                page: {
                                    changes: {
                                        actor: string;
                                        /** Format: uint64 */
                                        at_unix_ms: number;
                                        change_sha256: string;
                                        /** Format: uint64 */
                                        commit_ordinal: number;
                                        commit_sha256: string;
                                        /** Format: uint64 */
                                        cursor: number;
                                        mutation: {
                                            claim: {
                                                /** Format: float */
                                                confidence?: number | null;
                                                object: string;
                                                on_behalf_of?: string | null;
                                                predicate: string;
                                                producer: string;
                                                /** @enum {string} */
                                                promotion: "unpromoted" | "pending" | "promoted" | "denied";
                                                session?: string | null;
                                                signature?: string | null;
                                                subject: string;
                                                supersedes_sha256?: string | null;
                                                /** @enum {string} */
                                                tier: "local" | "primary" | "tenant";
                                                /** Format: uint64 */
                                                tx_time: number;
                                                /** Format: uint64 */
                                                valid_from: number;
                                                /** Format: uint64 */
                                                valid_to?: number | null;
                                            };
                                            /** @constant */
                                            family: "claim";
                                        } | {
                                            /** @constant */
                                            family: "data";
                                            /**
                                             * @description Public multi-model mutation vocabulary. It is deliberately independent of
                                             *     `vyrm_core`; adapters lower these values into the authoritative runtime.
                                             */
                                            mutation: {
                                                /** Format: float */
                                                confidence?: number | null;
                                                /** @constant */
                                                mutation: "assert_claim";
                                                object: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                predicate: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                producer: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                subject: string;
                                                /** Format: uint64 */
                                                tx_time: number;
                                                /** Format: uint64 */
                                                valid_from: number;
                                            } | {
                                                /** @constant */
                                                mutation: "put_schema";
                                                registry: {
                                                    /** @default {} */
                                                    events: {
                                                        [key: string]: {
                                                            /** @default false */
                                                            allow_additional_properties: boolean;
                                                            /** @default {} */
                                                            properties: {
                                                                [key: string]: {
                                                                    /** @default false */
                                                                    required: boolean;
                                                                    /** @enum {string} */
                                                                    value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                                };
                                                            };
                                                            /** @default false */
                                                            subject_required: boolean;
                                                            /** @default [] */
                                                            subject_types: string[];
                                                        };
                                                    };
                                                    migration: string;
                                                    /** @default {} */
                                                    records: {
                                                        [key: string]: {
                                                            /** @default false */
                                                            allow_additional_properties: boolean;
                                                            /** @default {} */
                                                            properties: {
                                                                [key: string]: {
                                                                    /** @default false */
                                                                    required: boolean;
                                                                    /** @enum {string} */
                                                                    value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                                };
                                                            };
                                                            /** @default [] */
                                                            unique_properties: string[];
                                                        };
                                                    };
                                                    /** @default {} */
                                                    relations: {
                                                        [key: string]: {
                                                            /** @default false */
                                                            allow_additional_properties: boolean;
                                                            /** @default [] */
                                                            from: string[];
                                                            /** Format: uint64 */
                                                            max_incoming?: number | null;
                                                            /** Format: uint64 */
                                                            max_outgoing?: number | null;
                                                            /** @default {} */
                                                            properties: {
                                                                [key: string]: {
                                                                    /** @default false */
                                                                    required: boolean;
                                                                    /** @enum {string} */
                                                                    value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                                };
                                                            };
                                                            /** @default [] */
                                                            to: string[];
                                                            /** @default false */
                                                            unique_pair: boolean;
                                                        };
                                                    };
                                                    /** Format: uint64 */
                                                    revision: number;
                                                };
                                            } | {
                                                /** @constant */
                                                mutation: "put_record";
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                reference: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                /** Format: uint64 */
                                                valid_from: number;
                                                /** Format: uint64 */
                                                valid_to?: number | null;
                                            } | {
                                                from: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                /** @constant */
                                                mutation: "put_relation";
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                reference: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                to: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                /** Format: uint64 */
                                                valid_from: number;
                                                /** Format: uint64 */
                                                valid_to?: number | null;
                                            } | {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                                /** @constant */
                                                mutation: "append_event";
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                subject?: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                } | null;
                                            } | {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                collection_id?: string | null;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                field: string;
                                                /** @constant */
                                                mutation: "put_vector";
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                provenance?: {
                                                    /** Format: uint32 */
                                                    dimensions: number;
                                                    /** @default {} */
                                                    generation_parameters: {
                                                        [key: string]: {
                                                            /** @constant */
                                                            type: "null";
                                                        } | {
                                                            /** @constant */
                                                            type: "bool";
                                                            value: boolean;
                                                        } | {
                                                            /** @constant */
                                                            type: "integer";
                                                            /** Format: int64 */
                                                            value: number;
                                                        } | {
                                                            /** @constant */
                                                            type: "unsigned";
                                                            /** Format: uint64 */
                                                            value: number;
                                                        } | {
                                                            /** @constant */
                                                            type: "decimal";
                                                            value: string;
                                                        } | {
                                                            /** @constant */
                                                            type: "string";
                                                            value: string;
                                                        } | {
                                                            /** @constant */
                                                            type: "digest";
                                                            value: string;
                                                        } | {
                                                            /** @constant */
                                                            type: "list";
                                                            value: components["schemas"]["QueryValue"][];
                                                        } | {
                                                            /** @constant */
                                                            type: "map";
                                                            value: {
                                                                [key: string]: components["schemas"]["QueryValue"];
                                                            };
                                                        };
                                                    };
                                                    model: string;
                                                    model_sha256: string;
                                                    /** @enum {string} */
                                                    normalization: "none" | "unit_l2";
                                                    source_sha256: string;
                                                } | null;
                                                reference: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                subject: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                /** Format: uint64 */
                                                valid_from: number;
                                                /** Format: uint64 */
                                                valid_to?: number | null;
                                                value: {
                                                    /** @constant */
                                                    kind: "dense";
                                                    values: number[];
                                                } | {
                                                    /** Format: uint32 */
                                                    dimensions: number;
                                                    indices: number[];
                                                    /** @constant */
                                                    kind: "sparse";
                                                    values: number[];
                                                } | {
                                                    /** Format: uint32 */
                                                    dimensions: number;
                                                    /** @constant */
                                                    kind: "multi_dense";
                                                    vectors: number[][];
                                                };
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                vector_name?: string | null;
                                            } | {
                                                /** @constant */
                                                mutation: "append_series_sample";
                                                /** Format: uint64 */
                                                observed_at: number;
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                reference: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                series: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                value: {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                };
                                            } | {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                field: string;
                                                /** @constant */
                                                mutation: "put_geo";
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                reference: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                subject: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                /** Format: uint64 */
                                                valid_from: number;
                                                /** Format: uint64 */
                                                valid_to?: number | null;
                                                value: {
                                                    /** @constant */
                                                    kind: "point";
                                                    point: {
                                                        /** Format: double */
                                                        latitude: number;
                                                        /** Format: double */
                                                        longitude: number;
                                                    };
                                                } | {
                                                    /** @constant */
                                                    kind: "bounding_box";
                                                    northeast: {
                                                        /** Format: double */
                                                        latitude: number;
                                                        /** Format: double */
                                                        longitude: number;
                                                    };
                                                    southwest: {
                                                        /** Format: double */
                                                        latitude: number;
                                                        /** Format: double */
                                                        longitude: number;
                                                    };
                                                };
                                            } | {
                                                /** Format: uint64 */
                                                length: number;
                                                media_type: string;
                                                /** @constant */
                                                mutation: "publish_object_reference";
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                receipt: {
                                                    backend: string;
                                                    etag?: string | null;
                                                    key: string;
                                                    version?: string | null;
                                                };
                                                reference: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                sha256: string;
                                                subject?: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                } | null;
                                            };
                                        };
                                        previous_change_sha256?: string | null;
                                        scope: string;
                                    }[];
                                    has_more: boolean;
                                    /** Format: uint64 */
                                    head_cursor: number;
                                    /** Format: uint64 */
                                    requested_after_cursor: number;
                                    /** Format: uint64 */
                                    through_cursor: number;
                                    validation: {
                                        /** Format: uint64 */
                                        change_reads: number;
                                        method: string;
                                        /** Format: uint16 */
                                        proof_nodes: number;
                                    };
                                };
                                timed_out: boolean;
                                /** Format: uint64 */
                                waited_ms: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                page: {
                                    changes: {
                                        actor: string;
                                        /** Format: uint64 */
                                        at_unix_ms: number;
                                        change_sha256: string;
                                        /** Format: uint64 */
                                        commit_ordinal: number;
                                        commit_sha256: string;
                                        /** Format: uint64 */
                                        cursor: number;
                                        mutation: {
                                            claim: {
                                                /** Format: float */
                                                confidence?: number | null;
                                                object: string;
                                                on_behalf_of?: string | null;
                                                predicate: string;
                                                producer: string;
                                                /** @enum {string} */
                                                promotion: "unpromoted" | "pending" | "promoted" | "denied";
                                                session?: string | null;
                                                signature?: string | null;
                                                subject: string;
                                                supersedes_sha256?: string | null;
                                                /** @enum {string} */
                                                tier: "local" | "primary" | "tenant";
                                                /** Format: uint64 */
                                                tx_time: number;
                                                /** Format: uint64 */
                                                valid_from: number;
                                                /** Format: uint64 */
                                                valid_to?: number | null;
                                            };
                                            /** @constant */
                                            family: "claim";
                                        } | {
                                            /** @constant */
                                            family: "data";
                                            /**
                                             * @description Public multi-model mutation vocabulary. It is deliberately independent of
                                             *     `vyrm_core`; adapters lower these values into the authoritative runtime.
                                             */
                                            mutation: {
                                                /** Format: float */
                                                confidence?: number | null;
                                                /** @constant */
                                                mutation: "assert_claim";
                                                object: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                predicate: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                producer: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                subject: string;
                                                /** Format: uint64 */
                                                tx_time: number;
                                                /** Format: uint64 */
                                                valid_from: number;
                                            } | {
                                                /** @constant */
                                                mutation: "put_schema";
                                                registry: {
                                                    /** @default {} */
                                                    events: {
                                                        [key: string]: {
                                                            /** @default false */
                                                            allow_additional_properties: boolean;
                                                            /** @default {} */
                                                            properties: {
                                                                [key: string]: {
                                                                    /** @default false */
                                                                    required: boolean;
                                                                    /** @enum {string} */
                                                                    value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                                };
                                                            };
                                                            /** @default false */
                                                            subject_required: boolean;
                                                            /** @default [] */
                                                            subject_types: string[];
                                                        };
                                                    };
                                                    migration: string;
                                                    /** @default {} */
                                                    records: {
                                                        [key: string]: {
                                                            /** @default false */
                                                            allow_additional_properties: boolean;
                                                            /** @default {} */
                                                            properties: {
                                                                [key: string]: {
                                                                    /** @default false */
                                                                    required: boolean;
                                                                    /** @enum {string} */
                                                                    value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                                };
                                                            };
                                                            /** @default [] */
                                                            unique_properties: string[];
                                                        };
                                                    };
                                                    /** @default {} */
                                                    relations: {
                                                        [key: string]: {
                                                            /** @default false */
                                                            allow_additional_properties: boolean;
                                                            /** @default [] */
                                                            from: string[];
                                                            /** Format: uint64 */
                                                            max_incoming?: number | null;
                                                            /** Format: uint64 */
                                                            max_outgoing?: number | null;
                                                            /** @default {} */
                                                            properties: {
                                                                [key: string]: {
                                                                    /** @default false */
                                                                    required: boolean;
                                                                    /** @enum {string} */
                                                                    value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                                };
                                                            };
                                                            /** @default [] */
                                                            to: string[];
                                                            /** @default false */
                                                            unique_pair: boolean;
                                                        };
                                                    };
                                                    /** Format: uint64 */
                                                    revision: number;
                                                };
                                            } | {
                                                /** @constant */
                                                mutation: "put_record";
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                reference: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                /** Format: uint64 */
                                                valid_from: number;
                                                /** Format: uint64 */
                                                valid_to?: number | null;
                                            } | {
                                                from: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                /** @constant */
                                                mutation: "put_relation";
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                reference: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                to: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                /** Format: uint64 */
                                                valid_from: number;
                                                /** Format: uint64 */
                                                valid_to?: number | null;
                                            } | {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                                /** @constant */
                                                mutation: "append_event";
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                subject?: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                } | null;
                                            } | {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                collection_id?: string | null;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                field: string;
                                                /** @constant */
                                                mutation: "put_vector";
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                provenance?: {
                                                    /** Format: uint32 */
                                                    dimensions: number;
                                                    /** @default {} */
                                                    generation_parameters: {
                                                        [key: string]: {
                                                            /** @constant */
                                                            type: "null";
                                                        } | {
                                                            /** @constant */
                                                            type: "bool";
                                                            value: boolean;
                                                        } | {
                                                            /** @constant */
                                                            type: "integer";
                                                            /** Format: int64 */
                                                            value: number;
                                                        } | {
                                                            /** @constant */
                                                            type: "unsigned";
                                                            /** Format: uint64 */
                                                            value: number;
                                                        } | {
                                                            /** @constant */
                                                            type: "decimal";
                                                            value: string;
                                                        } | {
                                                            /** @constant */
                                                            type: "string";
                                                            value: string;
                                                        } | {
                                                            /** @constant */
                                                            type: "digest";
                                                            value: string;
                                                        } | {
                                                            /** @constant */
                                                            type: "list";
                                                            value: components["schemas"]["QueryValue"][];
                                                        } | {
                                                            /** @constant */
                                                            type: "map";
                                                            value: {
                                                                [key: string]: components["schemas"]["QueryValue"];
                                                            };
                                                        };
                                                    };
                                                    model: string;
                                                    model_sha256: string;
                                                    /** @enum {string} */
                                                    normalization: "none" | "unit_l2";
                                                    source_sha256: string;
                                                } | null;
                                                reference: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                subject: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                /** Format: uint64 */
                                                valid_from: number;
                                                /** Format: uint64 */
                                                valid_to?: number | null;
                                                value: {
                                                    /** @constant */
                                                    kind: "dense";
                                                    values: number[];
                                                } | {
                                                    /** Format: uint32 */
                                                    dimensions: number;
                                                    indices: number[];
                                                    /** @constant */
                                                    kind: "sparse";
                                                    values: number[];
                                                } | {
                                                    /** Format: uint32 */
                                                    dimensions: number;
                                                    /** @constant */
                                                    kind: "multi_dense";
                                                    vectors: number[][];
                                                };
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                vector_name?: string | null;
                                            } | {
                                                /** @constant */
                                                mutation: "append_series_sample";
                                                /** Format: uint64 */
                                                observed_at: number;
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                reference: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                series: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                value: {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                };
                                            } | {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                field: string;
                                                /** @constant */
                                                mutation: "put_geo";
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                reference: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                subject: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                /** Format: uint64 */
                                                valid_from: number;
                                                /** Format: uint64 */
                                                valid_to?: number | null;
                                                value: {
                                                    /** @constant */
                                                    kind: "point";
                                                    point: {
                                                        /** Format: double */
                                                        latitude: number;
                                                        /** Format: double */
                                                        longitude: number;
                                                    };
                                                } | {
                                                    /** @constant */
                                                    kind: "bounding_box";
                                                    northeast: {
                                                        /** Format: double */
                                                        latitude: number;
                                                        /** Format: double */
                                                        longitude: number;
                                                    };
                                                    southwest: {
                                                        /** Format: double */
                                                        latitude: number;
                                                        /** Format: double */
                                                        longitude: number;
                                                    };
                                                };
                                            } | {
                                                /** Format: uint64 */
                                                length: number;
                                                media_type: string;
                                                /** @constant */
                                                mutation: "publish_object_reference";
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                receipt: {
                                                    backend: string;
                                                    etag?: string | null;
                                                    key: string;
                                                    version?: string | null;
                                                };
                                                reference: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                };
                                                sha256: string;
                                                subject?: {
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    id: string;
                                                    /**
                                                     * @description A canonical public identifier component.
                                                     *
                                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                     *     labels are separate data and may use arbitrary Unicode.
                                                     */
                                                    kind: string;
                                                } | null;
                                            };
                                        };
                                        previous_change_sha256?: string | null;
                                        scope: string;
                                    }[];
                                    has_more: boolean;
                                    /** Format: uint64 */
                                    head_cursor: number;
                                    /** Format: uint64 */
                                    requested_after_cursor: number;
                                    /** Format: uint64 */
                                    through_cursor: number;
                                    validation: {
                                        /** Format: uint64 */
                                        change_reads: number;
                                        method: string;
                                        /** Format: uint16 */
                                        proof_nodes: number;
                                    };
                                };
                                timed_out: boolean;
                                /** Format: uint64 */
                                waited_ms: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "changefeed-read": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        /** Format: uint64 */
                        after_cursor: number;
                        /** Format: uint64 */
                        limit: number;
                        scope: string;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                changes: {
                                    actor: string;
                                    /** Format: uint64 */
                                    at_unix_ms: number;
                                    change_sha256: string;
                                    /** Format: uint64 */
                                    commit_ordinal: number;
                                    commit_sha256: string;
                                    /** Format: uint64 */
                                    cursor: number;
                                    mutation: {
                                        claim: {
                                            /** Format: float */
                                            confidence?: number | null;
                                            object: string;
                                            on_behalf_of?: string | null;
                                            predicate: string;
                                            producer: string;
                                            /** @enum {string} */
                                            promotion: "unpromoted" | "pending" | "promoted" | "denied";
                                            session?: string | null;
                                            signature?: string | null;
                                            subject: string;
                                            supersedes_sha256?: string | null;
                                            /** @enum {string} */
                                            tier: "local" | "primary" | "tenant";
                                            /** Format: uint64 */
                                            tx_time: number;
                                            /** Format: uint64 */
                                            valid_from: number;
                                            /** Format: uint64 */
                                            valid_to?: number | null;
                                        };
                                        /** @constant */
                                        family: "claim";
                                    } | {
                                        /** @constant */
                                        family: "data";
                                        /**
                                         * @description Public multi-model mutation vocabulary. It is deliberately independent of
                                         *     `vyrm_core`; adapters lower these values into the authoritative runtime.
                                         */
                                        mutation: {
                                            /** Format: float */
                                            confidence?: number | null;
                                            /** @constant */
                                            mutation: "assert_claim";
                                            object: string;
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            predicate: string;
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            producer: string;
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            subject: string;
                                            /** Format: uint64 */
                                            tx_time: number;
                                            /** Format: uint64 */
                                            valid_from: number;
                                        } | {
                                            /** @constant */
                                            mutation: "put_schema";
                                            registry: {
                                                /** @default {} */
                                                events: {
                                                    [key: string]: {
                                                        /** @default false */
                                                        allow_additional_properties: boolean;
                                                        /** @default {} */
                                                        properties: {
                                                            [key: string]: {
                                                                /** @default false */
                                                                required: boolean;
                                                                /** @enum {string} */
                                                                value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                            };
                                                        };
                                                        /** @default false */
                                                        subject_required: boolean;
                                                        /** @default [] */
                                                        subject_types: string[];
                                                    };
                                                };
                                                migration: string;
                                                /** @default {} */
                                                records: {
                                                    [key: string]: {
                                                        /** @default false */
                                                        allow_additional_properties: boolean;
                                                        /** @default {} */
                                                        properties: {
                                                            [key: string]: {
                                                                /** @default false */
                                                                required: boolean;
                                                                /** @enum {string} */
                                                                value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                            };
                                                        };
                                                        /** @default [] */
                                                        unique_properties: string[];
                                                    };
                                                };
                                                /** @default {} */
                                                relations: {
                                                    [key: string]: {
                                                        /** @default false */
                                                        allow_additional_properties: boolean;
                                                        /** @default [] */
                                                        from: string[];
                                                        /** Format: uint64 */
                                                        max_incoming?: number | null;
                                                        /** Format: uint64 */
                                                        max_outgoing?: number | null;
                                                        /** @default {} */
                                                        properties: {
                                                            [key: string]: {
                                                                /** @default false */
                                                                required: boolean;
                                                                /** @enum {string} */
                                                                value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                            };
                                                        };
                                                        /** @default [] */
                                                        to: string[];
                                                        /** @default false */
                                                        unique_pair: boolean;
                                                    };
                                                };
                                                /** Format: uint64 */
                                                revision: number;
                                            };
                                        } | {
                                            /** @constant */
                                            mutation: "put_record";
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            reference: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            /** Format: uint64 */
                                            valid_from: number;
                                            /** Format: uint64 */
                                            valid_to?: number | null;
                                        } | {
                                            from: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            /** @constant */
                                            mutation: "put_relation";
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            reference: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            to: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            /** Format: uint64 */
                                            valid_from: number;
                                            /** Format: uint64 */
                                            valid_to?: number | null;
                                        } | {
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            kind: string;
                                            /** @constant */
                                            mutation: "append_event";
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            subject?: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            } | null;
                                        } | {
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            collection_id?: string | null;
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            field: string;
                                            /** @constant */
                                            mutation: "put_vector";
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            provenance?: {
                                                /** Format: uint32 */
                                                dimensions: number;
                                                /** @default {} */
                                                generation_parameters: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                model: string;
                                                model_sha256: string;
                                                /** @enum {string} */
                                                normalization: "none" | "unit_l2";
                                                source_sha256: string;
                                            } | null;
                                            reference: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            subject: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            /** Format: uint64 */
                                            valid_from: number;
                                            /** Format: uint64 */
                                            valid_to?: number | null;
                                            value: {
                                                /** @constant */
                                                kind: "dense";
                                                values: number[];
                                            } | {
                                                /** Format: uint32 */
                                                dimensions: number;
                                                indices: number[];
                                                /** @constant */
                                                kind: "sparse";
                                                values: number[];
                                            } | {
                                                /** Format: uint32 */
                                                dimensions: number;
                                                /** @constant */
                                                kind: "multi_dense";
                                                vectors: number[][];
                                            };
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            vector_name?: string | null;
                                        } | {
                                            /** @constant */
                                            mutation: "append_series_sample";
                                            /** Format: uint64 */
                                            observed_at: number;
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            reference: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            series: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            value: {
                                                /** @constant */
                                                type: "integer";
                                                /** Format: int64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "unsigned";
                                                /** Format: uint64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "decimal";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "bool";
                                                value: boolean;
                                            } | {
                                                /** @constant */
                                                type: "string";
                                                value: string;
                                            };
                                        } | {
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            field: string;
                                            /** @constant */
                                            mutation: "put_geo";
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            reference: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            subject: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            /** Format: uint64 */
                                            valid_from: number;
                                            /** Format: uint64 */
                                            valid_to?: number | null;
                                            value: {
                                                /** @constant */
                                                kind: "point";
                                                point: {
                                                    /** Format: double */
                                                    latitude: number;
                                                    /** Format: double */
                                                    longitude: number;
                                                };
                                            } | {
                                                /** @constant */
                                                kind: "bounding_box";
                                                northeast: {
                                                    /** Format: double */
                                                    latitude: number;
                                                    /** Format: double */
                                                    longitude: number;
                                                };
                                                southwest: {
                                                    /** Format: double */
                                                    latitude: number;
                                                    /** Format: double */
                                                    longitude: number;
                                                };
                                            };
                                        } | {
                                            /** Format: uint64 */
                                            length: number;
                                            media_type: string;
                                            /** @constant */
                                            mutation: "publish_object_reference";
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            receipt: {
                                                backend: string;
                                                etag?: string | null;
                                                key: string;
                                                version?: string | null;
                                            };
                                            reference: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            sha256: string;
                                            subject?: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            } | null;
                                        };
                                    };
                                    previous_change_sha256?: string | null;
                                    scope: string;
                                }[];
                                has_more: boolean;
                                /** Format: uint64 */
                                head_cursor: number;
                                /** Format: uint64 */
                                requested_after_cursor: number;
                                /** Format: uint64 */
                                through_cursor: number;
                                validation: {
                                    /** Format: uint64 */
                                    change_reads: number;
                                    method: string;
                                    /** Format: uint16 */
                                    proof_nodes: number;
                                };
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                changes: {
                                    actor: string;
                                    /** Format: uint64 */
                                    at_unix_ms: number;
                                    change_sha256: string;
                                    /** Format: uint64 */
                                    commit_ordinal: number;
                                    commit_sha256: string;
                                    /** Format: uint64 */
                                    cursor: number;
                                    mutation: {
                                        claim: {
                                            /** Format: float */
                                            confidence?: number | null;
                                            object: string;
                                            on_behalf_of?: string | null;
                                            predicate: string;
                                            producer: string;
                                            /** @enum {string} */
                                            promotion: "unpromoted" | "pending" | "promoted" | "denied";
                                            session?: string | null;
                                            signature?: string | null;
                                            subject: string;
                                            supersedes_sha256?: string | null;
                                            /** @enum {string} */
                                            tier: "local" | "primary" | "tenant";
                                            /** Format: uint64 */
                                            tx_time: number;
                                            /** Format: uint64 */
                                            valid_from: number;
                                            /** Format: uint64 */
                                            valid_to?: number | null;
                                        };
                                        /** @constant */
                                        family: "claim";
                                    } | {
                                        /** @constant */
                                        family: "data";
                                        /**
                                         * @description Public multi-model mutation vocabulary. It is deliberately independent of
                                         *     `vyrm_core`; adapters lower these values into the authoritative runtime.
                                         */
                                        mutation: {
                                            /** Format: float */
                                            confidence?: number | null;
                                            /** @constant */
                                            mutation: "assert_claim";
                                            object: string;
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            predicate: string;
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            producer: string;
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            subject: string;
                                            /** Format: uint64 */
                                            tx_time: number;
                                            /** Format: uint64 */
                                            valid_from: number;
                                        } | {
                                            /** @constant */
                                            mutation: "put_schema";
                                            registry: {
                                                /** @default {} */
                                                events: {
                                                    [key: string]: {
                                                        /** @default false */
                                                        allow_additional_properties: boolean;
                                                        /** @default {} */
                                                        properties: {
                                                            [key: string]: {
                                                                /** @default false */
                                                                required: boolean;
                                                                /** @enum {string} */
                                                                value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                            };
                                                        };
                                                        /** @default false */
                                                        subject_required: boolean;
                                                        /** @default [] */
                                                        subject_types: string[];
                                                    };
                                                };
                                                migration: string;
                                                /** @default {} */
                                                records: {
                                                    [key: string]: {
                                                        /** @default false */
                                                        allow_additional_properties: boolean;
                                                        /** @default {} */
                                                        properties: {
                                                            [key: string]: {
                                                                /** @default false */
                                                                required: boolean;
                                                                /** @enum {string} */
                                                                value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                            };
                                                        };
                                                        /** @default [] */
                                                        unique_properties: string[];
                                                    };
                                                };
                                                /** @default {} */
                                                relations: {
                                                    [key: string]: {
                                                        /** @default false */
                                                        allow_additional_properties: boolean;
                                                        /** @default [] */
                                                        from: string[];
                                                        /** Format: uint64 */
                                                        max_incoming?: number | null;
                                                        /** Format: uint64 */
                                                        max_outgoing?: number | null;
                                                        /** @default {} */
                                                        properties: {
                                                            [key: string]: {
                                                                /** @default false */
                                                                required: boolean;
                                                                /** @enum {string} */
                                                                value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                            };
                                                        };
                                                        /** @default [] */
                                                        to: string[];
                                                        /** @default false */
                                                        unique_pair: boolean;
                                                    };
                                                };
                                                /** Format: uint64 */
                                                revision: number;
                                            };
                                        } | {
                                            /** @constant */
                                            mutation: "put_record";
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            reference: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            /** Format: uint64 */
                                            valid_from: number;
                                            /** Format: uint64 */
                                            valid_to?: number | null;
                                        } | {
                                            from: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            /** @constant */
                                            mutation: "put_relation";
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            reference: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            to: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            /** Format: uint64 */
                                            valid_from: number;
                                            /** Format: uint64 */
                                            valid_to?: number | null;
                                        } | {
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            kind: string;
                                            /** @constant */
                                            mutation: "append_event";
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            subject?: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            } | null;
                                        } | {
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            collection_id?: string | null;
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            field: string;
                                            /** @constant */
                                            mutation: "put_vector";
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            provenance?: {
                                                /** Format: uint32 */
                                                dimensions: number;
                                                /** @default {} */
                                                generation_parameters: {
                                                    [key: string]: {
                                                        /** @constant */
                                                        type: "null";
                                                    } | {
                                                        /** @constant */
                                                        type: "bool";
                                                        value: boolean;
                                                    } | {
                                                        /** @constant */
                                                        type: "integer";
                                                        /** Format: int64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "unsigned";
                                                        /** Format: uint64 */
                                                        value: number;
                                                    } | {
                                                        /** @constant */
                                                        type: "decimal";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "string";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "digest";
                                                        value: string;
                                                    } | {
                                                        /** @constant */
                                                        type: "list";
                                                        value: components["schemas"]["QueryValue"][];
                                                    } | {
                                                        /** @constant */
                                                        type: "map";
                                                        value: {
                                                            [key: string]: components["schemas"]["QueryValue"];
                                                        };
                                                    };
                                                };
                                                model: string;
                                                model_sha256: string;
                                                /** @enum {string} */
                                                normalization: "none" | "unit_l2";
                                                source_sha256: string;
                                            } | null;
                                            reference: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            subject: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            /** Format: uint64 */
                                            valid_from: number;
                                            /** Format: uint64 */
                                            valid_to?: number | null;
                                            value: {
                                                /** @constant */
                                                kind: "dense";
                                                values: number[];
                                            } | {
                                                /** Format: uint32 */
                                                dimensions: number;
                                                indices: number[];
                                                /** @constant */
                                                kind: "sparse";
                                                values: number[];
                                            } | {
                                                /** Format: uint32 */
                                                dimensions: number;
                                                /** @constant */
                                                kind: "multi_dense";
                                                vectors: number[][];
                                            };
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            vector_name?: string | null;
                                        } | {
                                            /** @constant */
                                            mutation: "append_series_sample";
                                            /** Format: uint64 */
                                            observed_at: number;
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            reference: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            series: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            value: {
                                                /** @constant */
                                                type: "integer";
                                                /** Format: int64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "unsigned";
                                                /** Format: uint64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "decimal";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "bool";
                                                value: boolean;
                                            } | {
                                                /** @constant */
                                                type: "string";
                                                value: string;
                                            };
                                        } | {
                                            /**
                                             * @description A canonical public identifier component.
                                             *
                                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                             *     labels are separate data and may use arbitrary Unicode.
                                             */
                                            field: string;
                                            /** @constant */
                                            mutation: "put_geo";
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            reference: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            subject: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            /** Format: uint64 */
                                            valid_from: number;
                                            /** Format: uint64 */
                                            valid_to?: number | null;
                                            value: {
                                                /** @constant */
                                                kind: "point";
                                                point: {
                                                    /** Format: double */
                                                    latitude: number;
                                                    /** Format: double */
                                                    longitude: number;
                                                };
                                            } | {
                                                /** @constant */
                                                kind: "bounding_box";
                                                northeast: {
                                                    /** Format: double */
                                                    latitude: number;
                                                    /** Format: double */
                                                    longitude: number;
                                                };
                                                southwest: {
                                                    /** Format: double */
                                                    latitude: number;
                                                    /** Format: double */
                                                    longitude: number;
                                                };
                                            };
                                        } | {
                                            /** Format: uint64 */
                                            length: number;
                                            media_type: string;
                                            /** @constant */
                                            mutation: "publish_object_reference";
                                            /** @default {} */
                                            properties: {
                                                [key: string]: {
                                                    /** @constant */
                                                    type: "null";
                                                } | {
                                                    /** @constant */
                                                    type: "bool";
                                                    value: boolean;
                                                } | {
                                                    /** @constant */
                                                    type: "integer";
                                                    /** Format: int64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "unsigned";
                                                    /** Format: uint64 */
                                                    value: number;
                                                } | {
                                                    /** @constant */
                                                    type: "decimal";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "string";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "digest";
                                                    value: string;
                                                } | {
                                                    /** @constant */
                                                    type: "list";
                                                    value: components["schemas"]["QueryValue"][];
                                                } | {
                                                    /** @constant */
                                                    type: "map";
                                                    value: {
                                                        [key: string]: components["schemas"]["QueryValue"];
                                                    };
                                                };
                                            };
                                            receipt: {
                                                backend: string;
                                                etag?: string | null;
                                                key: string;
                                                version?: string | null;
                                            };
                                            reference: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            };
                                            sha256: string;
                                            subject?: {
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                id: string;
                                                /**
                                                 * @description A canonical public identifier component.
                                                 *
                                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                                 *     labels are separate data and may use arbitrary Unicode.
                                                 */
                                                kind: string;
                                            } | null;
                                        };
                                    };
                                    previous_change_sha256?: string | null;
                                    scope: string;
                                }[];
                                has_more: boolean;
                                /** Format: uint64 */
                                head_cursor: number;
                                /** Format: uint64 */
                                requested_after_cursor: number;
                                /** Format: uint64 */
                                through_cursor: number;
                                validation: {
                                    /** Format: uint64 */
                                    change_reads: number;
                                    method: string;
                                    /** Format: uint16 */
                                    proof_nodes: number;
                                };
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "estate-read": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path: {
                estate: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: Record<string, never>;
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                activity_policy: {
                                    /** Format: uint64 */
                                    idle_after_ms: number;
                                    /** Format: uint64 */
                                    neglected_after_ms: number;
                                    /** Format: uint64 */
                                    stale_after_ms: number;
                                };
                                /** Format: uint64 */
                                created_at_unix_ms: number;
                                /** Format: uint16 */
                                format_version: number;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /** Format: uint32 */
                                idempotency_binding_count: number;
                                instances: {
                                    activity: {
                                        /** @enum {string} */
                                        class: "unknown" | "active" | "idle" | "stale" | "neglected";
                                        /** Format: uint64 */
                                        evaluated_at_unix_ms: number;
                                        /** Format: uint64 */
                                        last_heartbeat_at_unix_ms?: number | null;
                                        /** Format: uint64 */
                                        last_meaningful_runtime_at_unix_ms?: number | null;
                                    };
                                    desired: {
                                        configuration_sha256: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        deployment_ref: string;
                                        /** Format: uint64 */
                                        generation: number;
                                        /** @enum {string} */
                                        phase: "running" | "stopped" | "absent";
                                        /** Format: uint64 */
                                        updated_at_unix_ms: number;
                                        version: string;
                                    };
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    id: string;
                                    observed: {
                                        error?: string | null;
                                        evidence_sha256?: string | null;
                                        /** Format: uint64 */
                                        generation: number;
                                        /** Format: uint64 */
                                        observed_at_unix_ms: number;
                                        /** @enum {string} */
                                        phase: "unknown" | "provisioning" | "starting" | "running" | "stopping" | "stopped" | "deleting" | "absent" | "failed";
                                        /** Format: uint32 */
                                        process_id?: number | null;
                                        version?: string | null;
                                    };
                                }[];
                                operations: {
                                    /** Format: uint32 */
                                    attempts: number;
                                    /** Format: uint64 */
                                    created_at_unix_ms: number;
                                    /** Format: uint64 */
                                    desired_generation: number;
                                    error?: string | null;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    id: string;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    instance_id: string;
                                    /** @enum {string} */
                                    kind: "provision" | "start" | "stop" | "restart" | "upgrade" | "delete";
                                    lease?: {
                                        /** Format: uint64 */
                                        acquired_at_unix_ms: number;
                                        /** Format: uint64 */
                                        epoch: number;
                                        /** Format: uint64 */
                                        expires_at_unix_ms: number;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        owner: string;
                                    } | null;
                                    receipts?: {
                                        /** Format: uint64 */
                                        at_unix_ms: number;
                                        /** @enum {string} */
                                        boundary: "prepared" | "applied" | "completed" | "failed";
                                        evidence_sha256: string;
                                        /** Format: uint64 */
                                        lease_epoch: number;
                                    }[];
                                    request_sha256: string;
                                    /** @enum {string} */
                                    state: "pending" | "leased" | "prepared" | "applied" | "succeeded" | "failed" | "superseded";
                                    /** Format: uint64 */
                                    updated_at_unix_ms: number;
                                }[];
                                /** Format: uint64 */
                                revision: number;
                                /** Format: uint64 */
                                updated_at_unix_ms: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                activity_policy: {
                                    /** Format: uint64 */
                                    idle_after_ms: number;
                                    /** Format: uint64 */
                                    neglected_after_ms: number;
                                    /** Format: uint64 */
                                    stale_after_ms: number;
                                };
                                /** Format: uint64 */
                                created_at_unix_ms: number;
                                /** Format: uint16 */
                                format_version: number;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /** Format: uint32 */
                                idempotency_binding_count: number;
                                instances: {
                                    activity: {
                                        /** @enum {string} */
                                        class: "unknown" | "active" | "idle" | "stale" | "neglected";
                                        /** Format: uint64 */
                                        evaluated_at_unix_ms: number;
                                        /** Format: uint64 */
                                        last_heartbeat_at_unix_ms?: number | null;
                                        /** Format: uint64 */
                                        last_meaningful_runtime_at_unix_ms?: number | null;
                                    };
                                    desired: {
                                        configuration_sha256: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        deployment_ref: string;
                                        /** Format: uint64 */
                                        generation: number;
                                        /** @enum {string} */
                                        phase: "running" | "stopped" | "absent";
                                        /** Format: uint64 */
                                        updated_at_unix_ms: number;
                                        version: string;
                                    };
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    id: string;
                                    observed: {
                                        error?: string | null;
                                        evidence_sha256?: string | null;
                                        /** Format: uint64 */
                                        generation: number;
                                        /** Format: uint64 */
                                        observed_at_unix_ms: number;
                                        /** @enum {string} */
                                        phase: "unknown" | "provisioning" | "starting" | "running" | "stopping" | "stopped" | "deleting" | "absent" | "failed";
                                        /** Format: uint32 */
                                        process_id?: number | null;
                                        version?: string | null;
                                    };
                                }[];
                                operations: {
                                    /** Format: uint32 */
                                    attempts: number;
                                    /** Format: uint64 */
                                    created_at_unix_ms: number;
                                    /** Format: uint64 */
                                    desired_generation: number;
                                    error?: string | null;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    id: string;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    instance_id: string;
                                    /** @enum {string} */
                                    kind: "provision" | "start" | "stop" | "restart" | "upgrade" | "delete";
                                    lease?: {
                                        /** Format: uint64 */
                                        acquired_at_unix_ms: number;
                                        /** Format: uint64 */
                                        epoch: number;
                                        /** Format: uint64 */
                                        expires_at_unix_ms: number;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        owner: string;
                                    } | null;
                                    receipts?: {
                                        /** Format: uint64 */
                                        at_unix_ms: number;
                                        /** @enum {string} */
                                        boundary: "prepared" | "applied" | "completed" | "failed";
                                        evidence_sha256: string;
                                        /** Format: uint64 */
                                        lease_epoch: number;
                                    }[];
                                    request_sha256: string;
                                    /** @enum {string} */
                                    state: "pending" | "leased" | "prepared" | "applied" | "succeeded" | "failed" | "superseded";
                                    /** Format: uint64 */
                                    updated_at_unix_ms: number;
                                }[];
                                /** Format: uint64 */
                                revision: number;
                                /** Format: uint64 */
                                updated_at_unix_ms: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "health-live": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                observed_at_unix_ms: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                observed_at_unix_ms: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "health-ready": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                backend: string;
                                /** Format: uint64 */
                                claim_sequence: number;
                                /** Format: uint64 */
                                observed_at_unix_ms: number;
                                /** Format: uint64 */
                                runtime_cursor: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                backend: string;
                                /** Format: uint64 */
                                claim_sequence: number;
                                /** Format: uint64 */
                                observed_at_unix_ms: number;
                                /** Format: uint64 */
                                runtime_cursor: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "query-execute": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        /**
                         * @default {
                         *       "max_batch_rows": 256,
                         *       "max_output_bytes": 524288,
                         *       "max_rows": 10000,
                         *       "max_scanned_changes": 100000
                         *     }
                         */
                        budget?: {
                            /** Format: uint64 */
                            max_batch_rows: number;
                            /** Format: uint64 */
                            max_output_bytes: number;
                            /** Format: uint64 */
                            max_rows: number;
                            /** Format: uint64 */
                            max_scanned_changes: number;
                        };
                        parameters?: {
                            [key: string]: {
                                /** @constant */
                                type: "null";
                            } | {
                                /** @constant */
                                type: "bool";
                                value: boolean;
                            } | {
                                /** @constant */
                                type: "integer";
                                /** Format: int64 */
                                value: number;
                            } | {
                                /** @constant */
                                type: "unsigned";
                                /** Format: uint64 */
                                value: number;
                            } | {
                                /** @constant */
                                type: "decimal";
                                value: string;
                            } | {
                                /** @constant */
                                type: "string";
                                value: string;
                            } | {
                                /** @constant */
                                type: "digest";
                                value: string;
                            } | {
                                /** @constant */
                                type: "list";
                                value: components["schemas"]["QueryValue"][];
                            } | {
                                /** @constant */
                                type: "map";
                                value: {
                                    [key: string]: components["schemas"]["QueryValue"];
                                };
                            };
                        };
                        query: string;
                        scope: string;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                canonical_query: string;
                                execution: {
                                    /** Format: uint64 */
                                    output_bytes: number;
                                    /** Format: uint64 */
                                    returned_rows: number;
                                    /** Format: uint64 */
                                    scanned_changes: number;
                                    stamp_validation: string;
                                    /** Format: uint64 */
                                    stamp_validation_max_changes: number;
                                    /** Format: uint16 */
                                    stamp_validation_proof_nodes: number;
                                    truncated: boolean;
                                };
                                /** Format: uint64 */
                                known_at_cursor: number;
                                plan: {
                                    authorization_boundary: string;
                                    candidates: {
                                        exact: boolean;
                                        name: string;
                                        reason: string;
                                        selected: boolean;
                                    }[];
                                    deterministic_order: string;
                                    exact: boolean;
                                    plan_sha256: string;
                                };
                                read_manifest_sha256: string;
                                rows: {
                                    identity: string;
                                    values: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                }[];
                                /** Format: uint64 */
                                schema_revision: number;
                                scope: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                canonical_query: string;
                                execution: {
                                    /** Format: uint64 */
                                    output_bytes: number;
                                    /** Format: uint64 */
                                    returned_rows: number;
                                    /** Format: uint64 */
                                    scanned_changes: number;
                                    stamp_validation: string;
                                    /** Format: uint64 */
                                    stamp_validation_max_changes: number;
                                    /** Format: uint16 */
                                    stamp_validation_proof_nodes: number;
                                    truncated: boolean;
                                };
                                /** Format: uint64 */
                                known_at_cursor: number;
                                plan: {
                                    authorization_boundary: string;
                                    candidates: {
                                        exact: boolean;
                                        name: string;
                                        reason: string;
                                        selected: boolean;
                                    }[];
                                    deterministic_order: string;
                                    exact: boolean;
                                    plan_sha256: string;
                                };
                                read_manifest_sha256: string;
                                rows: {
                                    identity: string;
                                    values: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                }[];
                                /** Format: uint64 */
                                schema_revision: number;
                                scope: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "query-index-ensure": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        /**
                         * @default {
                         *       "max_batch_rows": 256,
                         *       "max_output_bytes": 524288,
                         *       "max_rows": 10000,
                         *       "max_scanned_changes": 100000
                         *     }
                         */
                        budget?: {
                            /** Format: uint64 */
                            max_batch_rows: number;
                            /** Format: uint64 */
                            max_output_bytes: number;
                            /** Format: uint64 */
                            max_rows: number;
                            /** Format: uint64 */
                            max_scanned_changes: number;
                        };
                        definition_query: string;
                        /**
                         * @description A canonical public identifier component.
                         *
                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                         *     labels are separate data and may use arbitrary Unicode.
                         */
                        index_id: string;
                        scope: string;
                        /** @default false */
                        unique?: boolean;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                catalogue_revision: number;
                                idempotent_replay: boolean;
                                index: {
                                    /** Format: uint64 */
                                    artifact_rows?: number | null;
                                    artifact_sha256: string;
                                    /** Format: uint64 */
                                    built_valid_at?: number | null;
                                    configuration_sha256: string;
                                    definition_query: string;
                                    /** Format: uint64 */
                                    generation: number;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    index_id: string;
                                    /** Format: uint64 */
                                    source_cursor: number;
                                    /** @enum {string} */
                                    state: "building" | "ready" | "quarantined" | "retiring";
                                    unique: boolean;
                                };
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                catalogue_revision: number;
                                idempotent_replay: boolean;
                                index: {
                                    /** Format: uint64 */
                                    artifact_rows?: number | null;
                                    artifact_sha256: string;
                                    /** Format: uint64 */
                                    built_valid_at?: number | null;
                                    configuration_sha256: string;
                                    definition_query: string;
                                    /** Format: uint64 */
                                    generation: number;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    index_id: string;
                                    /** Format: uint64 */
                                    source_cursor: number;
                                    /** @enum {string} */
                                    state: "building" | "ready" | "quarantined" | "retiring";
                                    unique: boolean;
                                };
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "query-index-list": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        scope: string;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                indexes: {
                                    /** Format: uint64 */
                                    artifact_rows?: number | null;
                                    artifact_sha256: string;
                                    /** Format: uint64 */
                                    built_valid_at?: number | null;
                                    configuration_sha256: string;
                                    definition_query: string;
                                    /** Format: uint64 */
                                    generation: number;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    index_id: string;
                                    /** Format: uint64 */
                                    source_cursor: number;
                                    /** @enum {string} */
                                    state: "building" | "ready" | "quarantined" | "retiring";
                                    unique: boolean;
                                }[];
                                /** Format: uint64 */
                                revision: number;
                                scope: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                indexes: {
                                    /** Format: uint64 */
                                    artifact_rows?: number | null;
                                    artifact_sha256: string;
                                    /** Format: uint64 */
                                    built_valid_at?: number | null;
                                    configuration_sha256: string;
                                    definition_query: string;
                                    /** Format: uint64 */
                                    generation: number;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    index_id: string;
                                    /** Format: uint64 */
                                    source_cursor: number;
                                    /** @enum {string} */
                                    state: "building" | "ready" | "quarantined" | "retiring";
                                    unique: boolean;
                                }[];
                                /** Format: uint64 */
                                revision: number;
                                scope: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "query-live-poll": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        /** Format: uint64 */
                        after_cursor: number;
                        /**
                         * @default {
                         *       "max_batch_rows": 256,
                         *       "max_output_bytes": 524288,
                         *       "max_rows": 10000,
                         *       "max_scanned_changes": 100000
                         *     }
                         */
                        budget?: {
                            /** Format: uint64 */
                            max_batch_rows: number;
                            /** Format: uint64 */
                            max_output_bytes: number;
                            /** Format: uint64 */
                            max_rows: number;
                            /** Format: uint64 */
                            max_scanned_changes: number;
                        };
                        /** Format: uint64 */
                        max_delta_rows: number;
                        parameters?: {
                            [key: string]: {
                                /** @constant */
                                type: "null";
                            } | {
                                /** @constant */
                                type: "bool";
                                value: boolean;
                            } | {
                                /** @constant */
                                type: "integer";
                                /** Format: int64 */
                                value: number;
                            } | {
                                /** @constant */
                                type: "unsigned";
                                /** Format: uint64 */
                                value: number;
                            } | {
                                /** @constant */
                                type: "decimal";
                                value: string;
                            } | {
                                /** @constant */
                                type: "string";
                                value: string;
                            } | {
                                /** @constant */
                                type: "digest";
                                value: string;
                            } | {
                                /** @constant */
                                type: "list";
                                value: components["schemas"]["QueryValue"][];
                            } | {
                                /** @constant */
                                type: "map";
                                value: {
                                    [key: string]: components["schemas"]["QueryValue"];
                                };
                            };
                        };
                        query: string;
                        scope: string;
                        /**
                         * Format: uint64
                         * @default 0
                         */
                        wait_timeout_ms?: number;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                added: {
                                    identity: string;
                                    values: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                }[];
                                /** Format: uint64 */
                                from_cursor: number;
                                /** Format: uint64 */
                                head_cursor: number;
                                query_sha256: string;
                                removed: {
                                    identity: string;
                                    values: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                }[];
                                /** Format: uint64 */
                                through_cursor: number;
                                timed_out: boolean;
                                updated: {
                                    after: {
                                        identity: string;
                                        values: {
                                            [key: string]: {
                                                /** @constant */
                                                type: "null";
                                            } | {
                                                /** @constant */
                                                type: "bool";
                                                value: boolean;
                                            } | {
                                                /** @constant */
                                                type: "integer";
                                                /** Format: int64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "unsigned";
                                                /** Format: uint64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "decimal";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "string";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "digest";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "list";
                                                value: components["schemas"]["QueryValue"][];
                                            } | {
                                                /** @constant */
                                                type: "map";
                                                value: {
                                                    [key: string]: components["schemas"]["QueryValue"];
                                                };
                                            };
                                        };
                                    };
                                    before: {
                                        identity: string;
                                        values: {
                                            [key: string]: {
                                                /** @constant */
                                                type: "null";
                                            } | {
                                                /** @constant */
                                                type: "bool";
                                                value: boolean;
                                            } | {
                                                /** @constant */
                                                type: "integer";
                                                /** Format: int64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "unsigned";
                                                /** Format: uint64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "decimal";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "string";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "digest";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "list";
                                                value: components["schemas"]["QueryValue"][];
                                            } | {
                                                /** @constant */
                                                type: "map";
                                                value: {
                                                    [key: string]: components["schemas"]["QueryValue"];
                                                };
                                            };
                                        };
                                    };
                                }[];
                                /** Format: uint64 */
                                waited_ms: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                added: {
                                    identity: string;
                                    values: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                }[];
                                /** Format: uint64 */
                                from_cursor: number;
                                /** Format: uint64 */
                                head_cursor: number;
                                query_sha256: string;
                                removed: {
                                    identity: string;
                                    values: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                }[];
                                /** Format: uint64 */
                                through_cursor: number;
                                timed_out: boolean;
                                updated: {
                                    after: {
                                        identity: string;
                                        values: {
                                            [key: string]: {
                                                /** @constant */
                                                type: "null";
                                            } | {
                                                /** @constant */
                                                type: "bool";
                                                value: boolean;
                                            } | {
                                                /** @constant */
                                                type: "integer";
                                                /** Format: int64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "unsigned";
                                                /** Format: uint64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "decimal";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "string";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "digest";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "list";
                                                value: components["schemas"]["QueryValue"][];
                                            } | {
                                                /** @constant */
                                                type: "map";
                                                value: {
                                                    [key: string]: components["schemas"]["QueryValue"];
                                                };
                                            };
                                        };
                                    };
                                    before: {
                                        identity: string;
                                        values: {
                                            [key: string]: {
                                                /** @constant */
                                                type: "null";
                                            } | {
                                                /** @constant */
                                                type: "bool";
                                                value: boolean;
                                            } | {
                                                /** @constant */
                                                type: "integer";
                                                /** Format: int64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "unsigned";
                                                /** Format: uint64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "decimal";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "string";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "digest";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "list";
                                                value: components["schemas"]["QueryValue"][];
                                            } | {
                                                /** @constant */
                                                type: "map";
                                                value: {
                                                    [key: string]: components["schemas"]["QueryValue"];
                                                };
                                            };
                                        };
                                    };
                                }[];
                                /** Format: uint64 */
                                waited_ms: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "restore-create": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        backup_sha256: string;
                        /**
                         * @description A canonical public identifier component.
                         *
                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                         *     labels are separate data and may use arbitrary Unicode.
                         */
                        restore_id: string;
                        /** Format: uint64 */
                        restored_at_unix_ms: number;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                backup_sha256: string;
                                idempotent_replay: boolean;
                                inventory: {
                                    /** Format: uint64 */
                                    action_count: number;
                                    archive_sha256: string;
                                    /** Format: uint64 */
                                    claim_sequence: number;
                                    /** Format: uint16 */
                                    contract_version: number;
                                    /** Format: uint16 */
                                    format_version: number;
                                    /** Format: uint64 */
                                    payload_bytes: number;
                                    /** Format: uint64 */
                                    runtime_commits: number;
                                    /** Format: uint64 */
                                    runtime_cursor: number;
                                    /** Format: uint64 */
                                    runtime_mutations: number;
                                    /** Format: uint64 */
                                    standalone_claims: number;
                                };
                                reopened: boolean;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                restore_id: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                backup_sha256: string;
                                idempotent_replay: boolean;
                                inventory: {
                                    /** Format: uint64 */
                                    action_count: number;
                                    archive_sha256: string;
                                    /** Format: uint64 */
                                    claim_sequence: number;
                                    /** Format: uint16 */
                                    contract_version: number;
                                    /** Format: uint16 */
                                    format_version: number;
                                    /** Format: uint64 */
                                    payload_bytes: number;
                                    /** Format: uint64 */
                                    runtime_commits: number;
                                    /** Format: uint64 */
                                    runtime_cursor: number;
                                    /** Format: uint64 */
                                    runtime_mutations: number;
                                    /** Format: uint64 */
                                    standalone_claims: number;
                                };
                                reopened: boolean;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                restore_id: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "endpoint-catalogue": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                endpoints: {
                                    /** @enum {string} */
                                    action: "service_inspect" | "unknown_request" | "session_create" | "session_renew" | "session_close" | "query_execute" | "query_live_poll" | "query_index_ensure" | "query_index_list" | "transaction_begin" | "transaction_preview" | "transaction_commit" | "transaction_abort" | "changefeed_read" | "changefeed_follow" | "vector_collection_ensure" | "vector_collection_list" | "vector_search" | "backup_create" | "backup_list" | "restore_create" | "estate_read" | "audit_read" | "security_admin";
                                    /** @enum {string} */
                                    authentication: "public" | "api_key" | "session_bearer";
                                    /** @enum {string} */
                                    method: "get" | "post" | "delete";
                                    mutation: boolean;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    operation: string;
                                    path: string;
                                    request_type: string;
                                    response_type: string;
                                }[];
                                protocol: string;
                                /** Format: uint16 */
                                protocol_version: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                endpoints: {
                                    /** @enum {string} */
                                    action: "service_inspect" | "unknown_request" | "session_create" | "session_renew" | "session_close" | "query_execute" | "query_live_poll" | "query_index_ensure" | "query_index_list" | "transaction_begin" | "transaction_preview" | "transaction_commit" | "transaction_abort" | "changefeed_read" | "changefeed_follow" | "vector_collection_ensure" | "vector_collection_list" | "vector_search" | "backup_create" | "backup_list" | "restore_create" | "estate_read" | "audit_read" | "security_admin";
                                    /** @enum {string} */
                                    authentication: "public" | "api_key" | "session_bearer";
                                    /** @enum {string} */
                                    method: "get" | "post" | "delete";
                                    mutation: boolean;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    operation: string;
                                    path: string;
                                    request_type: string;
                                    response_type: string;
                                }[];
                                protocol: string;
                                /** Format: uint16 */
                                protocol_version: number;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "openapi-read": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: unknown;
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: unknown;
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "session-create": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Principal": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        /**
                         * @description Resource limits requested for one loopback transport session. These are
                         *     availability boundaries, not authentication or authorization policy.
                         */
                        limits: {
                            /** Format: uint64 */
                            absolute_timeout_ms: number;
                            /** Format: uint64 */
                            idle_timeout_ms: number;
                            /** Format: uint16 */
                            max_open_transactions: number;
                        };
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                absolute_expires_at_unix_ms: number;
                                /** Format: uint64 */
                                idle_expires_at_unix_ms: number;
                                /** Format: uint64 */
                                issued_at_unix_ms: number;
                                /**
                                 * @description Resource limits requested for one loopback transport session. These are
                                 *     availability boundaries, not authentication or authorization policy.
                                 */
                                limits: {
                                    /** Format: uint64 */
                                    absolute_timeout_ms: number;
                                    /** Format: uint64 */
                                    idle_timeout_ms: number;
                                    /** Format: uint16 */
                                    max_open_transactions: number;
                                };
                                session_id: string;
                                token: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                absolute_expires_at_unix_ms: number;
                                /** Format: uint64 */
                                idle_expires_at_unix_ms: number;
                                /** Format: uint64 */
                                issued_at_unix_ms: number;
                                /**
                                 * @description Resource limits requested for one loopback transport session. These are
                                 *     availability boundaries, not authentication or authorization policy.
                                 */
                                limits: {
                                    /** Format: uint64 */
                                    absolute_timeout_ms: number;
                                    /** Format: uint64 */
                                    idle_timeout_ms: number;
                                    /** Format: uint16 */
                                    max_open_transactions: number;
                                };
                                session_id: string;
                                token: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "session-close": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path: {
                session: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: Record<string, never>;
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint16 */
                                affected_open_transactions: number;
                                /** Format: uint64 */
                                ended_at_unix_ms: number;
                                idempotent_replay: boolean;
                                session_id: string;
                                /** @enum {string} */
                                state: "closed" | "expired";
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint16 */
                                affected_open_transactions: number;
                                /** Format: uint64 */
                                ended_at_unix_ms: number;
                                idempotent_replay: boolean;
                                session_id: string;
                                /** @enum {string} */
                                state: "closed" | "expired";
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "session-renew": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path: {
                session: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: Record<string, never>;
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                absolute_expires_at_unix_ms: number;
                                /** Format: uint64 */
                                idle_expires_at_unix_ms: number;
                                /** Format: uint64 */
                                issued_at_unix_ms: number;
                                /**
                                 * @description Resource limits requested for one loopback transport session. These are
                                 *     availability boundaries, not authentication or authorization policy.
                                 */
                                limits: {
                                    /** Format: uint64 */
                                    absolute_timeout_ms: number;
                                    /** Format: uint64 */
                                    idle_timeout_ms: number;
                                    /** Format: uint16 */
                                    max_open_transactions: number;
                                };
                                session_id: string;
                                token: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                absolute_expires_at_unix_ms: number;
                                /** Format: uint64 */
                                idle_expires_at_unix_ms: number;
                                /** Format: uint64 */
                                issued_at_unix_ms: number;
                                /**
                                 * @description Resource limits requested for one loopback transport session. These are
                                 *     availability boundaries, not authentication or authorization policy.
                                 */
                                limits: {
                                    /** Format: uint64 */
                                    absolute_timeout_ms: number;
                                    /** Format: uint64 */
                                    idle_timeout_ms: number;
                                    /** Format: uint16 */
                                    max_open_transactions: number;
                                };
                                session_id: string;
                                token: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "transaction-begin": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        /**
                         * @description A canonical public identifier component.
                         *
                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                         *     labels are separate data and may use arbitrary Unicode.
                         */
                        scope: string;
                        /** Format: uint64 */
                        timeout_ms: number;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                expires_at_unix_ms: number;
                                /** Format: uint64 */
                                read_cursor: number;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                scope: string;
                                session_id: string;
                                /** @enum {string} */
                                state: "open" | "committed" | "aborted" | "expired";
                                transaction_id: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                expires_at_unix_ms: number;
                                /** Format: uint64 */
                                read_cursor: number;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                scope: string;
                                session_id: string;
                                /** @enum {string} */
                                state: "open" | "committed" | "aborted" | "expired";
                                transaction_id: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "transaction-abort": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path: {
                transaction: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: Record<string, never>;
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                expires_at_unix_ms: number;
                                /** Format: uint64 */
                                read_cursor: number;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                scope: string;
                                session_id: string;
                                /** @enum {string} */
                                state: "open" | "committed" | "aborted" | "expired";
                                transaction_id: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                expires_at_unix_ms: number;
                                /** Format: uint64 */
                                read_cursor: number;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                scope: string;
                                session_id: string;
                                /** @enum {string} */
                                state: "open" | "committed" | "aborted" | "expired";
                                transaction_id: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "transaction-commit": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path: {
                transaction: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        mutations: ({
                            /** Format: float */
                            confidence?: number | null;
                            /** @constant */
                            mutation: "assert_claim";
                            object: string;
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            predicate: string;
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            producer: string;
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            subject: string;
                            /** Format: uint64 */
                            tx_time: number;
                            /** Format: uint64 */
                            valid_from: number;
                        } | {
                            /** @constant */
                            mutation: "put_schema";
                            registry: {
                                /** @default {} */
                                events?: {
                                    [key: string]: {
                                        /** @default false */
                                        allow_additional_properties?: boolean;
                                        /** @default {} */
                                        properties?: {
                                            [key: string]: {
                                                /** @default false */
                                                required?: boolean;
                                                /** @enum {string} */
                                                value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                            };
                                        };
                                        /** @default false */
                                        subject_required?: boolean;
                                        /** @default [] */
                                        subject_types?: string[];
                                    };
                                };
                                migration: string;
                                /** @default {} */
                                records?: {
                                    [key: string]: {
                                        /** @default false */
                                        allow_additional_properties?: boolean;
                                        /** @default {} */
                                        properties?: {
                                            [key: string]: {
                                                /** @default false */
                                                required?: boolean;
                                                /** @enum {string} */
                                                value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                            };
                                        };
                                        /** @default [] */
                                        unique_properties?: string[];
                                    };
                                };
                                /** @default {} */
                                relations?: {
                                    [key: string]: {
                                        /** @default false */
                                        allow_additional_properties?: boolean;
                                        /** @default [] */
                                        from?: string[];
                                        /** Format: uint64 */
                                        max_incoming?: number | null;
                                        /** Format: uint64 */
                                        max_outgoing?: number | null;
                                        /** @default {} */
                                        properties?: {
                                            [key: string]: {
                                                /** @default false */
                                                required?: boolean;
                                                /** @enum {string} */
                                                value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                            };
                                        };
                                        /** @default [] */
                                        to?: string[];
                                        /** @default false */
                                        unique_pair?: boolean;
                                    };
                                };
                                /** Format: uint64 */
                                revision: number;
                            };
                        } | {
                            /** @constant */
                            mutation: "put_record";
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            reference: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            /** Format: uint64 */
                            valid_from: number;
                            /** Format: uint64 */
                            valid_to?: number | null;
                        } | {
                            from: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            /** @constant */
                            mutation: "put_relation";
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            reference: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            to: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            /** Format: uint64 */
                            valid_from: number;
                            /** Format: uint64 */
                            valid_to?: number | null;
                        } | {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            kind: string;
                            /** @constant */
                            mutation: "append_event";
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            subject?: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            } | null;
                        } | {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            collection_id?: string | null;
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            field: string;
                            /** @constant */
                            mutation: "put_vector";
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            provenance?: {
                                /** Format: uint32 */
                                dimensions: number;
                                /** @default {} */
                                generation_parameters?: {
                                    [key: string]: {
                                        /** @constant */
                                        type: "null";
                                    } | {
                                        /** @constant */
                                        type: "bool";
                                        value: boolean;
                                    } | {
                                        /** @constant */
                                        type: "integer";
                                        /** Format: int64 */
                                        value: number;
                                    } | {
                                        /** @constant */
                                        type: "unsigned";
                                        /** Format: uint64 */
                                        value: number;
                                    } | {
                                        /** @constant */
                                        type: "decimal";
                                        value: string;
                                    } | {
                                        /** @constant */
                                        type: "string";
                                        value: string;
                                    } | {
                                        /** @constant */
                                        type: "digest";
                                        value: string;
                                    } | {
                                        /** @constant */
                                        type: "list";
                                        value: components["schemas"]["QueryValue"][];
                                    } | {
                                        /** @constant */
                                        type: "map";
                                        value: {
                                            [key: string]: components["schemas"]["QueryValue"];
                                        };
                                    };
                                };
                                model: string;
                                model_sha256: string;
                                /** @enum {string} */
                                normalization: "none" | "unit_l2";
                                source_sha256: string;
                            } | null;
                            reference: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            subject: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            /** Format: uint64 */
                            valid_from: number;
                            /** Format: uint64 */
                            valid_to?: number | null;
                            value: {
                                /** @constant */
                                kind: "dense";
                                values: number[];
                            } | {
                                /** Format: uint32 */
                                dimensions: number;
                                indices: number[];
                                /** @constant */
                                kind: "sparse";
                                values: number[];
                            } | {
                                /** Format: uint32 */
                                dimensions: number;
                                /** @constant */
                                kind: "multi_dense";
                                vectors: number[][];
                            };
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            vector_name?: string | null;
                        } | {
                            /** @constant */
                            mutation: "append_series_sample";
                            /** Format: uint64 */
                            observed_at: number;
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            reference: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            series: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            value: {
                                /** @constant */
                                type: "integer";
                                /** Format: int64 */
                                value: number;
                            } | {
                                /** @constant */
                                type: "unsigned";
                                /** Format: uint64 */
                                value: number;
                            } | {
                                /** @constant */
                                type: "decimal";
                                value: string;
                            } | {
                                /** @constant */
                                type: "bool";
                                value: boolean;
                            } | {
                                /** @constant */
                                type: "string";
                                value: string;
                            };
                        } | {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            field: string;
                            /** @constant */
                            mutation: "put_geo";
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            reference: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            subject: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            /** Format: uint64 */
                            valid_from: number;
                            /** Format: uint64 */
                            valid_to?: number | null;
                            value: {
                                /** @constant */
                                kind: "point";
                                point: {
                                    /** Format: double */
                                    latitude: number;
                                    /** Format: double */
                                    longitude: number;
                                };
                            } | {
                                /** @constant */
                                kind: "bounding_box";
                                northeast: {
                                    /** Format: double */
                                    latitude: number;
                                    /** Format: double */
                                    longitude: number;
                                };
                                southwest: {
                                    /** Format: double */
                                    latitude: number;
                                    /** Format: double */
                                    longitude: number;
                                };
                            };
                        } | {
                            /** Format: uint64 */
                            length: number;
                            media_type: string;
                            /** @constant */
                            mutation: "publish_object_reference";
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            receipt: {
                                backend: string;
                                etag?: string | null;
                                key: string;
                                version?: string | null;
                            };
                            reference: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            sha256: string;
                            subject?: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            } | null;
                        })[];
                        operation_sha256: string;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                claim_mutation_count?: number | null;
                                /** Format: uint64 */
                                first_claim_sequence: number;
                                /** Format: uint64 */
                                first_runtime_cursor?: number | null;
                                idempotent_replay: boolean;
                                /** Format: uint64 */
                                last_claim_sequence: number;
                                /** Format: uint64 */
                                last_runtime_cursor?: number | null;
                                /** Format: uint64 */
                                mutation_count: number;
                                operation_sha256: string;
                                runtime_commit_sha256?: string | null;
                                transaction_id: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                claim_mutation_count?: number | null;
                                /** Format: uint64 */
                                first_claim_sequence: number;
                                /** Format: uint64 */
                                first_runtime_cursor?: number | null;
                                idempotent_replay: boolean;
                                /** Format: uint64 */
                                last_claim_sequence: number;
                                /** Format: uint64 */
                                last_runtime_cursor?: number | null;
                                /** Format: uint64 */
                                mutation_count: number;
                                operation_sha256: string;
                                runtime_commit_sha256?: string | null;
                                transaction_id: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "transaction-preview": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path: {
                transaction: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        mutations: ({
                            /** Format: float */
                            confidence?: number | null;
                            /** @constant */
                            mutation: "assert_claim";
                            object: string;
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            predicate: string;
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            producer: string;
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            subject: string;
                            /** Format: uint64 */
                            tx_time: number;
                            /** Format: uint64 */
                            valid_from: number;
                        } | {
                            /** @constant */
                            mutation: "put_schema";
                            registry: {
                                /** @default {} */
                                events?: {
                                    [key: string]: {
                                        /** @default false */
                                        allow_additional_properties?: boolean;
                                        /** @default {} */
                                        properties?: {
                                            [key: string]: {
                                                /** @default false */
                                                required?: boolean;
                                                /** @enum {string} */
                                                value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                            };
                                        };
                                        /** @default false */
                                        subject_required?: boolean;
                                        /** @default [] */
                                        subject_types?: string[];
                                    };
                                };
                                migration: string;
                                /** @default {} */
                                records?: {
                                    [key: string]: {
                                        /** @default false */
                                        allow_additional_properties?: boolean;
                                        /** @default {} */
                                        properties?: {
                                            [key: string]: {
                                                /** @default false */
                                                required?: boolean;
                                                /** @enum {string} */
                                                value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                            };
                                        };
                                        /** @default [] */
                                        unique_properties?: string[];
                                    };
                                };
                                /** @default {} */
                                relations?: {
                                    [key: string]: {
                                        /** @default false */
                                        allow_additional_properties?: boolean;
                                        /** @default [] */
                                        from?: string[];
                                        /** Format: uint64 */
                                        max_incoming?: number | null;
                                        /** Format: uint64 */
                                        max_outgoing?: number | null;
                                        /** @default {} */
                                        properties?: {
                                            [key: string]: {
                                                /** @default false */
                                                required?: boolean;
                                                /** @enum {string} */
                                                value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                            };
                                        };
                                        /** @default [] */
                                        to?: string[];
                                        /** @default false */
                                        unique_pair?: boolean;
                                    };
                                };
                                /** Format: uint64 */
                                revision: number;
                            };
                        } | {
                            /** @constant */
                            mutation: "put_record";
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            reference: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            /** Format: uint64 */
                            valid_from: number;
                            /** Format: uint64 */
                            valid_to?: number | null;
                        } | {
                            from: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            /** @constant */
                            mutation: "put_relation";
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            reference: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            to: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            /** Format: uint64 */
                            valid_from: number;
                            /** Format: uint64 */
                            valid_to?: number | null;
                        } | {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            kind: string;
                            /** @constant */
                            mutation: "append_event";
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            subject?: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            } | null;
                        } | {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            collection_id?: string | null;
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            field: string;
                            /** @constant */
                            mutation: "put_vector";
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            provenance?: {
                                /** Format: uint32 */
                                dimensions: number;
                                /** @default {} */
                                generation_parameters?: {
                                    [key: string]: {
                                        /** @constant */
                                        type: "null";
                                    } | {
                                        /** @constant */
                                        type: "bool";
                                        value: boolean;
                                    } | {
                                        /** @constant */
                                        type: "integer";
                                        /** Format: int64 */
                                        value: number;
                                    } | {
                                        /** @constant */
                                        type: "unsigned";
                                        /** Format: uint64 */
                                        value: number;
                                    } | {
                                        /** @constant */
                                        type: "decimal";
                                        value: string;
                                    } | {
                                        /** @constant */
                                        type: "string";
                                        value: string;
                                    } | {
                                        /** @constant */
                                        type: "digest";
                                        value: string;
                                    } | {
                                        /** @constant */
                                        type: "list";
                                        value: components["schemas"]["QueryValue"][];
                                    } | {
                                        /** @constant */
                                        type: "map";
                                        value: {
                                            [key: string]: components["schemas"]["QueryValue"];
                                        };
                                    };
                                };
                                model: string;
                                model_sha256: string;
                                /** @enum {string} */
                                normalization: "none" | "unit_l2";
                                source_sha256: string;
                            } | null;
                            reference: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            subject: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            /** Format: uint64 */
                            valid_from: number;
                            /** Format: uint64 */
                            valid_to?: number | null;
                            value: {
                                /** @constant */
                                kind: "dense";
                                values: number[];
                            } | {
                                /** Format: uint32 */
                                dimensions: number;
                                indices: number[];
                                /** @constant */
                                kind: "sparse";
                                values: number[];
                            } | {
                                /** Format: uint32 */
                                dimensions: number;
                                /** @constant */
                                kind: "multi_dense";
                                vectors: number[][];
                            };
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            vector_name?: string | null;
                        } | {
                            /** @constant */
                            mutation: "append_series_sample";
                            /** Format: uint64 */
                            observed_at: number;
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            reference: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            series: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            value: {
                                /** @constant */
                                type: "integer";
                                /** Format: int64 */
                                value: number;
                            } | {
                                /** @constant */
                                type: "unsigned";
                                /** Format: uint64 */
                                value: number;
                            } | {
                                /** @constant */
                                type: "decimal";
                                value: string;
                            } | {
                                /** @constant */
                                type: "bool";
                                value: boolean;
                            } | {
                                /** @constant */
                                type: "string";
                                value: string;
                            };
                        } | {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            field: string;
                            /** @constant */
                            mutation: "put_geo";
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            reference: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            subject: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            /** Format: uint64 */
                            valid_from: number;
                            /** Format: uint64 */
                            valid_to?: number | null;
                            value: {
                                /** @constant */
                                kind: "point";
                                point: {
                                    /** Format: double */
                                    latitude: number;
                                    /** Format: double */
                                    longitude: number;
                                };
                            } | {
                                /** @constant */
                                kind: "bounding_box";
                                northeast: {
                                    /** Format: double */
                                    latitude: number;
                                    /** Format: double */
                                    longitude: number;
                                };
                                southwest: {
                                    /** Format: double */
                                    latitude: number;
                                    /** Format: double */
                                    longitude: number;
                                };
                            };
                        } | {
                            /** Format: uint64 */
                            length: number;
                            media_type: string;
                            /** @constant */
                            mutation: "publish_object_reference";
                            /** @default {} */
                            properties?: {
                                [key: string]: {
                                    /** @constant */
                                    type: "null";
                                } | {
                                    /** @constant */
                                    type: "bool";
                                    value: boolean;
                                } | {
                                    /** @constant */
                                    type: "integer";
                                    /** Format: int64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "unsigned";
                                    /** Format: uint64 */
                                    value: number;
                                } | {
                                    /** @constant */
                                    type: "decimal";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "string";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "digest";
                                    value: string;
                                } | {
                                    /** @constant */
                                    type: "list";
                                    value: components["schemas"]["QueryValue"][];
                                } | {
                                    /** @constant */
                                    type: "map";
                                    value: {
                                        [key: string]: components["schemas"]["QueryValue"];
                                    };
                                };
                            };
                            receipt: {
                                backend: string;
                                etag?: string | null;
                                key: string;
                                version?: string | null;
                            };
                            reference: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            };
                            sha256: string;
                            subject?: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                id: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                kind: string;
                            } | null;
                        })[];
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                mutations: ({
                                    /** Format: float */
                                    confidence?: number | null;
                                    /** @constant */
                                    mutation: "assert_claim";
                                    object: string;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    predicate: string;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    producer: string;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    subject: string;
                                    /** Format: uint64 */
                                    tx_time: number;
                                    /** Format: uint64 */
                                    valid_from: number;
                                } | {
                                    /** @constant */
                                    mutation: "put_schema";
                                    registry: {
                                        /** @default {} */
                                        events: {
                                            [key: string]: {
                                                /** @default false */
                                                allow_additional_properties: boolean;
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @default false */
                                                        required: boolean;
                                                        /** @enum {string} */
                                                        value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                    };
                                                };
                                                /** @default false */
                                                subject_required: boolean;
                                                /** @default [] */
                                                subject_types: string[];
                                            };
                                        };
                                        migration: string;
                                        /** @default {} */
                                        records: {
                                            [key: string]: {
                                                /** @default false */
                                                allow_additional_properties: boolean;
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @default false */
                                                        required: boolean;
                                                        /** @enum {string} */
                                                        value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                    };
                                                };
                                                /** @default [] */
                                                unique_properties: string[];
                                            };
                                        };
                                        /** @default {} */
                                        relations: {
                                            [key: string]: {
                                                /** @default false */
                                                allow_additional_properties: boolean;
                                                /** @default [] */
                                                from: string[];
                                                /** Format: uint64 */
                                                max_incoming?: number | null;
                                                /** Format: uint64 */
                                                max_outgoing?: number | null;
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @default false */
                                                        required: boolean;
                                                        /** @enum {string} */
                                                        value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                    };
                                                };
                                                /** @default [] */
                                                to: string[];
                                                /** @default false */
                                                unique_pair: boolean;
                                            };
                                        };
                                        /** Format: uint64 */
                                        revision: number;
                                    };
                                } | {
                                    /** @constant */
                                    mutation: "put_record";
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    /** Format: uint64 */
                                    valid_from: number;
                                    /** Format: uint64 */
                                    valid_to?: number | null;
                                } | {
                                    from: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    /** @constant */
                                    mutation: "put_relation";
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    to: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    /** Format: uint64 */
                                    valid_from: number;
                                    /** Format: uint64 */
                                    valid_to?: number | null;
                                } | {
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    kind: string;
                                    /** @constant */
                                    mutation: "append_event";
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    subject?: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    } | null;
                                } | {
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    collection_id?: string | null;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    field: string;
                                    /** @constant */
                                    mutation: "put_vector";
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    provenance?: {
                                        /** Format: uint32 */
                                        dimensions: number;
                                        /** @default {} */
                                        generation_parameters: {
                                            [key: string]: {
                                                /** @constant */
                                                type: "null";
                                            } | {
                                                /** @constant */
                                                type: "bool";
                                                value: boolean;
                                            } | {
                                                /** @constant */
                                                type: "integer";
                                                /** Format: int64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "unsigned";
                                                /** Format: uint64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "decimal";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "string";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "digest";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "list";
                                                value: components["schemas"]["QueryValue"][];
                                            } | {
                                                /** @constant */
                                                type: "map";
                                                value: {
                                                    [key: string]: components["schemas"]["QueryValue"];
                                                };
                                            };
                                        };
                                        model: string;
                                        model_sha256: string;
                                        /** @enum {string} */
                                        normalization: "none" | "unit_l2";
                                        source_sha256: string;
                                    } | null;
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    subject: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    /** Format: uint64 */
                                    valid_from: number;
                                    /** Format: uint64 */
                                    valid_to?: number | null;
                                    value: {
                                        /** @constant */
                                        kind: "dense";
                                        values: number[];
                                    } | {
                                        /** Format: uint32 */
                                        dimensions: number;
                                        indices: number[];
                                        /** @constant */
                                        kind: "sparse";
                                        values: number[];
                                    } | {
                                        /** Format: uint32 */
                                        dimensions: number;
                                        /** @constant */
                                        kind: "multi_dense";
                                        vectors: number[][];
                                    };
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    vector_name?: string | null;
                                } | {
                                    /** @constant */
                                    mutation: "append_series_sample";
                                    /** Format: uint64 */
                                    observed_at: number;
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    series: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    value: {
                                        /** @constant */
                                        type: "integer";
                                        /** Format: int64 */
                                        value: number;
                                    } | {
                                        /** @constant */
                                        type: "unsigned";
                                        /** Format: uint64 */
                                        value: number;
                                    } | {
                                        /** @constant */
                                        type: "decimal";
                                        value: string;
                                    } | {
                                        /** @constant */
                                        type: "bool";
                                        value: boolean;
                                    } | {
                                        /** @constant */
                                        type: "string";
                                        value: string;
                                    };
                                } | {
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    field: string;
                                    /** @constant */
                                    mutation: "put_geo";
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    subject: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    /** Format: uint64 */
                                    valid_from: number;
                                    /** Format: uint64 */
                                    valid_to?: number | null;
                                    value: {
                                        /** @constant */
                                        kind: "point";
                                        point: {
                                            /** Format: double */
                                            latitude: number;
                                            /** Format: double */
                                            longitude: number;
                                        };
                                    } | {
                                        /** @constant */
                                        kind: "bounding_box";
                                        northeast: {
                                            /** Format: double */
                                            latitude: number;
                                            /** Format: double */
                                            longitude: number;
                                        };
                                        southwest: {
                                            /** Format: double */
                                            latitude: number;
                                            /** Format: double */
                                            longitude: number;
                                        };
                                    };
                                } | {
                                    /** Format: uint64 */
                                    length: number;
                                    media_type: string;
                                    /** @constant */
                                    mutation: "publish_object_reference";
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    receipt: {
                                        backend: string;
                                        etag?: string | null;
                                        key: string;
                                        version?: string | null;
                                    };
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    sha256: string;
                                    subject?: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    } | null;
                                })[];
                                operation_sha256: string;
                                /** Format: uint64 */
                                read_cursor: number;
                                transaction_id: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                mutations: ({
                                    /** Format: float */
                                    confidence?: number | null;
                                    /** @constant */
                                    mutation: "assert_claim";
                                    object: string;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    predicate: string;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    producer: string;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    subject: string;
                                    /** Format: uint64 */
                                    tx_time: number;
                                    /** Format: uint64 */
                                    valid_from: number;
                                } | {
                                    /** @constant */
                                    mutation: "put_schema";
                                    registry: {
                                        /** @default {} */
                                        events: {
                                            [key: string]: {
                                                /** @default false */
                                                allow_additional_properties: boolean;
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @default false */
                                                        required: boolean;
                                                        /** @enum {string} */
                                                        value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                    };
                                                };
                                                /** @default false */
                                                subject_required: boolean;
                                                /** @default [] */
                                                subject_types: string[];
                                            };
                                        };
                                        migration: string;
                                        /** @default {} */
                                        records: {
                                            [key: string]: {
                                                /** @default false */
                                                allow_additional_properties: boolean;
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @default false */
                                                        required: boolean;
                                                        /** @enum {string} */
                                                        value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                    };
                                                };
                                                /** @default [] */
                                                unique_properties: string[];
                                            };
                                        };
                                        /** @default {} */
                                        relations: {
                                            [key: string]: {
                                                /** @default false */
                                                allow_additional_properties: boolean;
                                                /** @default [] */
                                                from: string[];
                                                /** Format: uint64 */
                                                max_incoming?: number | null;
                                                /** Format: uint64 */
                                                max_outgoing?: number | null;
                                                /** @default {} */
                                                properties: {
                                                    [key: string]: {
                                                        /** @default false */
                                                        required: boolean;
                                                        /** @enum {string} */
                                                        value_type: "null" | "bool" | "integer" | "unsigned" | "decimal" | "string" | "digest" | "list" | "map";
                                                    };
                                                };
                                                /** @default [] */
                                                to: string[];
                                                /** @default false */
                                                unique_pair: boolean;
                                            };
                                        };
                                        /** Format: uint64 */
                                        revision: number;
                                    };
                                } | {
                                    /** @constant */
                                    mutation: "put_record";
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    /** Format: uint64 */
                                    valid_from: number;
                                    /** Format: uint64 */
                                    valid_to?: number | null;
                                } | {
                                    from: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    /** @constant */
                                    mutation: "put_relation";
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    to: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    /** Format: uint64 */
                                    valid_from: number;
                                    /** Format: uint64 */
                                    valid_to?: number | null;
                                } | {
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    kind: string;
                                    /** @constant */
                                    mutation: "append_event";
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    subject?: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    } | null;
                                } | {
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    collection_id?: string | null;
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    field: string;
                                    /** @constant */
                                    mutation: "put_vector";
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    provenance?: {
                                        /** Format: uint32 */
                                        dimensions: number;
                                        /** @default {} */
                                        generation_parameters: {
                                            [key: string]: {
                                                /** @constant */
                                                type: "null";
                                            } | {
                                                /** @constant */
                                                type: "bool";
                                                value: boolean;
                                            } | {
                                                /** @constant */
                                                type: "integer";
                                                /** Format: int64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "unsigned";
                                                /** Format: uint64 */
                                                value: number;
                                            } | {
                                                /** @constant */
                                                type: "decimal";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "string";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "digest";
                                                value: string;
                                            } | {
                                                /** @constant */
                                                type: "list";
                                                value: components["schemas"]["QueryValue"][];
                                            } | {
                                                /** @constant */
                                                type: "map";
                                                value: {
                                                    [key: string]: components["schemas"]["QueryValue"];
                                                };
                                            };
                                        };
                                        model: string;
                                        model_sha256: string;
                                        /** @enum {string} */
                                        normalization: "none" | "unit_l2";
                                        source_sha256: string;
                                    } | null;
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    subject: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    /** Format: uint64 */
                                    valid_from: number;
                                    /** Format: uint64 */
                                    valid_to?: number | null;
                                    value: {
                                        /** @constant */
                                        kind: "dense";
                                        values: number[];
                                    } | {
                                        /** Format: uint32 */
                                        dimensions: number;
                                        indices: number[];
                                        /** @constant */
                                        kind: "sparse";
                                        values: number[];
                                    } | {
                                        /** Format: uint32 */
                                        dimensions: number;
                                        /** @constant */
                                        kind: "multi_dense";
                                        vectors: number[][];
                                    };
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    vector_name?: string | null;
                                } | {
                                    /** @constant */
                                    mutation: "append_series_sample";
                                    /** Format: uint64 */
                                    observed_at: number;
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    series: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    value: {
                                        /** @constant */
                                        type: "integer";
                                        /** Format: int64 */
                                        value: number;
                                    } | {
                                        /** @constant */
                                        type: "unsigned";
                                        /** Format: uint64 */
                                        value: number;
                                    } | {
                                        /** @constant */
                                        type: "decimal";
                                        value: string;
                                    } | {
                                        /** @constant */
                                        type: "bool";
                                        value: boolean;
                                    } | {
                                        /** @constant */
                                        type: "string";
                                        value: string;
                                    };
                                } | {
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    field: string;
                                    /** @constant */
                                    mutation: "put_geo";
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    subject: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    /** Format: uint64 */
                                    valid_from: number;
                                    /** Format: uint64 */
                                    valid_to?: number | null;
                                    value: {
                                        /** @constant */
                                        kind: "point";
                                        point: {
                                            /** Format: double */
                                            latitude: number;
                                            /** Format: double */
                                            longitude: number;
                                        };
                                    } | {
                                        /** @constant */
                                        kind: "bounding_box";
                                        northeast: {
                                            /** Format: double */
                                            latitude: number;
                                            /** Format: double */
                                            longitude: number;
                                        };
                                        southwest: {
                                            /** Format: double */
                                            latitude: number;
                                            /** Format: double */
                                            longitude: number;
                                        };
                                    };
                                } | {
                                    /** Format: uint64 */
                                    length: number;
                                    media_type: string;
                                    /** @constant */
                                    mutation: "publish_object_reference";
                                    /** @default {} */
                                    properties: {
                                        [key: string]: {
                                            /** @constant */
                                            type: "null";
                                        } | {
                                            /** @constant */
                                            type: "bool";
                                            value: boolean;
                                        } | {
                                            /** @constant */
                                            type: "integer";
                                            /** Format: int64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "unsigned";
                                            /** Format: uint64 */
                                            value: number;
                                        } | {
                                            /** @constant */
                                            type: "decimal";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "string";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "digest";
                                            value: string;
                                        } | {
                                            /** @constant */
                                            type: "list";
                                            value: components["schemas"]["QueryValue"][];
                                        } | {
                                            /** @constant */
                                            type: "map";
                                            value: {
                                                [key: string]: components["schemas"]["QueryValue"];
                                            };
                                        };
                                    };
                                    receipt: {
                                        backend: string;
                                        etag?: string | null;
                                        key: string;
                                        version?: string | null;
                                    };
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    sha256: string;
                                    subject?: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    } | null;
                                })[];
                                operation_sha256: string;
                                /** Format: uint64 */
                                read_cursor: number;
                                transaction_id: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "vector-collection-ensure": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        /**
                         * @description A canonical public identifier component.
                         *
                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                         *     labels are separate data and may use arbitrary Unicode.
                         */
                        collection_id: string;
                        scope: string;
                        vectors: {
                            /** Format: uint32 */
                            dimensions: number;
                            embedding_model?: {
                                digest: string;
                                name: string;
                            } | null;
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            field: string;
                            /** @enum {string} */
                            kind: "dense" | "sparse" | "multi_dense";
                            /** @enum {string} */
                            memory_tier: "pinned" | "cached" | "cold";
                            /** @enum {string} */
                            metric: "cosine" | "dot" | "euclidean" | "manhattan";
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            name: string;
                        }[];
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                catalogue_revision: number;
                                collection: {
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    collection_id: string;
                                    configuration_sha256: string;
                                    /** Format: uint64 */
                                    created_at_unix_ms: number;
                                    /** Format: uint64 */
                                    generation: number;
                                    /** Format: uint64 */
                                    updated_at_unix_ms: number;
                                    vectors: {
                                        /** Format: uint32 */
                                        dimensions: number;
                                        embedding_model?: {
                                            digest: string;
                                            name: string;
                                        } | null;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        field: string;
                                        /** @enum {string} */
                                        kind: "dense" | "sparse" | "multi_dense";
                                        /** @enum {string} */
                                        memory_tier: "pinned" | "cached" | "cold";
                                        /** @enum {string} */
                                        metric: "cosine" | "dot" | "euclidean" | "manhattan";
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        name: string;
                                    }[];
                                };
                                idempotent_replay: boolean;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /** Format: uint64 */
                                catalogue_revision: number;
                                collection: {
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    collection_id: string;
                                    configuration_sha256: string;
                                    /** Format: uint64 */
                                    created_at_unix_ms: number;
                                    /** Format: uint64 */
                                    generation: number;
                                    /** Format: uint64 */
                                    updated_at_unix_ms: number;
                                    vectors: {
                                        /** Format: uint32 */
                                        dimensions: number;
                                        embedding_model?: {
                                            digest: string;
                                            name: string;
                                        } | null;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        field: string;
                                        /** @enum {string} */
                                        kind: "dense" | "sparse" | "multi_dense";
                                        /** @enum {string} */
                                        memory_tier: "pinned" | "cached" | "cold";
                                        /** @enum {string} */
                                        metric: "cosine" | "dot" | "euclidean" | "manhattan";
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        name: string;
                                    }[];
                                };
                                idempotent_replay: boolean;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "vector-collection-list": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        scope: string;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                collections: {
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    collection_id: string;
                                    configuration_sha256: string;
                                    /** Format: uint64 */
                                    created_at_unix_ms: number;
                                    /** Format: uint64 */
                                    generation: number;
                                    /** Format: uint64 */
                                    updated_at_unix_ms: number;
                                    vectors: {
                                        /** Format: uint32 */
                                        dimensions: number;
                                        embedding_model?: {
                                            digest: string;
                                            name: string;
                                        } | null;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        field: string;
                                        /** @enum {string} */
                                        kind: "dense" | "sparse" | "multi_dense";
                                        /** @enum {string} */
                                        memory_tier: "pinned" | "cached" | "cold";
                                        /** @enum {string} */
                                        metric: "cosine" | "dot" | "euclidean" | "manhattan";
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        name: string;
                                    }[];
                                }[];
                                /** Format: uint64 */
                                revision: number;
                                scope: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                collections: {
                                    /**
                                     * @description A canonical public identifier component.
                                     *
                                     *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                     *     labels are separate data and may use arbitrary Unicode.
                                     */
                                    collection_id: string;
                                    configuration_sha256: string;
                                    /** Format: uint64 */
                                    created_at_unix_ms: number;
                                    /** Format: uint64 */
                                    generation: number;
                                    /** Format: uint64 */
                                    updated_at_unix_ms: number;
                                    vectors: {
                                        /** Format: uint32 */
                                        dimensions: number;
                                        embedding_model?: {
                                            digest: string;
                                            name: string;
                                        } | null;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        field: string;
                                        /** @enum {string} */
                                        kind: "dense" | "sparse" | "multi_dense";
                                        /** @enum {string} */
                                        memory_tier: "pinned" | "cached" | "cold";
                                        /** @enum {string} */
                                        metric: "cosine" | "dot" | "euclidean" | "manhattan";
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        name: string;
                                    }[];
                                }[];
                                /** Format: uint64 */
                                revision: number;
                                scope: string;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
    "vector-search": {
        parameters: {
            query?: never;
            header: {
                "X-RRD-Session": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": {
                    context: {
                        /** Format: uint64 */
                        deadline_unix_ms?: number | null;
                        idempotency_key?: string | null;
                        operation_id: string;
                        request_id: string;
                    };
                    payload: {
                        /**
                         * @description A canonical public identifier component.
                         *
                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                         *     labels are separate data and may use arbitrary Unicode.
                         */
                        collection_id?: string | null;
                        /**
                         * @description A canonical public identifier component.
                         *
                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                         *     labels are separate data and may use arbitrary Unicode.
                         */
                        field?: string | null;
                        /** Format: uint64 */
                        max_scanned_changes: number;
                        /** @enum {string|null} */
                        metric?: "cosine" | "dot" | "euclidean" | "manhattan" | null;
                        query: {
                            /** @constant */
                            kind: "dense";
                            values: number[];
                        } | {
                            /** Format: uint32 */
                            dimensions: number;
                            indices: number[];
                            /** @constant */
                            kind: "sparse";
                            values: number[];
                        } | {
                            /** @enum {string} */
                            comparator: "max_sim";
                            /** Format: uint32 */
                            dimensions: number;
                            /** @constant */
                            kind: "multi_dense";
                            vectors: number[][];
                        };
                        scope: string;
                        /** Format: uint64 */
                        top_k: number;
                        /** Format: uint64 */
                        valid_at: number;
                        /**
                         * @description A canonical public identifier component.
                         *
                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                         *     labels are separate data and may use arbitrary Unicode.
                         */
                        vector_name?: string | null;
                    };
                    protocol: string;
                    /** Format: uint16 */
                    protocol_version: number;
                    /**
                     * @description A fully explicit hierarchical identity. No field is inferred from process
                     *     cwd, connection state, or a human label.
                     */
                    resource: {
                        segments: {
                            /**
                             * @description A canonical public identifier component.
                             *
                             *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                             *     labels are separate data and may use arbitrary Unicode.
                             */
                            id: string;
                            /** @enum {string} */
                            kind: "organization" | "estate" | "project" | "instance" | "node" | "shard" | "collection" | "table" | "record" | "transaction" | "snapshot" | "backup" | "operation";
                        }[];
                    };
                };
            };
        };
        responses: {
            /** @description Typed RRD response */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                access_path: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                collection_id?: string | null;
                                exact: boolean;
                                hits: {
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    /** Format: double */
                                    score: number;
                                    /** Format: uint64 */
                                    source_cursor: number;
                                    subject: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                }[];
                                /** Format: uint64 */
                                known_at_cursor: number;
                                plan_sha256: string;
                                read_manifest_sha256: string;
                                /** Format: uint64 */
                                scanned_changes: number;
                                scope: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                vector_name?: string | null;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
            /** @description Typed RRD error response */
            default: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": {
                        operation_id: string;
                        outcome: {
                            payload: {
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                access_path: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                collection_id?: string | null;
                                exact: boolean;
                                hits: {
                                    reference: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                    /** Format: double */
                                    score: number;
                                    /** Format: uint64 */
                                    source_cursor: number;
                                    subject: {
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        id: string;
                                        /**
                                         * @description A canonical public identifier component.
                                         *
                                         *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                         *     labels are separate data and may use arbitrary Unicode.
                                         */
                                        kind: string;
                                    };
                                }[];
                                /** Format: uint64 */
                                known_at_cursor: number;
                                plan_sha256: string;
                                read_manifest_sha256: string;
                                /** Format: uint64 */
                                scanned_changes: number;
                                scope: string;
                                /**
                                 * @description A canonical public identifier component.
                                 *
                                 *     IDs are lowercase ASCII and intentionally URL/path safe. Human-facing
                                 *     labels are separate data and may use arbitrary Unicode.
                                 */
                                vector_name?: string | null;
                            };
                            /** @constant */
                            status: "ok";
                        } | {
                            error: {
                                /** @enum {string} */
                                code: "invalid_argument" | "not_found" | "already_exists" | "conflict" | "failed_precondition" | "unauthenticated" | "permission_denied" | "resource_exhausted" | "deadline_exceeded" | "cancelled" | "unavailable" | "corruption" | "unsupported_version" | "internal";
                                details?: {
                                    [key: string]: string;
                                };
                                message: string;
                                retryable: boolean;
                            };
                            /** @constant */
                            status: "error";
                        };
                        protocol: string;
                        /** Format: uint16 */
                        protocol_version: number;
                        request_id: string;
                    };
                };
            };
        };
    };
}


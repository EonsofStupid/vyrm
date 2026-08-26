package io.rrflow.rrd;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;
import tools.jackson.databind.JsonNode;
import tools.jackson.databind.json.JsonMapper;

final class RrdClientTest {
    private static final JsonMapper JSON = JsonMapper.builder().build();

    @Test
    void publicNegotiationRetriesTransportLossAndValidatesIdentity() throws Exception {
        AtomicInteger attempts = new AtomicInteger();
        HttpServer server = server(exchange -> {
            if (attempts.incrementAndGet() == 1) {
                exchange.close();
                return;
            }
            assertEquals("GET", exchange.getRequestMethod());
            assertEquals("/v1/capabilities", exchange.getRequestURI().getPath());
            write(exchange, 200, ok("server-request", "server-operation", Map.of(
                    "protocol", "rrd",
                    "protocol_version", 1,
                    "implementation", "rrd-server",
                    "implementation_version", "0.1.0",
                    "deployment_mode", "local_server",
                    "instance", Map.of("kind", "instance", "id", "sdk-test"),
                    "capabilities", List.of())));
        });
        try {
            RrdClient client = new RrdClient(baseUrl(server), "sdk-test");
            assertEquals(1, client.capabilities().path("protocol_version").asInt());
            assertEquals(2, attempts.get());
        } finally {
            server.stop(0);
        }
    }

    @Test
    void sessionAndQueryConstructAuthenticatedV1Envelopes() throws Exception {
        List<String> authorization = new ArrayList<>();
        List<JsonNode> envelopes = new ArrayList<>();
        HttpServer server = server(exchange -> {
            JsonNode request = JSON.readTree(exchange.getRequestBody().readAllBytes());
            authorization.add(exchange.getRequestHeaders().getFirst("Authorization"));
            envelopes.add(request);
            JsonNode context = request.path("context");
            Object payload = exchange.getRequestURI().getPath().equals("/v1/sessions")
                    ? Map.of(
                            "session_id", "session-1",
                            "token", "token-1",
                            "issued_at_unix_ms", 100,
                            "idle_expires_at_unix_ms", 60100,
                            "absolute_expires_at_unix_ms", 300100,
                            "limits", Map.of(
                                    "idle_timeout_ms", 60000,
                                    "absolute_timeout_ms", 300000,
                                    "max_open_transactions", 2))
                    : Map.of(
                            "canonical_query", "FROM record:document",
                            "scope", "instance:sdk-test",
                            "read_manifest_sha256", "a".repeat(64),
                            "known_at_cursor", 2,
                            "schema_revision", 1,
                            "plan", Map.of(),
                            "execution", Map.of(),
                            "rows", List.of());
            write(exchange, 200, ok(
                    context.path("request_id").asString(),
                    context.path("operation_id").asString(),
                    payload));
        });
        try {
            RrdClient client = new RrdClient(baseUrl(server), "sdk-test");
            Session session = client.createSession(
                    "java-sdk",
                    "not-persisted",
                    Map.of("limits", Map.of(
                            "idle_timeout_ms", 60000,
                            "absolute_timeout_ms", 300000,
                            "max_open_transactions", 2)),
                    options("request-session", "operation-session", "session-key", null));
            JsonNode query = client.call(
                    OperationId.QUERY_EXECUTE,
                    Map.of(
                            "scope", "instance:sdk-test",
                            "query", "FROM record:document",
                            "parameters", Map.of(),
                            "budget", Map.of(
                                    "max_scanned_changes", 100,
                                    "max_rows", 10,
                                    "max_output_bytes", 4096,
                                    "max_batch_rows", 10)),
                    new RequestOptions(
                            "request-query", "operation-query", null, null,
                            Map.of(), null, session, null));
            assertEquals(2, query.path("known_at_cursor").asInt());
            assertEquals(List.of("ApiKey not-persisted", "Bearer token-1"), authorization);
            assertEquals(
                    "sdk-test",
                    envelopes.get(1).path("resource").path("segments").get(0).path("id").asString());
        } finally {
            server.stop(0);
        }
    }

    @Test
    void remoteCleartextDeadlinesTypedErrorsAndGenerationAreEnforced() throws Exception {
        assertThrows(
                RrdClientException.class,
                () -> new RrdClient("http://192.0.2.1:9477", "sdk-test"));
        HttpServer server = server(exchange -> {
            JsonNode request = JSON.readTree(exchange.getRequestBody().readAllBytes());
            JsonNode context = request.path("context");
            write(exchange, 403, Map.of(
                    "protocol", "rrd",
                    "protocol_version", 1,
                    "request_id", context.path("request_id").asString(),
                    "operation_id", context.path("operation_id").asString(),
                    "outcome", Map.of(
                            "status", "error",
                            "error", Map.of(
                                    "code", "permission_denied",
                                    "message", "policy denied",
                                    "retryable", false,
                                    "details", Map.of()))));
        });
        try {
            RrdClient client = new RrdClient(baseUrl(server), "sdk-test");
            Session session = new Session(
                    "java-sdk", new Session.SessionLease("session-1", "token-1"));
            RrdApiException error = assertThrows(
                    RrdApiException.class,
                    () -> client.call(
                            OperationId.AUDIT_READ,
                            Map.of("after_sequence", 0, "limit", 10),
                            new RequestOptions(
                                    "request-audit", "operation-audit", null, null,
                                    Map.of(), null, session, null)));
            assertEquals("permission_denied", error.code());
            RrdClientException expired = assertThrows(
                    RrdClientException.class,
                    () -> client.call(
                            OperationId.AUDIT_READ,
                            Map.of("after_sequence", 0, "limit", 10),
                            new RequestOptions(
                                    "request-expired", "operation-expired", null,
                                    Instant.ofEpochMilli(1), Map.of(), null, session, null)));
            assertTrue(expired.getMessage().contains("deadline has expired"));
            assertEquals(31, OperationId.values().length);
            assertTrue(OperationId.BACKUP_CREATE.mutation());
            assertEquals(
                    OperationId.Authentication.PUBLIC,
                    OperationId.CAPABILITIES_READ.authentication());
        } finally {
            server.stop(0);
        }
    }

    private static RequestOptions options(
            String requestId, String operationId, String idempotencyKey, Session session) {
        return new RequestOptions(
                requestId, operationId, idempotencyKey, null, Map.of(), null, session, null);
    }

    private static HttpServer server(ThrowingHandler handler) throws IOException {
        HttpServer server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/", exchange -> {
            try {
                handler.handle(exchange);
            } catch (Exception error) {
                exchange.close();
                throw new IOException(error);
            }
        });
        server.start();
        return server;
    }

    private static String baseUrl(HttpServer server) {
        return "http://127.0.0.1:" + server.getAddress().getPort();
    }

    private static Map<String, Object> ok(String requestId, String operationId, Object payload) {
        return Map.of(
                "protocol", "rrd",
                "protocol_version", 1,
                "request_id", requestId,
                "operation_id", operationId,
                "outcome", Map.of("status", "ok", "payload", payload));
    }

    private static void write(HttpExchange exchange, int status, Object body) throws IOException {
        byte[] encoded = JSON.writeValueAsBytes(body);
        exchange.getResponseHeaders().set("Content-Type", "application/json");
        exchange.sendResponseHeaders(status, encoded.length);
        exchange.getResponseBody().write(encoded);
        exchange.close();
    }

    @FunctionalInterface
    private interface ThrowingHandler {
        void handle(HttpExchange exchange) throws Exception;
    }
}

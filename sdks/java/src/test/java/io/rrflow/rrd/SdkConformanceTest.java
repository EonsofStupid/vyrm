package io.rrflow.rrd;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.time.Duration;
import java.time.Instant;
import java.util.HashSet;
import java.util.HexFormat;
import java.util.Map;
import java.util.Set;
import org.junit.jupiter.api.Assumptions;
import org.junit.jupiter.api.Test;
import tools.jackson.databind.JsonNode;
import tools.jackson.databind.json.JsonMapper;
import tools.jackson.databind.node.ObjectNode;

final class SdkConformanceTest {
    private static final JsonMapper JSON = JsonMapper.builder().build();
    private static final Set<String> REQUIRED_DOMAINS = Set.of(
            "auth",
            "backup",
            "cancellation",
            "crud",
            "estate",
            "live_feeds",
            "query",
            "retries",
            "sessions",
            "transactions",
            "typed_errors",
            "vectors",
            "versions");

    @Test
    void sdkConformanceQualifiesTheSharedCorpus() throws Exception {
        String manifestPath = System.getenv("RRD_SDK_CONFORMANCE_MANIFEST");
        Assumptions.assumeTrue(manifestPath != null, "SDK conformance harness is not active");
        JsonNode manifest = JSON.readTree(Files.readAllBytes(Path.of(manifestPath)));
        byte[] corpusBytes = Files.readAllBytes(Path.of(manifest.path("corpus_path").asString()));
        JsonNode corpus = JSON.readTree(corpusBytes);
        assertEquals(corpus.path("format_version").asInt(), manifest.path("format_version").asInt());
        assertEquals(
                manifest.path("corpus_sha256").asString(),
                HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(corpusBytes)));
        Set<String> domains = new HashSet<>();
        corpus.path("required_domains").forEach(domain -> domains.add(domain.asString()));
        assertEquals(REQUIRED_DOMAINS, domains);

        JsonNode identity = corpus.path("identity");
        JsonNode expected = corpus.path("expected");
        JsonNode sessionFixture = corpus.path("session");
        JsonNode transaction = corpus.path("transaction");
        JsonNode vector = corpus.path("vector");
        JsonNode changefeed = corpus.path("changefeed");
        JsonNode backupFixture = corpus.path("backup");
        String instance = identity.path("instance").asString();

        RrdClient retryClient = client(
                manifest.path("retry_base_urls").path("java").asString(),
                instance,
                expected.path("retry_attempts").asInt());
        assertEquals(
                corpus.path("protocol_version").asInt(),
                retryClient.capabilities().path("protocol_version").asInt());

        RrdClient incompatibleClient =
                client(manifest.path("incompatible_version_url").asString(), instance, 2);
        assertThrows(RrdClientException.class, incompatibleClient::capabilities);

        RrdClient api = client(manifest.path("base_url").asString(), instance, 2);
        assertEquals(corpus.path("protocol").asString(), api.capabilities().path("protocol").asString());
        assertEquals(
                expected.path("endpoint_count").asInt(),
                api.endpointCatalogue().path("endpoints").size());
        RrdApiException authError = assertThrows(
                RrdApiException.class,
                () -> api.createSession(
                        identity.path("principal").asString(),
                        "wrong-sdk-conformance-key",
                        sessionFixture.path("create"),
                        options("wrong-key", null, true, Map.of(), null)));
        assertEquals(expected.path("typed_error").asString(), authError.code());

        Session session = api.createSession(
                identity.path("principal").asString(),
                identity.path("api_key").asString(),
                sessionFixture.path("create"),
                options("session-create", null, true, Map.of(), null));
        JsonNode renewed = api.call(
                OperationId.SESSION_RENEW,
                sessionFixture.path("renew"),
                options(
                        "session-renew",
                        session,
                        true,
                        Map.of("session", session.lease().sessionId()),
                        null));
        session = new Session(
                session.principalId(),
                new Session.SessionLease(
                        renewed.path("session_id").asString(), renewed.path("token").asString()));

        api.call(
                OperationId.VECTOR_COLLECTION_ENSURE,
                vector.path("ensure"),
                options("vector-ensure", session, true, Map.of(), null));
        JsonNode previewLease = api.call(
                OperationId.TRANSACTION_BEGIN,
                transaction.path("preview_begin"),
                options("preview-begin", session, true, Map.of(), null));
        String previewId = previewLease.path("transaction_id").asString();
        JsonNode preview = api.call(
                OperationId.TRANSACTION_PREVIEW,
                transaction.path("preview"),
                options(
                        "transaction-preview",
                        session,
                        true,
                        Map.of("transaction", previewId),
                        null));
        assertEquals(previewId, preview.path("transaction_id").asString());
        JsonNode aborted = api.call(
                OperationId.TRANSACTION_ABORT,
                transaction.path("abort"),
                options(
                        "transaction-abort",
                        session,
                        true,
                        Map.of("transaction", previewId),
                        null));
        assertEquals("aborted", aborted.path("state").asString());

        JsonNode commitLease = api.call(
                OperationId.TRANSACTION_BEGIN,
                transaction.path("commit_begin"),
                options("commit-begin", session, true, Map.of(), null));
        JsonNode committed = api.call(
                OperationId.TRANSACTION_COMMIT,
                transaction.path("commit"),
                options(
                        "transaction-commit",
                        session,
                        true,
                        Map.of("transaction", commitLease.path("transaction_id").asString()),
                        Instant.now().plusMillis(
                                transaction.path("commit_deadline_timeout_ms").asLong())));
        assertEquals(
                transaction.path("commit").path("operation_sha256").asString(),
                committed.path("operation_sha256").asString());

        JsonNode query = api.call(
                OperationId.QUERY_EXECUTE,
                corpus.path("query"),
                options("query", session, false, Map.of(), null));
        assertTrue(containsIdentity(
                query.path("rows"), expected.path("query_identity").asString()));
        JsonNode vectors = api.call(
                OperationId.VECTOR_SEARCH,
                vector.path("search"),
                options("vector-search", session, false, Map.of(), null));
        assertTrue(containsVector(
                vectors.path("hits"), expected.path("vector_reference").asString()));

        JsonNode changes = api.call(
                OperationId.CHANGEFEED_READ,
                changefeed.path("read"),
                options("changefeed-read", session, false, Map.of(), null));
        ObjectNode followRead = (ObjectNode) changefeed.path("read").deepCopy();
        followRead.put("after_cursor", changes.path("head_cursor").asLong());
        ObjectNode followPayload = JSON.createObjectNode();
        followPayload.set("read", followRead);
        followPayload.put(
                "wait_timeout_ms", changefeed.path("follow_wait_timeout_ms").asLong());
        JsonNode followed = api.call(
                OperationId.CHANGEFEED_FOLLOW,
                followPayload,
                options("changefeed-follow", session, false, Map.of(), null));
        assertTrue(followed.path("timed_out").asBoolean());

        RrdClient cancellationClient = client(manifest.path("base_url").asString(), instance, 1);
        ObjectNode cancellationPayload = JSON.createObjectNode();
        cancellationPayload.set("read", followRead);
        cancellationPayload.put(
                "wait_timeout_ms", changefeed.path("cancellation_wait_timeout_ms").asLong());
        Session activeSession = session;
        assertThrows(
                RrdClientException.class,
                () -> cancellationClient.call(
                        OperationId.CHANGEFEED_FOLLOW,
                        cancellationPayload,
                        options(
                                "changefeed-cancel",
                                activeSession,
                                false,
                                Map.of(),
                                Instant.now().plusMillis(
                                        changefeed.path("cancel_after_ms").asLong()))));

        JsonNode backup = api.call(
                OperationId.BACKUP_CREATE,
                backupFixture.path("create"),
                options("backup-create", session, true, Map.of(), null));
        JsonNode backupSnapshot = backup.path("backup");
        String labelPrefix = backupFixture.path("create").path("label").asString() + "--";
        String label = backupSnapshot.path("label").asString();
        assertTrue(label.startsWith(labelPrefix));
        assertTrue(label.substring(labelPrefix.length()).matches("^[0-9a-f]{16}$"));
        JsonNode backups = api.call(
                OperationId.BACKUP_LIST,
                backupFixture.path("list"),
                options("backup-list", session, false, Map.of(), null));
        assertTrue(containsBackup(
                backups.path("backups"), backupSnapshot.path("backup_sha256").asString()));
        JsonNode estate = api.call(
                OperationId.ESTATE_READ,
                corpus.path("estate"),
                options(
                        "estate-read",
                        session,
                        false,
                        Map.of("estate", identity.path("estate").asString()),
                        null));
        assertEquals(expected.path("estate_revision").asInt(), estate.path("revision").asInt());
        JsonNode closed = api.call(
                OperationId.SESSION_CLOSE,
                sessionFixture.path("close"),
                options(
                        "session-close",
                        session,
                        true,
                        Map.of("session", session.lease().sessionId()),
                        null));
        assertEquals("closed", closed.path("state").asString());
        System.err.printf(
                "SDK conformance OK: language=java corpus_sha256=%s%n",
                manifest.path("corpus_sha256").asString());
    }

    private static RrdClient client(String baseUrl, String instance, int attempts) {
        return new RrdClient(
                baseUrl, instance, Duration.ofSeconds(10), attempts, 4 * 1024 * 1024, null);
    }

    private static RequestOptions options(
            String step,
            Session session,
            boolean mutation,
            Map<String, String> paths,
            Instant deadline) {
        return new RequestOptions(
                "java-" + step + "-request",
                "java-" + step + "-operation",
                mutation ? "java-" + step + "-key" : null,
                deadline,
                paths,
                null,
                session,
                null);
    }

    private static boolean containsIdentity(JsonNode rows, String expected) {
        for (JsonNode row : rows) {
            if (expected.equals(row.path("identity").asString())) {
                return true;
            }
        }
        return false;
    }

    private static boolean containsVector(JsonNode hits, String expected) {
        for (JsonNode hit : hits) {
            JsonNode reference = hit.path("reference");
            if (expected.equals(
                    reference.path("kind").asString() + ":" + reference.path("id").asString())) {
                return true;
            }
        }
        return false;
    }

    private static boolean containsBackup(JsonNode backups, String expected) {
        for (JsonNode backup : backups) {
            if (expected.equals(backup.path("backup_sha256").asString())) {
                return true;
            }
        }
        return false;
    }
}

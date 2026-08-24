package io.rrflow.rrd;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.InetAddress;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.time.Instant;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import tools.jackson.databind.JsonNode;
import tools.jackson.databind.json.JsonMapper;

public final class RrdClient {
    private static final Pattern CORRELATION = Pattern.compile("^[A-Za-z0-9._:-]+$");
    private static final Pattern CANONICAL = Pattern.compile("^[a-z0-9][a-z0-9._-]*$");
    private static final Pattern PATH_PARAMETER = Pattern.compile("\\{([a-z]+)}");
    private static final Set<String> RESOURCE_KINDS = Set.of(
            "organization", "estate", "project", "instance", "node", "shard",
            "collection", "table", "record", "transaction", "snapshot", "backup",
            "operation");
    private static final int DEFAULT_RESPONSE_LIMIT = 4 * 1024 * 1024;
    private static final int MAXIMUM_RESPONSE_LIMIT = 16 * 1024 * 1024;
    private static final JsonMapper JSON = JsonMapper.builder().build();

    private final URI baseUri;
    private final String instance;
    private final Duration requestTimeout;
    private final int maxAttempts;
    private final int maxResponseBytes;
    private final HttpClient http;

    public RrdClient(String baseUrl, String instance) {
        this(baseUrl, instance, Duration.ofSeconds(5), 2, DEFAULT_RESPONSE_LIMIT, null);
    }

    public RrdClient(
            String baseUrl,
            String instance,
            Duration requestTimeout,
            int maxAttempts,
            int maxResponseBytes,
            HttpClient http) {
        this.baseUri = loopbackUri(baseUrl);
        this.instance = canonical(instance, "instance");
        if (requestTimeout.isZero() || requestTimeout.isNegative()
                || requestTimeout.compareTo(Duration.ofMinutes(5)) > 0) {
            throw new RrdClientException("request timeout must be in (0, 5m]");
        }
        if (maxAttempts < 1 || maxAttempts > 8) {
            throw new RrdClientException("max attempts must be in 1..=8");
        }
        if (maxResponseBytes < 1 || maxResponseBytes > MAXIMUM_RESPONSE_LIMIT) {
            throw new RrdClientException("response limit must be in 1..=16777216 bytes");
        }
        this.requestTimeout = requestTimeout;
        this.maxAttempts = maxAttempts;
        this.maxResponseBytes = maxResponseBytes;
        this.http = http == null
                ? HttpClient.newBuilder().followRedirects(HttpClient.Redirect.NEVER).build()
                : http;
    }

    public JsonNode capabilities() {
        JsonNode result = call(OperationId.CAPABILITIES_READ, null, RequestOptions.empty());
        if (!result.path("protocol").asString().equals("rrd")
                || result.path("protocol_version").asInt() != 1
                || !result.path("instance").path("id").asString().equals(instance)) {
            throw new RrdClientException("RRD capability protocol or instance identity differs");
        }
        return result;
    }

    public JsonNode endpointCatalogue() {
        return call(OperationId.ENDPOINT_CATALOGUE, null, RequestOptions.empty());
    }

    public JsonNode openApi() {
        return call(OperationId.OPENAPI_READ, null, RequestOptions.empty());
    }

    public Session createSession(
            String principalId, String credential, Object payload, RequestOptions options) {
        String principal = canonical(principalId, "principal");
        if (credential == null || credential.isEmpty()) {
            throw new RrdClientException("API-key credential must not be empty");
        }
        JsonNode result = call(
                OperationId.SESSION_CREATE, payload, options.withApiKey(principal, credential));
        String sessionId = result.path("session_id").asString();
        String token = result.path("token").asString();
        if (sessionId.isEmpty() || token.isEmpty()) {
            throw new RrdClientException("invalid RRD session lease identity");
        }
        return new Session(principal, new Session.SessionLease(sessionId, token));
    }

    public JsonNode call(OperationId operation, Object payload, RequestOptions options) {
        if (operation == null || options == null) {
            throw new RrdClientException("operation and request options are required");
        }
        Map<String, Object> context = requestContext(operation, options);
        String path = resolvePath(operation.path(), options.pathParameters());
        URI target = baseUri.resolve(path.startsWith("/") ? path.substring(1) : path);
        byte[] body = null;
        if (!operation.method().equals("GET")) {
            List<ResourceSegment> resource = options.resource();
            if (resource == null) {
                resource = defaultResource(options.pathParameters());
            }
            Map<String, Object> envelope = new HashMap<>();
            envelope.put("protocol", "rrd");
            envelope.put("protocol_version", 1);
            envelope.put("context", context);
            envelope.put("resource", Map.of("segments", validateResource(resource)));
            envelope.put("payload", payload == null ? Map.of() : payload);
            try {
                body = JSON.writeValueAsBytes(envelope);
            } catch (Exception error) {
                throw new RrdClientException("encode RRD request", error);
            }
        }
        boolean retrySafe = operation.method().equals("GET")
                || !operation.mutation()
                || options.idempotencyKey() != null;
        int attempts = retrySafe ? maxAttempts : 1;
        IOException lastError = null;
        for (int attempt = 0; attempt < attempts; attempt++) {
            Duration timeout = remainingTimeout(options.deadline());
            HttpRequest.Builder builder = HttpRequest.newBuilder(target)
                    .timeout(timeout)
                    .header("Accept", "application/json");
            authenticate(builder, operation, options);
            if (body == null) {
                builder.method(operation.method(), HttpRequest.BodyPublishers.noBody());
            } else {
                builder.header("Content-Type", "application/json")
                        .method(operation.method(), HttpRequest.BodyPublishers.ofByteArray(body));
            }
            try {
                HttpResponse<InputStream> response =
                        http.send(builder.build(), HttpResponse.BodyHandlers.ofInputStream());
                byte[] encoded = readBounded(response);
                return decodeResponse(response.statusCode(), encoded, context);
            } catch (IOException error) {
                lastError = error;
            } catch (InterruptedException error) {
                Thread.currentThread().interrupt();
                throw new RrdClientException("RRD request interrupted", error);
            }
        }
        throw new RrdClientException("RRD transport failed", lastError);
    }

    private void authenticate(
            HttpRequest.Builder builder, OperationId operation, RequestOptions options) {
        switch (operation.authentication()) {
            case PUBLIC -> { }
            case API_KEY -> {
                RequestOptions.ApiKey apiKey = options.apiKey();
                if (apiKey == null || apiKey.credential() == null || apiKey.credential().isEmpty()) {
                    throw new RrdClientException(operation.wireName() + " requires API-key authentication");
                }
                builder.header("X-RRD-Principal", canonical(apiKey.principalId(), "principal"));
                builder.header("Authorization", "ApiKey " + apiKey.credential());
            }
            case SESSION_BEARER -> {
                Session session = options.session();
                if (session == null || session.lease().sessionId().isEmpty()
                        || session.lease().token().isEmpty()) {
                    throw new RrdClientException(operation.wireName() + " requires a valid session");
                }
                builder.header("X-RRD-Session", session.lease().sessionId());
                builder.header("Authorization", "Bearer " + session.lease().token());
            }
        }
    }

    private Map<String, Object> requestContext(OperationId operation, RequestOptions options) {
        if (operation.method().equals("GET")) {
            return null;
        }
        Map<String, Object> context = new HashMap<>();
        context.put("request_id", correlation(options.requestId(), "request ID"));
        context.put("operation_id", correlation(options.operationId(), "operation ID"));
        if (options.idempotencyKey() != null) {
            context.put("idempotency_key", correlation(options.idempotencyKey(), "idempotency key"));
        } else if (operation.mutation()) {
            throw new RrdClientException("mutating requests require an idempotency key");
        }
        if (options.deadline() != null) {
            long milliseconds = options.deadline().toEpochMilli();
            if (milliseconds <= 0) {
                throw new RrdClientException("deadline must be a positive Unix millisecond instant");
            }
            context.put("deadline_unix_ms", milliseconds);
        }
        return context;
    }

    private JsonNode decodeResponse(int status, byte[] encoded, Map<String, Object> context) {
        final JsonNode envelope;
        try {
            envelope = JSON.readTree(encoded);
        } catch (Exception error) {
            throw new RrdClientException("RRD response envelope is invalid", error);
        }
        requireExactFields(envelope, Set.of(
                "protocol", "protocol_version", "request_id", "operation_id", "outcome"));
        if (!envelope.path("protocol").asString().equals("rrd")
                || envelope.path("protocol_version").asInt() != 1) {
            throw new RrdClientException("RRD response protocol differs");
        }
        if (context != null && (!envelope.path("request_id").asString().equals(context.get("request_id"))
                || !envelope.path("operation_id").asString().equals(context.get("operation_id")))) {
            throw new RrdClientException("RRD response request/operation identity differs");
        }
        JsonNode outcome = envelope.path("outcome");
        String outcomeStatus = outcome.path("status").asString();
        boolean success = status >= 200 && status < 300;
        if (success != outcomeStatus.equals("ok")) {
            throw new RrdClientException("RRD HTTP status and typed outcome disagree");
        }
        if (outcomeStatus.equals("error")) {
            requireExactFields(outcome, Set.of("status", "error"));
            JsonNode error = outcome.path("error");
            requireExactFields(error, Set.of("code", "message", "retryable", "details"));
            if (error.path("code").asString().isEmpty()
                    || error.path("message").asString().isEmpty()
                    || !error.path("retryable").isBoolean()
                    || !error.path("details").isObject()) {
                throw new RrdClientException("RRD error outcome is incomplete");
            }
            Map<String, String> details = new HashMap<>();
            error.path("details").properties().forEach(entry ->
                    details.put(entry.getKey(), entry.getValue().asString()));
            throw new RrdApiException(
                    status, error.path("code").asString(), error.path("message").asString(),
                    error.path("retryable").asBoolean(), details);
        }
        if (!outcomeStatus.equals("ok")) {
            throw new RrdClientException("RRD response outcome status is invalid");
        }
        requireExactFields(outcome, Set.of("status", "payload"));
        JsonNode result = outcome.path("payload");
        if (!result.isObject()) {
            throw new RrdClientException("RRD success payload must be an object");
        }
        return result;
    }

    private static void requireExactFields(JsonNode node, Set<String> expected) {
        if (node == null || !node.isObject()) {
            throw new RrdClientException("RRD response object is invalid");
        }
        Set<String> actual = new HashSet<>();
        actual.addAll(node.propertyNames());
        if (!actual.equals(expected)) {
            throw new RrdClientException("RRD response object fields differ");
        }
    }

    private byte[] readBounded(HttpResponse<InputStream> response) throws IOException {
        long declared = response.headers().firstValueAsLong("Content-Length").orElse(-1);
        if (declared > maxResponseBytes) {
            throw new RrdClientException("RRD response exceeded the configured byte limit");
        }
        try (InputStream input = response.body(); ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[8192];
            int total = 0;
            int count;
            while ((count = input.read(buffer)) != -1) {
                total += count;
                if (total > maxResponseBytes) {
                    throw new RrdClientException("RRD response exceeded the configured byte limit");
                }
                output.write(buffer, 0, count);
            }
            return output.toByteArray();
        }
    }

    private Duration remainingTimeout(Instant deadline) {
        if (deadline == null) {
            return requestTimeout;
        }
        Duration remaining = Duration.between(Instant.now(), deadline);
        if (remaining.isZero() || remaining.isNegative()) {
            throw new RrdClientException("RRD request deadline has expired");
        }
        return remaining.compareTo(requestTimeout) < 0 ? remaining : requestTimeout;
    }

    private List<ResourceSegment> defaultResource(Map<String, String> parameters) {
        List<ResourceSegment> result = new ArrayList<>();
        String estate = parameters.get("estate");
        if (estate != null) {
            result.add(new ResourceSegment("estate", canonical(estate, "estate")));
        }
        result.add(new ResourceSegment("instance", instance));
        return result;
    }

    private static List<ResourceSegment> validateResource(List<ResourceSegment> segments) {
        if (segments.isEmpty() || segments.size() > 16) {
            throw new RrdClientException("resource paths must contain 1..=16 segments");
        }
        Set<String> kinds = new HashSet<>();
        List<ResourceSegment> result = new ArrayList<>();
        for (ResourceSegment segment : segments) {
            if (!RESOURCE_KINDS.contains(segment.kind()) || !kinds.add(segment.kind())) {
                throw new RrdClientException("resource kind is unknown or repeated");
            }
            result.add(new ResourceSegment(segment.kind(), canonical(segment.id(), "resource")));
        }
        return List.copyOf(result);
    }

    private static String resolvePath(String template, Map<String, String> parameters) {
        Matcher matcher = PATH_PARAMETER.matcher(template);
        StringBuilder result = new StringBuilder();
        while (matcher.find()) {
            String value = parameters.get(matcher.group(1));
            if (value == null || value.isEmpty()) {
                throw new RrdClientException("missing path parameter " + matcher.group(1));
            }
            String encoded = URLEncoder.encode(
                    correlation(value, matcher.group(1) + " path parameter"), StandardCharsets.UTF_8)
                    .replace("+", "%20");
            matcher.appendReplacement(result, Matcher.quoteReplacement(encoded));
        }
        matcher.appendTail(result);
        return result.toString();
    }

    private static String correlation(String value, String label) {
        if (value == null || value.length() > 128 || !CORRELATION.matcher(value).matches()) {
            throw new RrdClientException(label + " is not a canonical RRD correlation ID");
        }
        return value;
    }

    private static String canonical(String value, String label) {
        if (value == null || value.length() > 128 || !CANONICAL.matcher(value).matches()) {
            throw new RrdClientException(label + " is not a canonical RRD identifier");
        }
        return value;
    }

    private static URI loopbackUri(String value) {
        final URI uri;
        try {
            uri = URI.create(value);
            String host = uri.getHost();
            boolean loopback = "localhost".equals(host);
            if (host != null && !loopback && host.matches("^[0-9a-fA-F:.]+$")) {
                InetAddress address = InetAddress.getByName(host);
                loopback = address.isLoopbackAddress();
            }
            if (!"http".equals(uri.getScheme()) || !loopback || uri.getUserInfo() != null
                    || uri.getQuery() != null || uri.getFragment() != null) {
                throw new IllegalArgumentException();
            }
            return URI.create(value.endsWith("/") ? value : value + "/");
        } catch (Exception error) {
            throw new RrdClientException(
                    "RRD Java client permits only credential-free loopback HTTP before TLS qualification",
                    error);
        }
    }
}

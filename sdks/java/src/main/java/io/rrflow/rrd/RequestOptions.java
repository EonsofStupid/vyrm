package io.rrflow.rrd;

import java.time.Instant;
import java.util.List;
import java.util.Map;

public record RequestOptions(
        String requestId,
        String operationId,
        String idempotencyKey,
        Instant deadline,
        Map<String, String> pathParameters,
        List<ResourceSegment> resource,
        Session session,
        ApiKey apiKey) {

    public record ApiKey(String principalId, String credential) {}

    public RequestOptions {
        pathParameters = pathParameters == null ? Map.of() : Map.copyOf(pathParameters);
        resource = resource == null ? null : List.copyOf(resource);
    }

    public static RequestOptions empty() {
        return new RequestOptions(null, null, null, null, Map.of(), null, null, null);
    }

    public RequestOptions withApiKey(String principalId, String credential) {
        return new RequestOptions(
                requestId, operationId, idempotencyKey, deadline, pathParameters, resource,
                null, new ApiKey(principalId, credential));
    }
}

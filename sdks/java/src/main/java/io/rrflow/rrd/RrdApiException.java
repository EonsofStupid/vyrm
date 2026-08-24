package io.rrflow.rrd;

import java.util.Map;

public final class RrdApiException extends RrdClientException {
    private static final long serialVersionUID = 1L;

    private final int status;
    private final String code;
    private final boolean retryable;
    private final transient Map<String, String> details;

    public RrdApiException(
            int status,
            String code,
            String message,
            boolean retryable,
            Map<String, String> details) {
        super("RRD API " + status + ": " + code + ": " + message);
        this.status = status;
        this.code = code;
        this.retryable = retryable;
        this.details = Map.copyOf(details);
    }

    public int status() { return status; }
    public String code() { return code; }
    public boolean retryable() { return retryable; }
    public Map<String, String> details() { return details; }
}

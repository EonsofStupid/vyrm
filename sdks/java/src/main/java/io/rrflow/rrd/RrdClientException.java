package io.rrflow.rrd;

public class RrdClientException extends RuntimeException {
    private static final long serialVersionUID = 1L;

    public RrdClientException(String message) {
        super(message);
    }

    public RrdClientException(String message, Throwable cause) {
        super(message, cause);
    }
}

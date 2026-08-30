package io.rrflow.rrd;

public record Session(String principalId, SessionLease lease) {
    public record SessionLease(String sessionId, String token) {}
}

using System;
using System.Collections.Generic;

namespace Rrflow.Rrd;

public sealed record ResourceSegment(string Kind, string Id);

public sealed record SessionLease(string SessionId, string Token);

public sealed record Session(string PrincipalId, SessionLease Lease);

public sealed record ApiKeyCredentials(string PrincipalId, string Credential);

public sealed record RequestOptions
{
    public string? RequestId { get; init; }
    public string? OperationId { get; init; }
    public string? IdempotencyKey { get; init; }
    public DateTimeOffset? Deadline { get; init; }
    public IReadOnlyDictionary<string, string> PathParameters { get; init; }
        = new Dictionary<string, string>();
    public IReadOnlyList<ResourceSegment>? Resource { get; init; }
    public Session? Session { get; init; }
    public ApiKeyCredentials? ApiKey { get; init; }
}

public sealed record RrdClientOptions
{
    public required Uri BaseUri { get; init; }
    public required string Instance { get; init; }
    public TimeSpan RequestTimeout { get; init; } = TimeSpan.FromSeconds(5);
    public int MaxAttempts { get; init; } = 2;
    public int MaxResponseBytes { get; init; } = 4 * 1024 * 1024;
}

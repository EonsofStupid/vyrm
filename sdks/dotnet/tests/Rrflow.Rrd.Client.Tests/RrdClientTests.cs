using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Rrflow.Rrd;
using Xunit;

namespace Rrflow.Rrd.Client.Tests;

public sealed class RrdClientTests
{
    [Fact]
    public async Task PublicNegotiationRetriesTransportLossAndValidatesIdentity()
    {
        int attempts = 0;
        StubHandler handler = new(async (request, _, _) =>
        {
            attempts++;
            Assert.Equal(HttpMethod.Get, request.Method);
            Assert.Equal("/v1/capabilities", request.RequestUri!.AbsolutePath);
            if (attempts == 1)
            {
                throw new HttpRequestException("connection reset");
            }
            return await Task.FromResult(Response(HttpStatusCode.OK, Ok(
                "server-request",
                "server-operation",
                new Dictionary<string, object?>
                {
                    ["protocol"] = "rrd",
                    ["protocol_version"] = 1,
                    ["implementation"] = "rrd-server",
                    ["implementation_version"] = "0.1.0",
                    ["deployment_mode"] = "local_daemon",
                    ["instance"] = new Dictionary<string, object?>
                    {
                        ["kind"] = "instance",
                        ["id"] = "sdk-test",
                    },
                    ["capabilities"] = Array.Empty<object>(),
                })));
        });
        using RrdClient client = Client(handler);
        JsonElement capabilities = await client.CapabilitiesAsync(TestContext.Current.CancellationToken);
        Assert.Equal(1, capabilities.GetProperty("protocol_version").GetInt32());
        Assert.Equal(2, attempts);
    }

    [Fact]
    public async Task SessionAndQueryConstructAuthenticatedV1Envelopes()
    {
        List<string?> authorization = new();
        List<JsonElement> envelopes = new();
        StubHandler handler = new(async (request, _, cancellationToken) =>
        {
            string encoded = await request.Content!.ReadAsStringAsync(cancellationToken);
            using JsonDocument document = JsonDocument.Parse(encoded);
            JsonElement envelope = document.RootElement.Clone();
            envelopes.Add(envelope);
            authorization.Add(request.Headers.Authorization?.ToString()
                ?? Header(request, "Authorization"));
            JsonElement context = envelope.GetProperty("context");
            object payload = request.RequestUri!.AbsolutePath == "/v1/sessions"
                ? new Dictionary<string, object?>
                {
                    ["session_id"] = "session-1",
                    ["token"] = "token-1",
                    ["issued_at_unix_ms"] = 100,
                    ["idle_expires_at_unix_ms"] = 60_100,
                    ["absolute_expires_at_unix_ms"] = 300_100,
                    ["limits"] = new Dictionary<string, object?>
                    {
                        ["idle_timeout_ms"] = 60_000,
                        ["absolute_timeout_ms"] = 300_000,
                        ["max_open_transactions"] = 2,
                    },
                }
                : new Dictionary<string, object?>
                {
                    ["canonical_query"] = "FROM record:document",
                    ["scope"] = "instance:sdk-test",
                    ["read_manifest_sha256"] = new string('a', 64),
                    ["known_at_cursor"] = 2,
                    ["schema_revision"] = 1,
                    ["plan"] = new Dictionary<string, object?>(),
                    ["execution"] = new Dictionary<string, object?>(),
                    ["rows"] = Array.Empty<object>(),
                };
            return Response(HttpStatusCode.OK, Ok(
                context.GetProperty("request_id").GetString()!,
                context.GetProperty("operation_id").GetString()!,
                payload));
        });
        using RrdClient client = Client(handler);
        Session session = await client.CreateSessionAsync(
            "dotnet-sdk",
            "not-persisted",
            new Dictionary<string, object?>
            {
                ["limits"] = new Dictionary<string, object?>
                {
                    ["idle_timeout_ms"] = 60_000,
                    ["absolute_timeout_ms"] = 300_000,
                    ["max_open_transactions"] = 2,
                },
            },
            new RequestOptions
            {
                RequestId = "request-session",
                OperationId = "operation-session",
                IdempotencyKey = "session-key",
            },
            TestContext.Current.CancellationToken);
        JsonElement query = await client.CallAsync(
            OperationId.QueryExecute,
            new Dictionary<string, object?>
            {
                ["scope"] = "instance:sdk-test",
                ["query"] = "FROM record:document",
                ["parameters"] = new Dictionary<string, object?>(),
                ["budget"] = new Dictionary<string, object?>
                {
                    ["max_scanned_changes"] = 100,
                    ["max_rows"] = 10,
                    ["max_output_bytes"] = 4_096,
                    ["max_batch_rows"] = 10,
                },
            },
            new RequestOptions
            {
                RequestId = "request-query",
                OperationId = "operation-query",
                Session = session,
            },
            TestContext.Current.CancellationToken);
        Assert.Equal(2, query.GetProperty("known_at_cursor").GetInt32());
        Assert.Equal(new[] { "ApiKey not-persisted", "Bearer token-1" }, authorization);
        Assert.Equal(
            "sdk-test",
            envelopes[1].GetProperty("resource").GetProperty("segments")[0]
                .GetProperty("id").GetString());
    }

    [Fact]
    public async Task SecurityDeadlinesTypedErrorsAndGenerationAreEnforced()
    {
        Assert.Throws<RrdClientException>(() => new RrdClient(new RrdClientOptions
        {
            BaseUri = new Uri("http://192.0.2.1:9477"),
            Instance = "sdk-test",
        }));
        StubHandler handler = new(async (request, _, cancellationToken) =>
        {
            string encoded = await request.Content!.ReadAsStringAsync(cancellationToken);
            using JsonDocument document = JsonDocument.Parse(encoded);
            JsonElement context = document.RootElement.GetProperty("context");
            return Response(HttpStatusCode.Forbidden, new Dictionary<string, object?>
            {
                ["protocol"] = "rrd",
                ["protocol_version"] = 1,
                ["request_id"] = context.GetProperty("request_id").GetString(),
                ["operation_id"] = context.GetProperty("operation_id").GetString(),
                ["outcome"] = new Dictionary<string, object?>
                {
                    ["status"] = "error",
                    ["error"] = new Dictionary<string, object?>
                    {
                        ["code"] = "permission_denied",
                        ["message"] = "policy denied",
                        ["retryable"] = false,
                        ["details"] = new Dictionary<string, string>(),
                    },
                },
            });
        });
        using RrdClient client = Client(handler);
        Session session = new("dotnet-sdk", new SessionLease("session-1", "token-1"));
        RrdApiException denied = await Assert.ThrowsAsync<RrdApiException>(() => client.CallAsync(
            OperationId.AuditRead,
            new Dictionary<string, object?> { ["after_sequence"] = 0, ["limit"] = 10 },
            new RequestOptions
            {
                RequestId = "request-audit",
                OperationId = "operation-audit",
                Session = session,
            },
            TestContext.Current.CancellationToken));
        Assert.Equal("permission_denied", denied.Code);
        RrdClientException expired = await Assert.ThrowsAsync<RrdClientException>(() => client.CallAsync(
            OperationId.AuditRead,
            new Dictionary<string, object?> { ["after_sequence"] = 0, ["limit"] = 10 },
            new RequestOptions
            {
                RequestId = "request-expired",
                OperationId = "operation-expired",
                Deadline = DateTimeOffset.FromUnixTimeMilliseconds(1),
                Session = session,
            },
            TestContext.Current.CancellationToken));
        Assert.Contains("deadline has expired", expired.Message, StringComparison.Ordinal);
        Assert.Equal(33, EndpointCatalog.Count);
        Assert.True(EndpointCatalog.Get(OperationId.BackupCreate).Mutation);
        Assert.True(EndpointCatalog.Get(OperationId.TransactionPreview).Mutation);
        Assert.Equal(
            Authentication.Public,
            EndpointCatalog.Get(OperationId.CapabilitiesRead).Authentication);
    }

    private static RrdClient Client(HttpMessageHandler handler) => new(
        new RrdClientOptions
        {
            BaseUri = new Uri("http://127.0.0.1:9477"),
            Instance = "sdk-test",
        },
        handler);

    private static Dictionary<string, object?> Ok(
        string requestId,
        string operationId,
        object payload) => new()
        {
            ["protocol"] = "rrd",
            ["protocol_version"] = 1,
            ["request_id"] = requestId,
            ["operation_id"] = operationId,
            ["outcome"] = new Dictionary<string, object?>
            {
                ["status"] = "ok",
                ["payload"] = payload,
            },
        };

    private static HttpResponseMessage Response(HttpStatusCode status, object body) => new(status)
    {
        Content = new StringContent(JsonSerializer.Serialize(body), Encoding.UTF8, "application/json"),
    };

    private static string? Header(HttpRequestMessage request, string name) =>
        request.Headers.TryGetValues(name, out IEnumerable<string>? values)
            ? string.Join(",", values)
            : null;

    private sealed class StubHandler(
        Func<HttpRequestMessage, int, CancellationToken, Task<HttpResponseMessage>> handler)
        : HttpMessageHandler
    {
        private int _attempt;

        protected override Task<HttpResponseMessage> SendAsync(
            HttpRequestMessage request,
            CancellationToken cancellationToken) =>
            handler(request, Interlocked.Increment(ref _attempt), cancellationToken);
    }
}

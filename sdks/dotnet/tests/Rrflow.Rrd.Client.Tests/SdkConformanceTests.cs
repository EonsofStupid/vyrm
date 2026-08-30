using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using Rrflow.Rrd;
using Xunit;

namespace Rrflow.Rrd.Client.Tests;

public sealed class SdkConformanceTests
{
    private static readonly HashSet<string> RequiredDomains = new(StringComparer.Ordinal)
    {
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
        "versions",
    };

    [Fact]
    public async Task SdkConformanceQualifiesTheSharedCorpus()
    {
        string? manifestPath = Environment.GetEnvironmentVariable("RRD_SDK_CONFORMANCE_MANIFEST");
        if (manifestPath is null)
        {
            return;
        }
        using JsonDocument manifestDocument = JsonDocument.Parse(await File.ReadAllBytesAsync(
            manifestPath,
            TestContext.Current.CancellationToken));
        JsonElement manifest = manifestDocument.RootElement;
        byte[] corpusBytes = await File.ReadAllBytesAsync(
            manifest.GetProperty("corpus_path").GetString()!,
            TestContext.Current.CancellationToken);
        using JsonDocument corpusDocument = JsonDocument.Parse(corpusBytes);
        JsonElement corpus = corpusDocument.RootElement;
        Assert.Equal(
            corpus.GetProperty("format_version").GetInt32(),
            manifest.GetProperty("format_version").GetInt32());
        Assert.Equal(
            manifest.GetProperty("corpus_sha256").GetString(),
            Convert.ToHexString(SHA256.HashData(corpusBytes)).ToLowerInvariant());
        HashSet<string> domains = corpus.GetProperty("required_domains")
            .EnumerateArray()
            .Select(domain => domain.GetString()!)
            .ToHashSet(StringComparer.Ordinal);
        Assert.True(RequiredDomains.SetEquals(domains));

        JsonElement identity = corpus.GetProperty("identity");
        JsonElement expected = corpus.GetProperty("expected");
        JsonElement sessionFixture = corpus.GetProperty("session");
        JsonElement transaction = corpus.GetProperty("transaction");
        JsonElement vector = corpus.GetProperty("vector");
        JsonElement changefeed = corpus.GetProperty("changefeed");
        JsonElement backupFixture = corpus.GetProperty("backup");
        string instance = identity.GetProperty("instance").GetString()!;

        using RrdClient retryClient = Client(
            manifest.GetProperty("retry_base_urls").GetProperty("dotnet").GetString()!,
            instance,
            expected.GetProperty("retry_attempts").GetInt32());
        JsonElement retryCapabilities = await retryClient.CapabilitiesAsync(
            TestContext.Current.CancellationToken);
        Assert.Equal(
            corpus.GetProperty("protocol_version").GetInt32(),
            retryCapabilities.GetProperty("protocol_version").GetInt32());

        using RrdClient incompatibleClient = Client(
            manifest.GetProperty("incompatible_version_url").GetString()!, instance, 2);
        await Assert.ThrowsAsync<RrdClientException>(() => incompatibleClient.CapabilitiesAsync(
            TestContext.Current.CancellationToken));

        using RrdClient api = Client(manifest.GetProperty("base_url").GetString()!, instance, 2);
        Assert.Equal(
            corpus.GetProperty("protocol").GetString(),
            (await api.CapabilitiesAsync(TestContext.Current.CancellationToken))
                .GetProperty("protocol").GetString());
        Assert.Equal(
            expected.GetProperty("endpoint_count").GetInt32(),
            (await api.EndpointCatalogueAsync(TestContext.Current.CancellationToken))
                .GetProperty("endpoints").GetArrayLength());
        RrdApiException authError = await Assert.ThrowsAsync<RrdApiException>(() =>
            api.CreateSessionAsync(
                identity.GetProperty("principal").GetString()!,
                "wrong-sdk-conformance-key",
                sessionFixture.GetProperty("create"),
                Options("wrong-key", mutation: true),
                TestContext.Current.CancellationToken));
        Assert.Equal(expected.GetProperty("typed_error").GetString(), authError.Code);

        Session session = await api.CreateSessionAsync(
            identity.GetProperty("principal").GetString()!,
            identity.GetProperty("api_key").GetString()!,
            sessionFixture.GetProperty("create"),
            Options("session-create", mutation: true),
            TestContext.Current.CancellationToken);
        JsonElement renewed = await api.CallAsync(
            OperationId.SessionRenew,
            sessionFixture.GetProperty("renew"),
            Options(
                "session-renew",
                session,
                mutation: true,
                paths: new Dictionary<string, string> { ["session"] = session.Lease.SessionId }),
            TestContext.Current.CancellationToken);
        session = new Session(
            session.PrincipalId,
            new SessionLease(
                renewed.GetProperty("session_id").GetString()!,
                renewed.GetProperty("token").GetString()!));

        await api.CallAsync(
            OperationId.VectorCollectionEnsure,
            vector.GetProperty("ensure"),
            Options("vector-ensure", session, mutation: true),
            TestContext.Current.CancellationToken);
        JsonElement previewLease = await api.CallAsync(
            OperationId.TransactionBegin,
            transaction.GetProperty("preview_begin"),
            Options("preview-begin", session, mutation: true),
            TestContext.Current.CancellationToken);
        string previewId = previewLease.GetProperty("transaction_id").GetString()!;
        JsonElement preview = await api.CallAsync(
            OperationId.TransactionPreview,
            transaction.GetProperty("preview"),
            Options(
                "transaction-preview",
                session,
                mutation: true,
                paths: new Dictionary<string, string> { ["transaction"] = previewId }),
            TestContext.Current.CancellationToken);
        Assert.Equal(previewId, preview.GetProperty("transaction_id").GetString());
        JsonElement aborted = await api.CallAsync(
            OperationId.TransactionAbort,
            transaction.GetProperty("abort"),
            Options(
                "transaction-abort",
                session,
                mutation: true,
                paths: new Dictionary<string, string> { ["transaction"] = previewId }),
            TestContext.Current.CancellationToken);
        Assert.Equal("aborted", aborted.GetProperty("state").GetString());

        JsonElement commitLease = await api.CallAsync(
            OperationId.TransactionBegin,
            transaction.GetProperty("commit_begin"),
            Options("commit-begin", session, mutation: true),
            TestContext.Current.CancellationToken);
        JsonElement committed = await api.CallAsync(
            OperationId.TransactionCommit,
            transaction.GetProperty("commit"),
            Options(
                "transaction-commit",
                session,
                mutation: true,
                paths: new Dictionary<string, string>
                {
                    ["transaction"] = commitLease.GetProperty("transaction_id").GetString()!,
                },
                deadline: DateTimeOffset.UtcNow.AddMilliseconds(
                    transaction.GetProperty("commit_deadline_timeout_ms").GetInt64())),
            TestContext.Current.CancellationToken);
        Assert.Equal(
            transaction.GetProperty("commit").GetProperty("operation_sha256").GetString(),
            committed.GetProperty("operation_sha256").GetString());

        JsonElement query = await api.CallAsync(
            OperationId.QueryExecute,
            corpus.GetProperty("query"),
            Options("query", session),
            TestContext.Current.CancellationToken);
        Assert.Contains(
            query.GetProperty("rows").EnumerateArray(),
            row => row.GetProperty("identity").GetString()
                == expected.GetProperty("query_identity").GetString());
        JsonElement vectors = await api.CallAsync(
            OperationId.VectorSearch,
            vector.GetProperty("search"),
            Options("vector-search", session),
            TestContext.Current.CancellationToken);
        Assert.Contains(
            vectors.GetProperty("hits").EnumerateArray(),
            hit =>
            {
                JsonElement reference = hit.GetProperty("reference");
                return reference.GetProperty("kind").GetString() + ":"
                    + reference.GetProperty("id").GetString()
                    == expected.GetProperty("vector_reference").GetString();
            });

        JsonElement changes = await api.CallAsync(
            OperationId.ChangefeedRead,
            changefeed.GetProperty("read"),
            Options("changefeed-read", session),
            TestContext.Current.CancellationToken);
        Dictionary<string, object?> followRead = JsonSerializer.Deserialize<Dictionary<string, object?>>(
            changefeed.GetProperty("read").GetRawText())!;
        followRead["after_cursor"] = changes.GetProperty("head_cursor").GetInt64();
        Dictionary<string, object?> followPayload = new()
        {
            ["read"] = followRead,
            ["wait_timeout_ms"] = changefeed.GetProperty("follow_wait_timeout_ms").GetInt64(),
        };
        JsonElement followed = await api.CallAsync(
            OperationId.ChangefeedFollow,
            followPayload,
            Options("changefeed-follow", session),
            TestContext.Current.CancellationToken);
        Assert.True(followed.GetProperty("timed_out").GetBoolean());

        using RrdClient cancellationClient = Client(
            manifest.GetProperty("base_url").GetString()!, instance, 1);
        using CancellationTokenSource cancellation = new();
        cancellation.CancelAfter(changefeed.GetProperty("cancel_after_ms").GetInt32());
        bool callerCancellationObserved = false;
        try
        {
            await cancellationClient.CallAsync(
                OperationId.ChangefeedFollow,
                new Dictionary<string, object?>
                {
                    ["read"] = followRead,
                    ["wait_timeout_ms"] = changefeed.GetProperty("cancellation_wait_timeout_ms")
                        .GetInt64(),
                },
                Options("changefeed-cancel", session),
                cancellation.Token);
        }
        catch (OperationCanceledException)
        {
            callerCancellationObserved = true;
        }
        Assert.True(callerCancellationObserved);

        JsonElement backup = await api.CallAsync(
            OperationId.BackupCreate,
            backupFixture.GetProperty("create"),
            Options("backup-create", session, mutation: true),
            TestContext.Current.CancellationToken);
        JsonElement backupSnapshot = backup.GetProperty("backup");
        string labelPrefix = backupFixture.GetProperty("create").GetProperty("label").GetString()
            + "--";
        string label = backupSnapshot.GetProperty("label").GetString()!;
        Assert.StartsWith(labelPrefix, label, StringComparison.Ordinal);
        Assert.Matches(
            new Regex("^[0-9a-f]{16}$", RegexOptions.CultureInvariant),
            label[labelPrefix.Length..]);
        JsonElement backups = await api.CallAsync(
            OperationId.BackupList,
            backupFixture.GetProperty("list"),
            Options("backup-list", session),
            TestContext.Current.CancellationToken);
        Assert.Contains(
            backups.GetProperty("backups").EnumerateArray(),
            entry => entry.GetProperty("backup_sha256").GetString()
                == backupSnapshot.GetProperty("backup_sha256").GetString());
        JsonElement estate = await api.CallAsync(
            OperationId.EstateRead,
            corpus.GetProperty("estate"),
            Options(
                "estate-read",
                session,
                paths: new Dictionary<string, string>
                {
                    ["estate"] = identity.GetProperty("estate").GetString()!,
                }),
            TestContext.Current.CancellationToken);
        Assert.Equal(
            expected.GetProperty("estate_revision").GetInt32(),
            estate.GetProperty("revision").GetInt32());
        JsonElement closed = await api.CallAsync(
            OperationId.SessionClose,
            sessionFixture.GetProperty("close"),
            Options(
                "session-close",
                session,
                mutation: true,
                paths: new Dictionary<string, string> { ["session"] = session.Lease.SessionId }),
            TestContext.Current.CancellationToken);
        Assert.Equal("closed", closed.GetProperty("state").GetString());
        Console.Error.WriteLine(
            $"SDK conformance OK: language=dotnet corpus_sha256="
            + manifest.GetProperty("corpus_sha256").GetString());
    }

    private static RrdClient Client(string baseUrl, string instance, int attempts) => new(
        new RrdClientOptions
        {
            BaseUri = new Uri(baseUrl),
            Instance = instance,
            RequestTimeout = TimeSpan.FromSeconds(10),
            MaxAttempts = attempts,
        });

    private static RequestOptions Options(
        string step,
        Session? session = null,
        bool mutation = false,
        IReadOnlyDictionary<string, string>? paths = null,
        DateTimeOffset? deadline = null) => new()
        {
            RequestId = $"dotnet-{step}-request",
            OperationId = $"dotnet-{step}-operation",
            IdempotencyKey = mutation ? $"dotnet-{step}-key" : null,
            Deadline = deadline,
            PathParameters = paths ?? new Dictionary<string, string>(),
            Session = session,
        };
}

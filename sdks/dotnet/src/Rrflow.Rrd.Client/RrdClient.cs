using System;
using System.Buffers;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;

namespace Rrflow.Rrd;

public sealed partial class RrdClient : IDisposable
{
    private const int MaximumResponseBytes = 16 * 1024 * 1024;
    private static readonly HashSet<string> ResourceKinds = new(StringComparer.Ordinal)
    {
        "organization", "estate", "project", "instance", "node", "shard", "collection",
        "table", "record", "transaction", "snapshot", "backup", "operation",
    };
    private static readonly JsonSerializerOptions SerializerOptions = new(JsonSerializerDefaults.Web);

    private readonly Uri _baseUri;
    private readonly string _instance;
    private readonly TimeSpan _requestTimeout;
    private readonly int _maxAttempts;
    private readonly int _maxResponseBytes;
    private readonly HttpClient _http;
    private readonly bool _ownsHttp;

    public RrdClient(RrdClientOptions options, HttpMessageHandler? handler = null)
    {
        ArgumentNullException.ThrowIfNull(options);
        _baseUri = LoopbackUri(options.BaseUri);
        _instance = Canonical(options.Instance, "instance");
        if (options.RequestTimeout <= TimeSpan.Zero || options.RequestTimeout > TimeSpan.FromMinutes(5))
        {
            throw new RrdClientException("request timeout must be in (0, 5m]");
        }
        if (options.MaxAttempts is < 1 or > 8)
        {
            throw new RrdClientException("max attempts must be in 1..=8");
        }
        if (options.MaxResponseBytes is < 1 or > MaximumResponseBytes)
        {
            throw new RrdClientException("response limit must be in 1..=16777216 bytes");
        }
        _requestTimeout = options.RequestTimeout;
        _maxAttempts = options.MaxAttempts;
        _maxResponseBytes = options.MaxResponseBytes;
        _ownsHttp = true;
        _http = handler is null
            ? new HttpClient(new HttpClientHandler { AllowAutoRedirect = false })
            : new HttpClient(handler, disposeHandler: false);
    }

    public async Task<JsonElement> CapabilitiesAsync(CancellationToken cancellationToken = default)
    {
        JsonElement result = await CallAsync(
            OperationId.CapabilitiesRead, null, new RequestOptions(), cancellationToken)
            .ConfigureAwait(false);
        if (result.GetProperty("protocol").GetString() != "rrd"
            || result.GetProperty("protocol_version").GetInt32() != 1
            || result.GetProperty("instance").GetProperty("id").GetString() != _instance)
        {
            throw new RrdClientException("RRD capability protocol or instance identity differs");
        }
        return result;
    }

    public Task<JsonElement> EndpointCatalogueAsync(CancellationToken cancellationToken = default) =>
        CallAsync(OperationId.EndpointCatalogue, null, new RequestOptions(), cancellationToken);

    public Task<JsonElement> OpenApiAsync(CancellationToken cancellationToken = default) =>
        CallAsync(OperationId.OpenapiRead, null, new RequestOptions(), cancellationToken);

    public async Task<Session> CreateSessionAsync(
        string principalId,
        string credential,
        object payload,
        RequestOptions options,
        CancellationToken cancellationToken = default)
    {
        string principal = Canonical(principalId, "principal");
        if (string.IsNullOrEmpty(credential))
        {
            throw new RrdClientException("API-key credential must not be empty");
        }
        JsonElement result = await CallAsync(
            OperationId.SessionCreate,
            payload,
            options with { Session = null, ApiKey = new ApiKeyCredentials(principal, credential) },
            cancellationToken).ConfigureAwait(false);
        string sessionId = result.GetProperty("session_id").GetString() ?? string.Empty;
        string token = result.GetProperty("token").GetString() ?? string.Empty;
        if (sessionId.Length == 0 || token.Length == 0)
        {
            throw new RrdClientException("invalid RRD session lease identity");
        }
        return new Session(principal, new SessionLease(sessionId, token));
    }

    public async Task<JsonElement> CallAsync(
        OperationId operation,
        object? payload,
        RequestOptions options,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(options);
        Endpoint endpoint = EndpointCatalog.Get(operation);
        Dictionary<string, object?>? requestContext = MakeContext(endpoint, options);
        string path = ResolvePath(endpoint.Path, options.PathParameters);
        Uri target = new(_baseUri, path.TrimStart('/'));
        byte[]? body = null;
        if (endpoint.Method != "GET")
        {
            IReadOnlyList<ResourceSegment> resource = options.Resource
                ?? DefaultResource(options.PathParameters);
            body = JsonSerializer.SerializeToUtf8Bytes(new Dictionary<string, object?>
            {
                ["protocol"] = "rrd",
                ["protocol_version"] = 1,
                ["context"] = requestContext,
                ["resource"] = new Dictionary<string, object?>
                {
                    ["segments"] = ValidateResource(resource),
                },
                ["payload"] = payload ?? new Dictionary<string, object?>(),
            }, SerializerOptions);
        }
        bool retrySafe = endpoint.Method == "GET" || !endpoint.Mutation
            || options.IdempotencyKey is not null;
        int attempts = retrySafe ? _maxAttempts : 1;
        Exception? lastError = null;
        for (int attempt = 0; attempt < attempts; attempt++)
        {
            TimeSpan timeout = RemainingTimeout(options.Deadline);
            using CancellationTokenSource attemptTimeout = new(timeout);
            using CancellationTokenSource linked = CancellationTokenSource.CreateLinkedTokenSource(
                cancellationToken, attemptTimeout.Token);
            using HttpRequestMessage request = new(new HttpMethod(endpoint.Method), target);
            request.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("application/json"));
            Authenticate(request, endpoint, options);
            if (body is not null)
            {
                request.Content = new ByteArrayContent(body);
                request.Content.Headers.ContentType = new MediaTypeHeaderValue("application/json");
            }
            try
            {
                using HttpResponseMessage response = await _http.SendAsync(
                    request, HttpCompletionOption.ResponseHeadersRead, linked.Token).ConfigureAwait(false);
                byte[] encoded = await ReadBoundedAsync(response, linked.Token).ConfigureAwait(false);
                return DecodeResponse((int)response.StatusCode, encoded, requestContext);
            }
            catch (Exception error) when (
                error is HttpRequestException
                || (error is OperationCanceledException && !cancellationToken.IsCancellationRequested))
            {
                lastError = error;
            }
        }
        cancellationToken.ThrowIfCancellationRequested();
        throw new RrdClientException("RRD transport failed", lastError!);
    }

    public void Dispose()
    {
        if (_ownsHttp)
        {
            _http.Dispose();
        }
    }

    private static Dictionary<string, object?>? MakeContext(Endpoint endpoint, RequestOptions options)
    {
        if (endpoint.Method == "GET")
        {
            return null;
        }
        Dictionary<string, object?> context = new()
        {
            ["request_id"] = Correlation(options.RequestId, "request ID"),
            ["operation_id"] = Correlation(options.OperationId, "operation ID"),
        };
        if (options.IdempotencyKey is not null)
        {
            context["idempotency_key"] = Correlation(options.IdempotencyKey, "idempotency key");
        }
        else if (endpoint.Mutation)
        {
            throw new RrdClientException("mutating requests require an idempotency key");
        }
        if (options.Deadline is not null)
        {
            long milliseconds = options.Deadline.Value.ToUnixTimeMilliseconds();
            if (milliseconds <= 0)
            {
                throw new RrdClientException("deadline must be a positive Unix millisecond instant");
            }
            context["deadline_unix_ms"] = milliseconds;
        }
        return context;
    }

    private static void Authenticate(HttpRequestMessage request, Endpoint endpoint, RequestOptions options)
    {
        if (endpoint.Authentication == Authentication.ApiKey)
        {
            if (options.ApiKey is null || string.IsNullOrEmpty(options.ApiKey.Credential))
            {
                throw new RrdClientException($"{endpoint.WireName} requires API-key authentication");
            }
            request.Headers.Add("X-RRD-Principal", Canonical(options.ApiKey.PrincipalId, "principal"));
            request.Headers.TryAddWithoutValidation("Authorization", "ApiKey " + options.ApiKey.Credential);
        }
        else if (endpoint.Authentication == Authentication.SessionBearer)
        {
            if (options.Session is null
                || options.Session.Lease.SessionId.Length == 0
                || options.Session.Lease.Token.Length == 0)
            {
                throw new RrdClientException($"{endpoint.WireName} requires a valid session");
            }
            request.Headers.Add("X-RRD-Session", options.Session.Lease.SessionId);
            request.Headers.TryAddWithoutValidation(
                "Authorization", "Bearer " + options.Session.Lease.Token);
        }
    }

    private static JsonElement DecodeResponse(
        int status,
        byte[] encoded,
        IReadOnlyDictionary<string, object?>? requestContext)
    {
        try
        {
            using JsonDocument document = JsonDocument.Parse(encoded);
            JsonElement envelope = document.RootElement;
            RequireFields(envelope, "protocol", "protocol_version", "request_id", "operation_id", "outcome");
            if (envelope.GetProperty("protocol").GetString() != "rrd"
                || envelope.GetProperty("protocol_version").GetInt32() != 1)
            {
                throw new RrdClientException("RRD response protocol differs");
            }
            if (requestContext is not null
                && (envelope.GetProperty("request_id").GetString()
                        != (string)requestContext["request_id"]!
                    || envelope.GetProperty("operation_id").GetString()
                        != (string)requestContext["operation_id"]!))
            {
                throw new RrdClientException("RRD response request/operation identity differs");
            }
            JsonElement outcome = envelope.GetProperty("outcome");
            string outcomeStatus = outcome.GetProperty("status").GetString() ?? string.Empty;
            bool success = status is >= 200 and < 300;
            if (success != (outcomeStatus == "ok"))
            {
                throw new RrdClientException("RRD HTTP status and typed outcome disagree");
            }
            if (outcomeStatus == "error")
            {
                RequireFields(outcome, "status", "error");
                JsonElement error = outcome.GetProperty("error");
                RequireFields(error, "code", "message", "retryable", "details");
                Dictionary<string, string> details = error.GetProperty("details")
                    .EnumerateObject().ToDictionary(
                        property => property.Name,
                        property => property.Value.GetString() ?? string.Empty,
                        StringComparer.Ordinal);
                throw new RrdApiException(
                    status,
                    error.GetProperty("code").GetString() ?? string.Empty,
                    error.GetProperty("message").GetString() ?? string.Empty,
                    error.GetProperty("retryable").GetBoolean(),
                    details);
            }
            if (outcomeStatus != "ok")
            {
                throw new RrdClientException("RRD response outcome status is invalid");
            }
            RequireFields(outcome, "status", "payload");
            JsonElement result = outcome.GetProperty("payload");
            if (result.ValueKind != JsonValueKind.Object)
            {
                throw new RrdClientException("RRD success payload must be an object");
            }
            return result.Clone();
        }
        catch (RrdClientException)
        {
            throw;
        }
        catch (Exception error) when (error is JsonException or InvalidOperationException)
        {
            throw new RrdClientException("RRD response envelope is invalid", error);
        }
    }

    private async Task<byte[]> ReadBoundedAsync(
        HttpResponseMessage response,
        CancellationToken cancellationToken)
    {
        if (response.Content.Headers.ContentLength > _maxResponseBytes)
        {
            throw new RrdClientException("RRD response exceeded the configured byte limit");
        }
        await using Stream input = await response.Content.ReadAsStreamAsync(cancellationToken)
            .ConfigureAwait(false);
        using MemoryStream output = new();
        byte[] buffer = ArrayPool<byte>.Shared.Rent(8192);
        try
        {
            int total = 0;
            int count;
            while ((count = await input.ReadAsync(buffer, cancellationToken).ConfigureAwait(false)) > 0)
            {
                total += count;
                if (total > _maxResponseBytes)
                {
                    throw new RrdClientException("RRD response exceeded the configured byte limit");
                }
                output.Write(buffer, 0, count);
            }
            return output.ToArray();
        }
        finally
        {
            ArrayPool<byte>.Shared.Return(buffer);
        }
    }

    private TimeSpan RemainingTimeout(DateTimeOffset? deadline)
    {
        if (deadline is null)
        {
            return _requestTimeout;
        }
        TimeSpan remaining = deadline.Value - DateTimeOffset.UtcNow;
        if (remaining <= TimeSpan.Zero)
        {
            throw new RrdClientException("RRD request deadline has expired");
        }
        return remaining < _requestTimeout ? remaining : _requestTimeout;
    }

    private IReadOnlyList<ResourceSegment> DefaultResource(
        IReadOnlyDictionary<string, string> parameters)
    {
        List<ResourceSegment> result = new();
        if (parameters.TryGetValue("estate", out string? estate))
        {
            result.Add(new ResourceSegment("estate", Canonical(estate, "estate")));
        }
        result.Add(new ResourceSegment("instance", _instance));
        return result;
    }

    private static IReadOnlyList<ResourceSegment> ValidateResource(
        IReadOnlyList<ResourceSegment> segments)
    {
        if (segments.Count is < 1 or > 16)
        {
            throw new RrdClientException("resource paths must contain 1..=16 segments");
        }
        HashSet<string> kinds = new(StringComparer.Ordinal);
        return segments.Select(segment =>
        {
            if (!ResourceKinds.Contains(segment.Kind) || !kinds.Add(segment.Kind))
            {
                throw new RrdClientException("resource kind is unknown or repeated");
            }
            return new ResourceSegment(segment.Kind, Canonical(segment.Id, "resource"));
        }).ToArray();
    }

    private static string ResolvePath(string template, IReadOnlyDictionary<string, string> parameters) =>
        PathParameter().Replace(template, match =>
        {
            string name = match.Groups[1].Value;
            if (!parameters.TryGetValue(name, out string? value) || value.Length == 0)
            {
                throw new RrdClientException($"missing path parameter {name}");
            }
            return Uri.EscapeDataString(Correlation(value, name + " path parameter"));
        });

    private static string Correlation(string? value, string label)
    {
        if (value is null || value.Length is < 1 or > 128 || !CorrelationPattern().IsMatch(value))
        {
            throw new RrdClientException($"{label} is not a canonical RRD correlation ID");
        }
        return value;
    }

    private static string Canonical(string? value, string label)
    {
        if (value is null || value.Length is < 1 or > 128 || !CanonicalPattern().IsMatch(value))
        {
            throw new RrdClientException($"{label} is not a canonical RRD identifier");
        }
        return value;
    }

    private static Uri LoopbackUri(Uri uri)
    {
        bool loopback = uri.Host.Equals("localhost", StringComparison.OrdinalIgnoreCase)
            || (IPAddress.TryParse(uri.Host, out IPAddress? address) && IPAddress.IsLoopback(address));
        if (uri.Scheme != Uri.UriSchemeHttp
            || !loopback
            || !string.IsNullOrEmpty(uri.UserInfo)
            || !string.IsNullOrEmpty(uri.Query)
            || !string.IsNullOrEmpty(uri.Fragment))
        {
            throw new RrdClientException(
                "RRD .NET client permits only credential-free loopback HTTP before TLS qualification");
        }
        return new Uri(uri.AbsoluteUri.TrimEnd('/') + "/", UriKind.Absolute);
    }

    private static void RequireFields(JsonElement element, params string[] expected)
    {
        if (element.ValueKind != JsonValueKind.Object)
        {
            throw new RrdClientException("RRD response object is invalid");
        }
        HashSet<string> actual = element.EnumerateObject()
            .Select(property => property.Name)
            .ToHashSet(StringComparer.Ordinal);
        if (!actual.SetEquals(expected))
        {
            throw new RrdClientException("RRD response object fields differ");
        }
    }

    [GeneratedRegex("^[A-Za-z0-9._:-]+$", RegexOptions.CultureInvariant)]
    private static partial Regex CorrelationPattern();

    [GeneratedRegex("^[a-z0-9][a-z0-9._-]*$", RegexOptions.CultureInvariant)]
    private static partial Regex CanonicalPattern();

    [GeneratedRegex("\\{([a-z]+)\\}", RegexOptions.CultureInvariant)]
    private static partial Regex PathParameter();
}

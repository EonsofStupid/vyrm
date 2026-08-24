using System;
using System.Collections.Generic;

namespace Rrflow.Rrd;

public class RrdClientException : Exception
{
    public RrdClientException(string message) : base(message) { }
    public RrdClientException(string message, Exception innerException)
        : base(message, innerException) { }
}

public sealed class RrdApiException : RrdClientException
{
    public RrdApiException(
        int status,
        string code,
        string message,
        bool retryable,
        IReadOnlyDictionary<string, string> details)
        : base($"RRD API {status}: {code}: {message}")
    {
        Status = status;
        Code = code;
        Retryable = retryable;
        Details = details;
    }

    public int Status { get; }
    public string Code { get; }
    public bool Retryable { get; }
    public IReadOnlyDictionary<string, string> Details { get; }
}

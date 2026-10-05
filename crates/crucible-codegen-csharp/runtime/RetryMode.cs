/// <summary>How a client limits its retries; docs/sdk-runtime-policy.md is the cross-language contract.</summary>
public enum RetryMode
{
    /// <summary>Retries every eligible failure up to the attempt limit, without a quota.</summary>
    Legacy,
    /// <summary>Adds the client's shared retry quota.</summary>
    Standard,
    /// <summary>Standard plus a client-side send-rate limiter that reacts to throttling.</summary>
    Adaptive,
}

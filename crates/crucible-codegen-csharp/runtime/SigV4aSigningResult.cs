/// <summary>The intermediate values of a SigV4a signature, for conformance checks.</summary>
public sealed record SigV4aSigningResult(string CanonicalRequest, string StringToSign, string Signature);

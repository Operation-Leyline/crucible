# XML client qualification

Java and Ruby run the official Smithy XML protocol models from
[`smithy-lang/smithy` revision bd0ed9e](https://github.com/smithy-lang/smithy/tree/bd0ed9e054b29874a5bec389e16d8c4db9dc9b94/smithy-aws-protocol-tests/model).
The fixtures are assembled and validated with Smithy 1.74.0, transformed using
`flattenAndRemoveMixins`, and serialized through Smithy's model serializer.
Their `crucibleQualification` metadata records the revision, compiler, source
paths, SHA-256 fingerprints, and excluded operations.

Refresh the fixtures with a JDK and network access:

```text
python tools/import-smithy-xml.py
```

The native gates use the checked-in JSON and require no Smithy compiler or
download during qualification:

```text
python tools/test-java.py
python tools/test-ruby.py
python tools/test_protocol_conformance.py
```

## Scope and reports

There are 335 official cases across the five services. Six compression cases
are excluded from the supported service projection. Their operation shapes,
traits, and original vectors remain in the fixtures. Reattaching each operation
produces the existing shape-specific `@requestCompression` diagnostic.

| Official service | Cases in projection | Applicable supported cases | Other cases |
|---|---:|---:|---|
| RestXml | 182 | 176 | 6 server-only |
| RestXmlWithNamespace | 2 | 2 | |
| AmazonS3 | 13 | 2 | 11 unsupported legacy addressing configurations |
| AwsQuery | 75 | 75 | |
| AwsEc2 | 57 | 57 | |
| Total | 329 | 312 | 6 server-only, 11 unsupported |

The eleven S3 cases require legacy scoped vendor configuration for virtual/path
addressing, accelerate, and dualstack endpoints. That integration is not
implemented. The S3 XML cases exercise unwrapped success and error responses.
`@s3UnwrappedXmlOutput` is resolved once in the operation IR; both runtimes
consume the resulting flag.

Each native gate writes a report per service, `xml-excluded-operations.json`,
and `xml-unsupported-s3-endpoints.json`. S3 retains `complete: false` and eleven
`unsupported` results. Its gate accepts only the exact declared unsupported
case set, with passing supported cases and no failures, recording
`qualifiedSubset: true`. An unexpected unsupported case, stale allowance,
or failing case fails the gate. Exclusions never count as passing tests.

The earlier 141 handwritten cases are preserved separately in
`xml-additions-{rest-xml,aws-query,ec2-query}.json`. They run alongside the
official suite and support the independent loopback HTTP consumers.

## Adapter boundaries

Requests use generated codecs and runtime preparation. Responses use native
runtime decoding and model round trips. Adapters provide inert SigV4 credentials
and a deterministic idempotency-token supplier. For captured buffered requests,
they include the byte length that the native transport adds after preparation.
Separate loopback consumers verify actual HTTP behavior.

Error vendor parameters describe server-side error construction. The client
adapters validate their code/type against the supplied XML before acknowledging
them. Other vendor configurations remain unsupported.

The comparator normalizes structure-member ordering, namespace prefixes, XML
indentation, and form parameter ordering. It preserves repeated XML element
order, mixed content, leaf text, and exact empty-body expectations. UTF-8 is
explicit at source import and native adapter boundaries.

These gates qualify the supported protocol subset locally. Published artifacts,
the complete native CI matrix, and authenticated AWS or Leyline workflows
remain separate evidence.

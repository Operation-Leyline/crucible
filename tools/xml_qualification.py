"""Shared XML qualification services and explicitly diagnosed source exclusions."""
import json
from pathlib import Path

FIXTURES = Path(__file__).resolve().parents[1] / "crates/crucible-smithy/tests/fixtures"
REGRESSIONS = [
    ("xml", "xml-additions-rest-xml.json", "aws.protocoltests.restxml#RestXml", "aws.protocols#restXml"),
    ("nowrap", "xml-additions-rest-xml.json", "aws.protocoltests.restxml.nowrap#RestXmlNoErrorWrapping", "aws.protocols#restXml"),
    ("query", "xml-additions-aws-query.json", "aws.protocoltests.query#AwsQuery", "aws.protocols#awsQuery"),
    ("ec2", "xml-additions-ec2-query.json", "aws.protocoltests.ec2#AwsEc2", "aws.protocols#ec2Query"),
]
OFFICIAL = [
    ("officialxml", "protocol-rest-xml.json", "aws.protocoltests.restxml#RestXml", "aws.protocols#restXml"),
    ("namespace", "protocol-rest-xml.json", "aws.protocoltests.restxml.xmlns#RestXmlWithNamespace", "aws.protocols#restXml"),
    ("s3", "protocol-rest-xml.json", "com.amazonaws.s3#AmazonS3", "aws.protocols#restXml"),
    ("officialquery", "protocol-aws-query.json", "aws.protocoltests.query#AwsQuery", "aws.protocols#awsQuery"),
    ("officialec2", "protocol-ec2-query.json", "aws.protocoltests.ec2#AwsEc2", "aws.protocols#ec2Query"),
]
PACKAGES = REGRESSIONS + OFFICIAL
S3_ENDPOINT_TESTS = [
    "S3EscapeObjectKeyInUriLabel", "S3EscapePathObjectKeyInUriLabel",
    "S3PreservesEmbeddedDotSegmentInUriLabel", "S3PreservesLeadingDotSegmentInUriLabel",
    "S3DefaultAddressing", "S3OperationAddressingPreferred", "S3PathAddressing",
    "S3VirtualHostAccelerateAddressing", "S3VirtualHostAddressing",
    "S3VirtualHostDualstackAccelerateAddressing", "S3VirtualHostDualstackAddressing",
]


def conformance_options(label):
    return [arg for test in S3_ENDPOINT_TESTS for arg in ("--allow-unsupported", test)] if label == "s3" else []


def write_exclusions(output):
    exclusions = []
    for fixture in sorted({fixture for _, fixture, _, _ in OFFICIAL}):
        metadata = json.loads((FIXTURES / fixture).read_text(encoding="utf-8"))["metadata"]["crucibleQualification"]
        exclusions += [{"fixture": fixture, "revision": metadata["revision"], "status": "unsupported", **operation}
                       for operation in metadata["excludedOperations"]]
    (output / "xml-excluded-operations.json").write_text(json.dumps(exclusions, indent=2) + "\n", encoding="utf-8")
    (output / "xml-unsupported-s3-endpoints.json").write_text(json.dumps({
        "status": "unsupported", "tests": S3_ENDPOINT_TESTS,
        "reason": "Legacy scoped S3 addressing configuration is not implemented; XML response cases run separately"
    }, indent=2) + "\n", encoding="utf-8")
    print("XML source exclusions:", json.dumps(exclusions))

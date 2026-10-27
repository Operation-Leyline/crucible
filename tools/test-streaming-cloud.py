#!/usr/bin/env python3
"""Opt-in AWS streaming qualification; creates only tagged temporary resources.

Requires boto3, explicit identity arguments, and a Linux musl bootstrap zip.
See docs/streaming.md. Never run this from ordinary credential-free CI.
"""
import argparse
import hashlib
import http.client
import io
import json
import re
import socket
import time
import uuid
import zipfile
from pathlib import Path
from urllib.parse import urlsplit

import boto3
from botocore.auth import SigV4Auth
from botocore.awsrequest import AWSRequest
from botocore.exceptions import ClientError


def parse_fixture_frame(text):
    """Decode the fixture's deliberately narrow two-line SSE event format."""
    lines = text.split("\n")
    if len(lines) != 2 or not lines[0].startswith("event: ") or not lines[1].startswith("data: "):
        raise ValueError(f"unexpected fixture frame: {text!r}")
    kind = lines[0][7:]
    if kind not in ("message", "crucible.complete", "crucible.error"):
        raise ValueError(f"unexpected fixture event: {kind!r}")
    return kind, json.loads(lines[1][6:])


def validate_fixture_events(mode, events, cancel=False):
    """Check the fixture contract, not arbitrary SSE or generated SDK semantics."""
    first = ("message", {"ready": "item-1"})
    expected = {
        "normal": [first, ("message", {"ready": "item-2"}), ("crucible.complete", {})],
        "zero": [("crucible.complete", {})],
        "late": [first, ("crucible.error", {"type": "WorkFailed", "status": 503,
                                          "error": {"message": "cloud late failure"}})],
        # Unmatched DeadlineExceeded is intentionally sanitized by the framework.
        "hold": [first, ("crucible.error", {"type": "InternalFailure", "status": 500,
                                          "error": {"message": "Internal server error"}})],
    }
    if mode not in expected or (cancel and mode != "hold"):
        raise ValueError(f"unknown fixture scenario: {mode}, cancel={cancel}")
    wanted = [first] if cancel else expected[mode]
    decoded = [parse_fixture_frame(event["text"]) for event in events]
    if decoded != wanted:
        raise ValueError(f"fixture {mode} events differ: expected {wanted!r}, received {decoded!r}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("profile", "region", "account", "principal", "bootstrap-zip", "report-dir"):
        parser.add_argument("--" + name, required=True)
    args = parser.parse_args()
    session = boto3.Session(profile_name=args.profile, region_name=args.region)
    identity = session.client("sts").get_caller_identity()
    if identity["Account"] != args.account or identity["Arn"] != args.principal:
        raise SystemExit("STS identity mismatch; no resources created")
    if not re.fullmatch(r"\d{12}", args.account):
        raise SystemExit("expected a 12-digit AWS account")
    zipped = Path(args.bootstrap_zip).read_bytes()
    with zipfile.ZipFile(io.BytesIO(zipped)) as archive:
        entry = archive.getinfo("bootstrap")
        binary = archive.read(entry)
        if binary[:5] != b"\x7fELF\x02" or binary[18:20] != b"\x3e\x00" or not (entry.external_attr >> 16) & 0o111:
            raise SystemExit("bootstrap must be an executable x86_64 Linux ELF")
    report_dir = Path(args.report_dir)
    report_dir.mkdir(parents=True, exist_ok=True)
    run = "crucible-stream-smoke-" + time.strftime("%m%d%H%M%S", time.gmtime()) + "-" + uuid.uuid4().hex[:6]
    tags = {"crucible-smoke": run, "purpose": "temporary-streaming-qualification"}
    group = "/aws/lambda/" + run
    report = {"run": run, "profile": args.profile, "region": args.region,
              "tags": tags, "request_budget_seconds": 5, "lambda_timeout_seconds": 15,
              "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "identity": {key: identity[key] for key in ("Account", "Arn", "UserId")},
              "resources": {}, "journal": [], "cases": [], "cleanup": {}, "logs": []}
    report["bootstrap_zip_sha256"] = hashlib.sha256(zipped).hexdigest()
    receipt = report_dir / (run + ".json")
    clients = {name: session.client(name) for name in ("iam", "lambda", "apigateway", "logs")}
    iam, lam, api, logs = (clients[name] for name in ("iam", "lambda", "apigateway", "logs"))
    resources = report["resources"]

    def save():
        temporary = receipt.with_suffix(".tmp")
        temporary.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        temporary.replace(receipt)

    def action(label, call, **kwargs):
        # Persist intent before AWS can create a billable resource. Record only
        # labels and owned resource IDs, never credentials or signed requests.
        report["journal"].append({"action": label, "state": "before", "time": time.time()})
        save()
        result = call(**kwargs)
        report["journal"].append({"action": label, "state": "after", "time": time.time()})
        save()
        return result

    def retry(call, seconds=35):
        until = time.monotonic() + seconds
        while True:
            try:
                return call()
            except ClientError as exc:
                if time.monotonic() >= until or exc.response["Error"]["Code"] not in (
                    "InvalidParameterValueException", "TooManyRequestsException", "ConflictException"
                ):
                    raise
                time.sleep(2)

    def request(base, service, method, mode, cancel=False, path="events"):
        nonce = uuid.uuid4().hex
        url = base.rstrip("/") + "/" + path
        body = json.dumps({"topic": mode}).encode() if method == "POST" else b""
        headers = {"Content-Type": "application/json", "x-cloud-mode": mode,
                   "x-cloud-nonce": nonce, "x-amz-content-sha256": hashlib.sha256(body).hexdigest()}
        signed = AWSRequest(method=method, url=url, data=body, headers=headers)
        SigV4Auth(session.get_credentials().get_frozen_credentials(), service, args.region).add_auth(signed)
        target = urlsplit(url)
        connection = http.client.HTTPSConnection(target.hostname, timeout=12)
        start = time.monotonic()
        item = {"endpoint": service, "method": method, "mode": mode, "nonce": nonce,
                "cancel": cancel, "events": [], "started_unix_ms": int(time.time() * 1000)}
        report["cases"].append(item)
        save()
        try:
            connection.request(method, target.path, body=body, headers=dict(signed.headers))
            response = connection.getresponse()
            item["headers_ms"] = round((time.monotonic() - start) * 1000, 2)
            item["status"] = response.status
            item["headers"] = {key.lower(): value for key, value in response.getheaders()
                               if key.lower() in ("content-type", "cache-control", "crucible-stream", "x-request-id")}
            if path == "read":
                item["body"] = response.read().decode()
                assert response.status == 200 and json.loads(item["body"]) == {"value": "unary"}
                return
            if response.status != 200:
                item["body"] = response.read(8192).decode(errors="replace")
                raise AssertionError(f"{service} {method} {mode}: HTTP {response.status} {item['body']}")
            media = item["headers"].get("content-type", "")
            assert "text/event-stream" in media and "crucible-stream=1" in media, media
            assert item["headers"].get("cache-control") == "no-store"
            assert item["headers"].get("crucible-stream") == "1"
            assert item["headers"].get("x-request-id")
            frame = []
            while True:
                line = response.readline()
                if not line:
                    break
                if line.strip():
                    frame.append(line.decode().rstrip("\r\n"))
                    continue
                if frame and not all(line.startswith(":") for line in frame):
                    text = "\n".join(frame)
                    kind, _ = parse_fixture_frame(text)
                    event = {"text": text, "ms": round((time.monotonic() - start) * 1000, 2)}
                    item["events"].append(event)
                    if cancel and kind == "message":
                        validate_fixture_events(mode, item["events"], cancel=True)
                        item["client_close_ms"] = event["ms"]
                        # Close the actual TCP connection, not just an iterator.
                        if connection.sock is not None:
                            connection.sock.shutdown(socket.SHUT_RDWR)
                        response.close()
                        frame = []
                        break
                frame = []
            if frame:
                raise ValueError("unterminated fixture frame")
            item["close_ms"] = round((time.monotonic() - start) * 1000, 2)
            events = item["events"]
            validate_fixture_events(mode, events, cancel)
            if not cancel:
                if mode == "hold":
                    assert 4500 <= item["close_ms"] < 10000, item
                if mode in ("late", "hold"):
                    gap = events[-1]["ms"] - events[0]["ms"]
                    minimum = 1000 if mode == "late" else 4000
                    if gap <= minimum:
                        raise ValueError(f"first {mode} item was buffered until terminal: {gap}ms")
                    item["first_item_before_terminal_ms"] = round(gap, 2)
                if mode == "normal":
                    gap = events[-1]["ms"] - events[0]["ms"]
                    assert gap > 2200, f"response was buffered: {gap}ms"
                    item["first_item_before_terminal_ms"] = round(gap, 2)
                    second_gap = events[-1]["ms"] - events[1]["ms"]
                    if second_gap <= 1000:
                        raise ValueError(f"second item was buffered until terminal: {second_gap}ms")
                    item["second_item_before_terminal_ms"] = round(second_gap, 2)
        finally:
            connection.close()
            save()

    def cleanup(name, verify_owned, delete, absent):
        try:
            if verify_owned():
                delete()
            until = time.monotonic() + 20
            while not absent():
                if time.monotonic() >= until:
                    raise RuntimeError(f"{name} still exists")
                time.sleep(1)
            report["cleanup"][name] = "deleted-and-verified"
        except Exception as exc:
            report["cleanup"][name] = {"error": str(exc)}
        save()

    def missing(call, codes):
        try:
            call()
            return False
        except ClientError as exc:
            if exc.response["Error"]["Code"] in codes:
                return True
            raise

    def owned(value):
        if value.get("crucible-smoke") != run:
            raise RuntimeError("resource ownership tag mismatch; refusing deletion")
        return True

    save()
    try:
        # Named resources are known before creation, allowing recovery from a
        # timeout after AWS accepted a request but before its response arrived.
        resources["role"] = run
        resources["log_group"] = group
        resources["function"] = run
        save()
        role = action("create-role", iam.create_role, RoleName=run,
                      AssumeRolePolicyDocument=json.dumps({"Version": "2012-10-17", "Statement": [{
                          "Effect": "Allow", "Principal": {"Service": "lambda.amazonaws.com"},
                          "Action": "sts:AssumeRole"}]}),
                      Tags=[{"Key": key, "Value": value} for key, value in tags.items()])["Role"]
        resources["role_arn"] = role["Arn"]
        action("create-log-group", logs.create_log_group, logGroupName=group, tags=tags)
        action("put-retention", logs.put_retention_policy, logGroupName=group, retentionInDays=1)
        action("put-role-policy", iam.put_role_policy, RoleName=run, PolicyName="owned-logs-only",
               PolicyDocument=json.dumps({"Version": "2012-10-17", "Statement": [{
                   "Effect": "Allow", "Action": ["logs:CreateLogStream", "logs:PutLogEvents"],
                   "Resource": f"arn:aws:logs:{args.region}:{args.account}:log-group:{group}:*"}]}))
        function = retry(lambda: action("create-function", lam.create_function, FunctionName=run,
                         Runtime="provided.al2023", Role=role["Arn"], Handler="bootstrap",
                         Code={"ZipFile": zipped}, Timeout=15, MemorySize=256,
                         Architectures=["x86_64"], Tags=tags))
        resources["function_arn"] = function["FunctionArn"]
        lam.get_waiter("function_active_v2").wait(FunctionName=run)
        url = action("create-function-url", lam.create_function_url_config, FunctionName=run,
                     AuthType="AWS_IAM", InvokeMode="RESPONSE_STREAM")["FunctionUrl"]
        resources["function_url"] = url
        action("grant-owned-url-invocation", lam.add_permission, FunctionName=run,
               StatementId="verified-smoke-principal-url", Action="lambda:InvokeFunctionUrl",
               Principal=args.principal, FunctionUrlAuthType="AWS_IAM")
        action("grant-owned-url-function-invocation", lam.add_permission, FunctionName=run,
               StatementId="verified-smoke-principal-function", Action="lambda:InvokeFunction",
               Principal=args.principal, InvokedViaFunctionUrl=True)
        rest = action("create-rest-api", api.create_rest_api, name=run,
                      endpointConfiguration={"types": ["REGIONAL"]}, tags=tags)
        resources["rest_api"] = rest["id"]
        resources["rest_api_arn"] = f"arn:aws:apigateway:{args.region}::/restapis/{rest['id']}"
        save()
        root = api.get_resources(restApiId=rest["id"])["items"][0]["id"]
        for path in ("events", "read"):
            resource = action("create-resource-" + path, api.create_resource,
                              restApiId=rest["id"], parentId=root, pathPart=path)
            for method in (("GET", "POST") if path == "events" else ("GET",)):
                common = dict(restApiId=rest["id"], resourceId=resource["id"], httpMethod=method)
                action("put-method-" + path + method, api.put_method, **common, authorizationType="AWS_IAM")
                action("put-integration-" + path + method, api.put_integration, **common,
                       type="AWS_PROXY", integrationHttpMethod="POST", responseTransferMode="STREAM",
                       uri=f"arn:aws:apigateway:{args.region}:lambda:path/2021-11-15/functions/{function['FunctionArn']}/response-streaming-invocations")
        action("grant-api-invocation", lam.add_permission, FunctionName=run, StatementId="owned-rest-api",
               Action="lambda:InvokeFunction", Principal="apigateway.amazonaws.com",
               SourceAccount=args.account,
               SourceArn=f"arn:aws:execute-api:{args.region}:{args.account}:{rest['id']}/*/*/*")
        action("deploy-api", api.create_deployment, restApiId=rest["id"], stageName="smoke")
        rest_url = f"https://{rest['id']}.execute-api.{args.region}.amazonaws.com/smoke"
        resources["rest_url"] = rest_url
        save()
        # Small propagation grace is bounded and does not retry business calls.
        time.sleep(3)
        for base, service in ((url, "lambda"), (rest_url, "execute-api")):
            request(base, service, "GET", "unary", path="read")
            for method in ("GET", "POST"):
                for mode in ("normal", "zero", "late", "hold"):
                    request(base, service, method, mode)
                request(base, service, method, "hold", cancel=True)
            # Give even a disconnected invocation time to hit its own deadline.
            time.sleep(6)
        report["result"] = "passed"
    except Exception as exc:
        report["result"] = "failed"
        report["error"] = str(exc)
    finally:
        # Recover named resources even when a create response was interrupted.
        for kind in ("function", "role", "rest_api"):
            try:
                if kind == "function" and "function_arn" not in resources and not missing(lambda: lam.get_function(FunctionName=run), ("ResourceNotFoundException",)):
                    resources["function_arn"] = lam.get_function(FunctionName=run)["Configuration"]["FunctionArn"]
                if kind == "role" and "role_arn" not in resources and not missing(lambda: iam.get_role(RoleName=run), ("NoSuchEntity",)):
                    resources["role_arn"] = iam.get_role(RoleName=run)["Role"]["Arn"]
                if kind == "rest_api" and "rest_api" not in resources:
                    for page in api.get_paginator("get_rest_apis").paginate():
                        for candidate in page["items"]:
                            if candidate["name"] == run and candidate.get("tags", {}).get("crucible-smoke") == run:
                                resources["rest_api"] = candidate["id"]
            except Exception as exc:
                report["cleanup"][kind + "_recovery"] = {"error": str(exc)}
        save()
        # Collect logs before destroying the group; CloudWatch delivery is
        # eventually consistent. The fixture owns a finite five-second budget.
        try:
            streams = [case for case in report["cases"] if case.get("events")]
            if "function_arn" in resources and streams:
                for attempt in range(12):
                    report["logs"] = []
                    for page in logs.get_paginator("filter_log_events").paginate(logGroupName=group):
                        report["logs"].extend({key: event[key] for key in ("timestamp", "message")}
                                              for event in page.get("events", []))
                    reports = sum("REPORT RequestId:" in event["message"] for event in report["logs"])
                    if reports >= len(report["cases"]) and all(any(f"nonce={case['nonce']} event=producer-drop" in event["message"] for event in report["logs"]) for case in streams):
                        break
                    time.sleep(2)
                for case in streams:
                    lines = [event["message"] for event in report["logs"] if f"nonce={case['nonce']} " in event["message"]]
                    drops = [line for line in lines if "event=producer-drop " in line]
                    completions = [line for line in lines if "event=stream-complete " in line]
                    assert len(drops) == 1 and len(completions) == 1, f"missing or duplicate lifecycle logs for {case['nonce']}"
                    case["server_drop_ms"] = int(re.search(r"elapsed_ms=(\d+)", drops[0]).group(1))
                    case["server_terminal"] = re.search(r"end=(\w+)", completions[0]).group(1)
                    if case["cancel"]:
                        # AWS may continue producing after socket close. Record
                        # that fact rather than asserting unsupported propagation.
                        case["disconnect_propagated_before_budget"] = case["server_drop_ms"] < 4000
                        assert case["server_drop_ms"] < 7000, case
                    if case["mode"] == "hold" and not case.get("disconnect_propagated_before_budget"):
                        assert 4500 <= case["server_drop_ms"] < 7000, case
                        assert case["server_terminal"] == "Failed", case
                if report.get("result") == "passed":
                    assert reports >= len(report["cases"]), "missing Lambda invocation completion reports"
                    assert not any("Task timed out" in event["message"] or "Status: timeout" in event["message"] for event in report["logs"]), "Lambda hit its hard timeout"
            save()
        except Exception as exc:
            report["result"] = "failed"
            report["log_validation_error"] = str(exc)
            save()
        if "rest_api" in resources:
            api_id = resources["rest_api"]
            cleanup("rest_api", lambda: owned(api.get_rest_api(restApiId=api_id).get("tags", {})),
                    lambda: api.delete_rest_api(restApiId=api_id),
                    lambda: missing(lambda: api.get_rest_api(restApiId=api_id), ("NotFoundException",)))
        if "function_arn" in resources:
            def delete_function():
                if not missing(lambda: lam.get_function_url_config(FunctionName=run), ("ResourceNotFoundException",)):
                    lam.delete_function_url_config(FunctionName=run)
                lam.delete_function(FunctionName=run)
            cleanup("function", lambda: owned(lam.list_tags(Resource=resources["function_arn"])["Tags"]),
                    delete_function,
                    lambda: missing(lambda: lam.get_function(FunctionName=run), ("ResourceNotFoundException",)))
        if "role_arn" in resources:
            def delete_role():
                if not missing(lambda: iam.get_role_policy(RoleName=run, PolicyName="owned-logs-only"), ("NoSuchEntity",)):
                    iam.delete_role_policy(RoleName=run, PolicyName="owned-logs-only")
                iam.delete_role(RoleName=run)
            cleanup("role", lambda: owned({tag["Key"]: tag["Value"] for tag in iam.list_role_tags(RoleName=run)["Tags"]}),
                    delete_role, lambda: missing(lambda: iam.get_role(RoleName=run), ("NoSuchEntity",)))
        def group_exists():
            return any(item["logGroupName"] == group for item in logs.describe_log_groups(logGroupNamePrefix=group)["logGroups"])
        cleanup("log_group", lambda: group_exists() and owned(logs.list_tags_log_group(logGroupName=group)["tags"]),
                lambda: logs.delete_log_group(logGroupName=group), lambda: not group_exists())
        save()
    print(json.dumps({"receipt": str(receipt), "result": report.get("result"), "cleanup": report["cleanup"]}))
    assert report.get("result") == "passed", report.get("error", "smoke failed")
    assert all(value == "deleted-and-verified" for value in report["cleanup"].values()), "cleanup incomplete; inspect receipt"


if __name__ == "__main__":
    main()

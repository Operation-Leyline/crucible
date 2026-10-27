"""Offline failure-path tests: no AWS SDK, credentials, or Docker execution."""
import argparse
import ast
import contextlib
import hashlib
import io
import json
from pathlib import Path
import re
import sys
import tempfile
import types
import unittest
from unittest.mock import patch
import uuid
import zipfile

import test_streaming_cloud as oracle

SOURCE = Path(__file__).with_name("test-streaming-cloud.py")
BUILDER = Path(__file__).with_name("build-streaming-cloud.py")


def source_functions(env, *, optimize=0):
    env.setdefault("HTTPClientError", type("HTTPClientError", (Exception,), {}))
    tree = ast.parse(SOURCE.read_text(encoding="utf-8"))
    functions = [node for node in tree.body if isinstance(node, ast.FunctionDef)]
    exec(compile(ast.fix_missing_locations(ast.Module(body=functions, type_ignores=[])), str(SOURCE), "exec", optimize=optimize), env)
    return next(node for node in functions if node.name == "main")


def load_builder(optimize=0):
    env = {"__name__": "offline_build", "__file__": str(BUILDER)}
    exec(compile(BUILDER.read_text(encoding="utf-8"), str(BUILDER), "exec", optimize=optimize), env)
    return types.SimpleNamespace(**env)


def fake_inputs(root, valid=True):
    binary = root / "artifacts/target/release/cloud_fixture"
    binary.parent.mkdir(parents=True)
    binary.write_bytes(b"\x7fELF\x02" + bytes(13) + b"\x3e\x00" if valid else b"MZnot-a-linux-bootstrap")
    for path in ("tests/consumers/streaming/src/bin/cloud_fixture.rs",
                 "tests/consumers/streaming/Cargo.lock", "crates/crucible-server-runtime-rust/src/host.rs"):
        entry = root / "source" / path
        entry.parent.mkdir(parents=True, exist_ok=True)
        entry.write_text("offline fixture")
    return root / "artifacts", root / "source"


def lost_role_response(*, visible_after=2, partition="aws", wrong_identity=False, wrong_tag=False, denied=False, retain=True, kind="role"):
    """Execute real main: a service accepts a create, then loses its response."""
    class ClientError(Exception):
        def __init__(self, code):
            super().__init__(code)
            self.response = {"Error": {"Code": code}}

    state = {"role": None, "function": None, "rest_api": None, "log_group": None,
             "role_reads": 0, "reads": 0, "create_calls": 0, "deletes": 0, "now": 0, "sts_calls": 0,
             "policy": False, "url": False}

    def fail(code):
        raise ClientError(code)

    def accepted(resource, record):
        if resource != kind or retain:
            state[resource] = record
        if resource == kind:
            raise TimeoutError("lost create response")

    def hidden(resource):
        if resource == kind:
            state["reads"] += 1
            return state["reads"] <= visible_after
        return False

    class Iam:
        def create_role(self, **kwargs):
            state["create_calls"] += 1
            accepted("role", kwargs)
            return {"Role": {"Arn": "arn:aws:iam::123456789012:role/" + kwargs["RoleName"]}}

        def get_role(self, **kwargs):
            state["role_reads"] += 1
            if denied:
                fail("AccessDeniedException")
            if state["role"] is None or hidden("role"):
                fail("NoSuchEntity")
            return {"Role": {"Arn": "arn:aws:iam::123456789012:role/" + kwargs["RoleName"]}}

        def list_role_tags(self, **kwargs):
            return {"Tags": [{"Key": "crucible-smoke", "Value": "different"}] if wrong_tag else state["role"]["Tags"]}

        def get_role_policy(self, **kwargs):
            if not state["policy"]:
                fail("NoSuchEntity")
            return {"PolicyDocument": "offline"}

        def put_role_policy(self, **kwargs):
            state["policy"] = True

        def delete_role_policy(self, **kwargs):
            state["policy"] = False

        def delete_role(self, **kwargs):
            state["role"] = None
            state["deletes"] += 1

    class Lambda:
        def create_function(self, **kwargs):
            accepted("function", kwargs)
            return {"FunctionArn": "arn:aws:lambda:us-east-1:123456789012:function:" + kwargs["FunctionName"]}

        def get_waiter(self, name):
            return types.SimpleNamespace(wait=lambda **kwargs: None)

        def get_function(self, **kwargs):
            if state["function"] is None or hidden("function"):
                fail("ResourceNotFoundException")
            return {"Configuration": {"FunctionArn": "arn:aws:lambda:us-east-1:123456789012:function:" + kwargs["FunctionName"]}}

        def list_tags(self, **kwargs):
            return {"Tags": state["function"]["Tags"]}

        def create_function_url_config(self, **kwargs):
            state["url"] = True
            return {"FunctionUrl": "https://offline.invalid"}

        def get_function_url_config(self, **kwargs):
            if not state["url"]:
                fail("ResourceNotFoundException")
            return {"FunctionUrl": "https://offline.invalid"}

        def delete_function_url_config(self, **kwargs):
            state["url"] = False

        def add_permission(self, **kwargs):
            return {}

        def delete_function(self, **kwargs):
            state["function"] = None

    class Logs:
        def create_log_group(self, **kwargs):
            accepted("log_group", kwargs)

        def put_retention_policy(self, **kwargs):
            pass

        def describe_log_groups(self, **kwargs):
            return {"logGroups": [] if state["log_group"] is None or hidden("log_group") else [{"logGroupName": state["log_group"]["logGroupName"]}]}

        def get_paginator(self, operation):
            return types.SimpleNamespace(paginate=lambda **kwargs: [self.describe_log_groups(**kwargs)])

        def list_tags_log_group(self, **kwargs):
            return {"tags": state["log_group"]["tags"]}

        def delete_log_group(self, **kwargs):
            state["log_group"] = None

    class Api:
        def create_rest_api(self, **kwargs):
            accepted("rest_api", kwargs)

        def get_paginator(self, operation):
            def pages():
                return [{"items": [] if state["rest_api"] is None or hidden("rest_api") else [
                    {"id": "owned-api", "name": state["rest_api"]["name"], "tags": state["rest_api"]["tags"]}]}]
            return types.SimpleNamespace(paginate=pages)

        def get_rest_api(self, **kwargs):
            if state["rest_api"] is None:
                fail("NotFoundException")
            return {"tags": state["rest_api"]["tags"]}

        def delete_rest_api(self, **kwargs):
            state["rest_api"] = None

    class Sts:
        def get_caller_identity(self):
            state["sts_calls"] += 1
            return {"Account": "999999999999" if wrong_identity else "123456789012",
                    "Arn": "arn:aws:iam::123456789012:user/offline", "UserId": "offline"}

    clients = {"iam": Iam(), "lambda": Lambda(), "logs": Logs(), "sts": Sts(), "apigateway": Api()}
    session = types.SimpleNamespace(get_partition_for_region=lambda _: partition, client=lambda name: clients[name])
    clock = types.SimpleNamespace(monotonic=lambda: state["now"], time=lambda: state["now"],
                                  sleep=lambda seconds: state.__setitem__("now", state["now"] + seconds),
                                  strftime=lambda *args: "offline", gmtime=lambda: None)
    env = dict(argparse=argparse, hashlib=hashlib, io=io, json=json, re=re, uuid=uuid,
               zipfile=zipfile, Path=Path, time=clock, ClientError=ClientError,
               boto3=types.SimpleNamespace(Session=lambda **kwargs: session), __doc__="offline failure test")
    source_functions(env, optimize=2)
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        binary = b"\x7fELF\x02" + bytes(13) + b"\x3e\x00"
        bootstrap = root / "bootstrap.zip"
        entry = zipfile.ZipInfo("bootstrap")
        entry.external_attr = 0o100755 << 16
        with zipfile.ZipFile(bootstrap, "w") as archive:
            archive.writestr(entry, binary)
        argv = [str(SOURCE), "--profile", "offline", "--region", "us-east-1", "--account", "123456789012",
                "--principal", "arn:aws:iam::123456789012:user/offline", "--bootstrap-zip", str(bootstrap), "--report-dir", str(root)]
        with patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
            try:
                env["main"]()
            except (SystemExit, AssertionError):
                pass
        receipts = list(root.glob("crucible-stream-smoke-*.json"))
        report = json.loads(receipts[0].read_text()) if receipts else None
    return state, report


class CloudSafety(unittest.TestCase):
    def test_optimized_case_gates_still_reject_bad_headers_unary_and_timing(self):
        scenarios = (
            (oracle.frames(oracle.COMPLETE), "zero", {"headers": {"content-type": "text/plain"}}),
            ([], "unary", {"body": '{"value":"wrong"}'}),
            (oracle.frames(oracle.MESSAGE_1, oracle.MESSAGE_2, oracle.COMPLETE, times=[0, 500, 2000]), "normal", {}),
        )
        for events, mode, options in scenarios:
            with self.subTest(mode=mode), self.assertRaises(AssertionError):
                oracle.replay_request(events, mode, optimize=2, **options)

    def test_optimized_final_failure_always_exits(self):
        env = {"report": {"result": "failed", "cleanup": {}}, "receipt": "offline.json", "json": json, "print": lambda _: None}
        main = source_functions(env, optimize=2)
        last_try = max(index for index, node in enumerate(main.body) if isinstance(node, ast.Try))
        tail = ast.fix_missing_locations(ast.Module(body=main.body[last_try + 1:], type_ignores=[]))
        with self.assertRaises(SystemExit):
            exec(compile(tail, str(SOURCE), "exec", optimize=2), env)

    def test_cleanup_failure_cannot_leave_passed_receipt(self):
        env = {"report": {"http_result": "passed", "log_result": "passed", "result": "passed", "cleanup": {}},
               "time": types.SimpleNamespace(monotonic=lambda: 0), "save": lambda: None,
               "run": "owned", "json": json}
        main = source_functions(env, optimize=2)
        nested = [node for node in main.body if isinstance(node, ast.FunctionDef) and node.name in ("cleanup", "owned")]
        exec(compile(ast.fix_missing_locations(ast.Module(body=nested, type_ignores=[])), str(SOURCE), "exec", optimize=2), env)

        def broken(*args):
            raise RuntimeError("delete failed")

        cleanup = next(node for node in nested if node.name == "cleanup")
        if cleanup.args.args[-1].arg == "absent":
            env["cleanup"]("function", lambda: True, broken, lambda: True)
        else:
            env["cleanup"]("function", lambda: {"tags": {"crucible-smoke": "owned"}}, broken)
        if "finalize_report" in env:
            env["finalize_report"](env["report"], {"function"})
        self.assertEqual(env["report"]["result"], "failed")

    def test_transient_delete_retries_with_fresh_ownership_and_bounded_failure(self):
        class ClientError(Exception):
            def __init__(self, code):
                super().__init__(code)
                self.response = {"Error": {"Code": code}}

        for scenario in ("transient", "network", "lost-response", "permanent", "foreign", "denied"):
            with self.subTest(scenario=scenario):
                state = {"now": 0, "exists": True, "attempts": 0, "reads": 0}
                env = {"report": {"cleanup": {}}, "save": lambda: None, "run": "owned",
                       "ClientError": ClientError,
                       "time": types.SimpleNamespace(monotonic=lambda: state["now"],
                           sleep=lambda seconds: state.__setitem__("now", state["now"] + seconds))}
                main = source_functions(env, optimize=2)
                nested = [node for node in main.body if isinstance(node, ast.FunctionDef) and node.name in ("cleanup", "owned")]
                exec(compile(ast.fix_missing_locations(ast.Module(body=nested, type_ignores=[])), str(SOURCE), "exec", optimize=2), env)

                def find():
                    state["reads"] += 1
                    if not state["exists"]:
                        return None
                    return {"tags": {"crucible-smoke": "foreign" if scenario == "foreign" and state["attempts"] else "owned"}}

                def delete(_):
                    state["attempts"] += 1
                    if scenario == "denied":
                        raise ClientError("AccessDeniedException")
                    if scenario == "lost-response":
                        state["exists"] = False
                        raise TimeoutError("delete response lost")
                    if scenario == "network" and state["attempts"] == 1:
                        raise env["HTTPClientError"]("connection closed")
                    if state["attempts"] == 1 or scenario == "permanent":
                        raise ClientError("ServiceUnavailableException")
                    state["exists"] = False

                env["cleanup"]("function", find, delete, settle_seconds=0)
                outcome = env["report"]["cleanup"]["function"]
                if scenario in ("transient", "network", "lost-response"):
                    self.assertEqual(outcome, "deleted-and-verified")
                    self.assertFalse(state["exists"])
                    self.assertGreaterEqual(state["reads"], 2)
                    self.assertEqual(state["attempts"], 1 if scenario == "lost-response" else 2)
                else:
                    self.assertIn("error", outcome)
                    self.assertTrue(state["exists"])
                    self.assertLessEqual(state["now"], 20)
                    if scenario == "permanent":
                        self.assertGreater(state["attempts"], 1)
                    else:
                        self.assertEqual(state["attempts"], 1)

    def test_lifecycle_outcome_matches_application_mode_and_cancel_timing(self):
        cases = [("normal", False, "Completed", 3000, True),
                 ("normal", False, "Failed", 3000, False),
                 ("zero", False, "Completed", 0, True),
                 ("zero", False, "Cancelled", 0, False),
                 ("late", False, "Failed", 1800, True),
                 ("late", False, "Completed", 1800, False),
                 ("hold", False, "Failed", 5000, True),
                 ("hold", True, "Cancelled", 1000, True),
                 ("hold", True, "Cancelled", 4500, True),
                 ("hold", True, "Failed", 5000, True),
                 ("hold", True, "Failed", 1000, False),
                 ("hold", True, "Completed", 1000, False)]
        for mode, cancel, outcome, elapsed, accepted in cases:
            with self.subTest(mode=mode, cancel=cancel, outcome=outcome, elapsed=elapsed):
                case = {"nonce": "offline", "mode": mode, "cancel": cancel}
                logs = [{"message": f"nonce=offline event=producer-drop elapsed_ms={elapsed}"},
                        {"message": f"nonce=offline event=stream-complete end={outcome}"}]
                env = {"report": {"logs": logs}, "streams": [case], "re": re}
                main = source_functions(env, optimize=2)
                loop = next(node for node in ast.walk(main) if isinstance(node, ast.For)
                            and isinstance(node.iter, ast.Name) and node.iter.id == "streams")
                code = compile(ast.fix_missing_locations(ast.Module(body=[loop], type_ignores=[])), str(SOURCE), "exec", optimize=2)
                if accepted:
                    exec(code, env)
                else:
                    with self.assertRaises(AssertionError):
                        exec(code, env)

    def test_lost_create_response_and_delayed_visibility_still_delete_owned_role(self):
        state, report = lost_role_response()
        self.assertIsNone(state["role"])
        self.assertEqual(state["deletes"], 1)
        self.assertGreaterEqual(state["role_reads"], 4)
        self.assertEqual(report["cleanup"]["role"], "deleted-and-verified")
        self.assertEqual(set(report["cleanup"]), {"function", "role", "log_group"})
        self.assertEqual(report["result"], "failed")  # The original create response was lost.

    def test_delayed_function_log_group_and_rest_api_recovery(self):
        for kind in ("function", "log_group", "rest_api"):
            with self.subTest(kind=kind):
                state, report = lost_role_response(kind=kind)
                self.assertTrue(all(state[name] is None for name in ("role", "function", "log_group", "rest_api")))
                self.assertGreaterEqual(state["reads"], 3)
                self.assertEqual(report["cleanup"][kind], "deleted-and-verified")
                self.assertFalse(state["url"])
                self.assertFalse(state["policy"])
                self.assertEqual(report["result"], "failed")

    def test_persistently_absent_recovery_records_every_intended_resource(self):
        state, report = lost_role_response(retain=False)
        self.assertGreaterEqual(state["now"], 35)
        self.assertEqual(report["cleanup"], {name: "absent-and-verified" for name in ("function", "role", "log_group")})

    def test_recovery_access_denied_and_foreign_tags_never_delete(self):
        for kwargs in ({"denied": True}, {"wrong_tag": True, "visible_after": 0}):
            with self.subTest(kwargs=kwargs):
                state, report = lost_role_response(**kwargs)
                self.assertEqual(state["deletes"], 0)
                self.assertIsNotNone(state["role"])
                self.assertIn("error", report["cleanup"].get("role", {}))
                self.assertEqual(report["result"], "failed")

    def test_pass_requires_http_logs_and_every_expected_cleanup_record(self):
        env = {}
        source_functions(env, optimize=2)
        self.assertIn("finalize_report", env)
        expected = {"role", "function", "log_group"}
        report = {"http_result": "passed", "log_result": "passed", "result": "pending-cleanup",
                  "cleanup": {name: "deleted-and-verified" for name in expected}}
        env["finalize_report"](report, expected)
        self.assertEqual(report["result"], "passed")
        report["cleanup"].pop("function")
        env["finalize_report"](report, expected)
        self.assertEqual(report["result"], "failed")
        report["cleanup"]["function"] = "absent-and-verified"
        report["log_result"] = "failed"
        env["finalize_report"](report, expected)
        self.assertEqual(report["result"], "failed")
        report["log_result"] = "passed"
        env["finalize_report"](report, expected)
        self.assertEqual(report["result"], "passed")

    def test_noncommercial_partition_and_identity_mismatch_reject_before_writes(self):
        for kwargs in ({"partition": "aws-cn"}, {"partition": "aws-us-gov"}, {"wrong_identity": True}):
            with self.subTest(kwargs=kwargs):
                state, report = lost_role_response(**kwargs)
                self.assertEqual(state["create_calls"], 0)
                self.assertIsNone(report)


class CloudBuildSafety(unittest.TestCase):
    def test_linux_build_preserves_invoking_uid_gid(self):
        builder = load_builder()
        commands = []
        fake_os = types.SimpleNamespace(getuid=lambda: 1234, getgid=lambda: 5678)
        with tempfile.TemporaryDirectory() as directory:
            env = builder.main.__globals__
            with patch.dict(env, os=fake_os, package=lambda *args: None), patch.object(builder.subprocess, "run", lambda command, **kwargs: commands.append(command)), patch.object(sys, "argv", [str(BUILDER), "--artifacts", directory]):
                builder.main()
        command = commands[0]
        self.assertIn("--user", command)
        self.assertEqual(command[command.index("--user") + 1], "1234:5678")

    def test_build_metadata_names_partial_inputs_explicitly(self):
        builder = load_builder()
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            artifacts, source = fake_inputs(Path(directory))
            builder.package(artifacts, source)
            record = json.loads((artifacts / "build.json").read_text())
        self.assertIn("partial_source_sha256", record)
        self.assertNotIn("source_sha256", record)
        self.assertEqual(len(record["partial_source_sha256"]), 3)

    def test_optimized_packaging_rejects_non_linux_binary(self):
        builder = load_builder(optimize=2)
        with tempfile.TemporaryDirectory() as directory:
            artifacts, source = fake_inputs(Path(directory), valid=False)
            with self.assertRaises(ValueError):
                builder.package(artifacts, source)


if __name__ == "__main__":
    unittest.main()

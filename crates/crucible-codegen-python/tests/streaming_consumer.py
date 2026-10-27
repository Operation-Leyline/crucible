"""Independent installed-wheel consumers over native TCP and Rust HTTP."""
import asyncio
import base64
import gc
import gzip
from contextlib import contextmanager
import json
import os
from pathlib import Path
import queue
import random
import socket
import socketserver
import subprocess
import threading
import time
import unittest
import zlib
from urllib.request import urlopen

import httpx
from streams_client import StreamsClient, AsyncStreamsClient, PublishInput, WorkFailed, HeaderFailure, PayloadFailure, DefaultReason, DefaultEmptyReason, NetworkError, TelemetryHook
from streams_client.stream import StreamProtocolError
import streams_client.stream as stream_module

MIME = "text/event-stream; charset=utf-8; crucible-stream=1"
ITEM = b'data: {"ready":"first"}\n\n'
COMPLETE = b'event: crucible.complete\ndata: {}\n\n'


def physical_frame_cases():
    for name, delimiter in (("lf", b"\n"), ("cr", b"\r"), ("crlf", b"\r\n")):
        for size in (256, 257):
            suffix = delimiter + b'data: {"ready":"first"}' + delimiter * 2
            frame = b"x:" + b"a" * (size - 2 - len(suffix)) + suffix
            assert len(frame) == size
            # A blank CR delivers immediately. Its optional LF can exceed the
            # previous frame's budget only on the following consumption call.
            items = int(size == 256 or name == "crlf")
            yield f"{name}-{size}", frame + COMPLETE, size == 256, items
    yield "ignored-crlf-65", b"x:\r\n" * 65 + ITEM + COMPLETE, False, 0
    for name, suffix in (("empty", b"\r\n\r\n"), ("terminal", b"\r\nevent: crucible.complete\r\ndata: {}\r\n\r\n")):
        frame = b"x:" + b"a" * (257 - 2 - len(suffix)) + suffix
        yield f"{name}-crlf-257", frame + (ITEM + COMPLETE if name == "empty" else b""), False, 0
    suffix = b'\r\ndata: {"ready":"first"}\r\n\r\n'
    frame = b"x:" + b"a" * (256 - 2 - len(suffix)) + suffix
    yield "consecutive-crlf-256", frame * 2 + COMPLETE, True, 2


@contextmanager
def raw(wire=ITEM + COMPLETE, *, hold=False, status=200, mime=MIME, bytewise=False, statuses=None,
        encoding="", error_body=None, hold_errors=False, truncate_http=False, split_crlf=False):
    requests = []
    disconnected = threading.Event()
    released = threading.Event()
    class Handler(socketserver.BaseRequestHandler):
        def handle(self):
            self.request.settimeout(5)
            data = b""
            while b"\r\n\r\n" not in data:
                data += self.request.recv(4096)
            header_end=data.index(b"\r\n\r\n")+4
            length=0
            for header in data[:header_end-4].split(b"\r\n")[1:]:
                name,_,value=header.partition(b":")
                if name.lower()==b"content-length": length=int(value.strip())
            # An unread POST body makes socket close reset Windows connections,
            # obscuring the typed opening error this fixture is meant to test.
            while len(data)-header_end<length:
                chunk=self.request.recv(min(4096,length-(len(data)-header_end)))
                if not chunk: return
                data+=chunk
            requests.append(data)
            code = statuses[min(len(requests)-1, len(statuses)-1)] if statuses else status
            body = wire if code == 200 else (error_body if error_body is not None else b'{"__type":"WorkFailed","message":"opening"}')
            content_encoding = f"Content-Encoding: {encoding}\r\n" if encoding and (code != 200 or statuses is None) else ""
            self.request.sendall(f"HTTP/1.1 {code} Test\r\nContent-Type: {mime}\r\n{content_encoding}x-request-id: profile\r\nX-Reason: header-spoof\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n".encode())
            try:
                chunks = (bytes([b]) for b in body) if bytewise else (body,)
                for chunk in chunks:
                    if chunk:
                        self.request.sendall(f"{len(chunk):x}\r\n".encode()+chunk+b"\r\n")
                        if split_crlf and chunk == b"\r": time.sleep(.001)
                if hold and (code == 200 or hold_errors):
                    while not released.is_set():
                        try:
                            if not self.request.recv(4096):
                                disconnected.set(); break
                        except socket.timeout:
                            continue
                elif not truncate_http:
                    self.request.sendall(b"0\r\n\r\n")
            except (OSError, ConnectionError):
                disconnected.set()
    class Server(socketserver.ThreadingTCPServer):
        allow_reuse_address = True
        daemon_threads = True
    server = Server(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}", requests, disconnected
    finally:
        released.set(); server.shutdown(); server.server_close(); thread.join()


@contextmanager
def rust(mode):
    binary = os.environ.get("CRUCIBLE_STREAM_FIXTURE")
    if not binary:
        raise RuntimeError("CRUCIBLE_STREAM_FIXTURE is required for installed native HTTP qualification")
    server = subprocess.Popen([binary, mode], stdout=subprocess.PIPE, text=True,
                              creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    try:
        # A fixture that hangs or exits before printing its endpoints must
        # fail this test, not block the whole qualification on readline().
        lines = queue.Queue()
        threading.Thread(target=lambda: lines.put(server.stdout.readline()), daemon=True).start()
        try:
            line = lines.get(timeout=30)
        except queue.Empty:
            raise RuntimeError(f"Rust fixture {mode!r} printed no endpoints within 30s") from None
        if not line:
            raise RuntimeError(f"Rust fixture {mode!r} exited before starting (status {server.poll()})")
        endpoint, control = line.split()
        yield endpoint, lambda: json.load(urlopen(control, timeout=2))
    finally:
        server.terminate(); server.wait(timeout=5); server.stdout.close()


def dropped(counts):
    deadline = time.monotonic() + 2
    while counts()[1] != 1 and time.monotonic() < deadline:
        time.sleep(.01)
    assert counts()[1] == 1, "producer must drop BEFORE fallback server cleanup"


class SyncStreams(unittest.TestCase):
    def test_physical_frame_bytes_contiguous_and_split_crlf(self):
        for split in (False, True):
            for name, wire, valid, expected_items in physical_frame_cases():
                with self.subTest(case=name, split=split), raw(wire, bytewise=split, split_crlf=split) as (url, requests, _), StreamsClient(url, max_stream_event_bytes=256) as client:
                    with client.watch() as stream:
                        items=[];error=None
                        try:
                            for value in stream: items.append(value)
                        except StreamProtocolError as caught: error=caught
                        self.assertEqual(error is None, valid)
                        self.assertEqual(len(items), expected_items)
                        self.assertTrue(stream._closed)
                        self.assertFalse(stream._thread.is_alive())
                        self.assertEqual(len(requests), 1)

    def test_factory_setup_uses_original_total_deadline_and_releases_worker(self):
        for options in ({},{"timeout":.2}):
            with self.subTest(call_options=options):
                transports=[];workers=[];credentials=[]
                def factory():
                    workers.append(threading.current_thread())
                    time.sleep(.6)
                    http=httpx.AsyncClient();transports.append(http);return http
                with raw() as (url,requests,_), StreamsClient(url,timeout=2 if options else .2,stream_http_factory=factory,token=lambda:credentials.append("called")) as client:
                    stream=None
                    try:
                        with self.assertRaises(TimeoutError): stream=client.watch(**options)
                        self.assertEqual(credentials,[])
                        self.assertEqual(requests,[])
                        self.assertEqual(len(transports),1)
                        self.assertTrue(transports[0].is_closed,"owned transport closes before operation failure returns")
                        self.assertEqual(len(workers),1)
                        self.assertFalse(workers[0].is_alive(),"worker joins before helper cleanup")
                    finally:
                        if stream is not None: stream.close()

    def test_precancel_skips_factory_and_factory_failure_joins_worker(self):
        calls=[];workers=[];credentials=[]
        def factory():
            calls.append("called");workers.append(threading.current_thread())
            raise RuntimeError("factory startup failed")
        with raw() as (url,requests,_), StreamsClient(url,stream_http_factory=factory,token=lambda:credentials.append("called")) as client:
            cancellation=threading.Event();cancellation.set()
            with self.assertRaises(asyncio.CancelledError): client.watch(cancellation=cancellation)
            self.assertEqual(calls,[],"pre-cancel must skip per-call transport creation")
            with self.assertRaisesRegex(RuntimeError,"factory startup failed"): client.watch()
            self.assertEqual(calls,["called"])
            self.assertFalse(workers[0].is_alive(),"failed startup joins worker before helper cleanup")
            self.assertEqual(credentials,[])
            self.assertEqual(requests,[])

    def test_deep_json_is_protocol_error_and_closes_before_manual_cleanup(self):
        wire=b'data: {"future":'+b'['*20000+b'0'+b']'*20000+b'}\n\n'+COMPLETE
        self.assertEqual(len(wire),40055)
        with raw(wire,hold=True) as (url,requests,disconnected), StreamsClient(url) as client, client.watch() as stream:
            with self.assertRaises(StreamProtocolError) as caught: next(stream)
            self.assertEqual(str(caught.exception),"malformed UTF-8 or JSON")
            self.assertEqual(len(requests),1,"JSON depth failure never replays successful headers")
            self.assertTrue(stream._closed)
            self.assertFalse(stream._thread.is_alive(),"error path joins worker before manual cleanup")
            self.assertTrue(disconnected.wait(1),"response lease closes before helper cleanup")

    def test_all_shared_vectors_bytewise_native_http(self):
        vectors = json.loads(Path(os.environ["CRUCIBLE_STREAM_VECTORS"]).read_text(encoding="utf-8"))
        self.assertEqual(len(vectors["cases"]), 25)
        for case in vectors["cases"]:
            with self.subTest(case=case["name"]):
                wire = bytes.fromhex(case["hex"]) if "hex" in case else case["wire"].encode()
                with raw(wire, bytewise=True) as (url, requests, _), StreamsClient(url, max_stream_event_bytes=case.get("limit", 1024*1024)) as client:
                    with client.watch() as stream:
                        items=[]
                        try:
                            items=list(stream)
                            self.assertTrue(case["valid"])
                        except StreamProtocolError:
                            self.assertFalse(case["valid"])
                    self.assertEqual(len(requests),1, "never replay successful headers")
                    if case["valid"]:
                        self.assertEqual(len(items),case["items"])
                    if case["name"] == "unknown-union":
                        self.assertEqual(items[0]._unknown,("future",{"x":"y"}))

    def test_held_lf_crlf_cr_flush_before_close(self):
        for delimiter in (b"\n",b"\r\n",b"\r"):
            with raw(b'data: {"ready":"first"}'+delimiter*2, hold=True) as (url, _, disconnected), StreamsClient(url, timeout=2) as client:
                stream=client.watch()
                self.assertEqual(next(stream).ready,"first")
                self.assertFalse(disconnected.is_set())
                stream.close()
                self.assertTrue(disconnected.wait(1),"close must release held body before fixture cleanup")

    def test_rust_get_post_unary_and_native_close(self):
        for post in (False, True):
            with rust("hold") as (url, counts), httpx.Client() as http, StreamsClient(url,http=http,stream_http_factory=httpx.AsyncClient) as client:
                stream=client.publish(PublishInput(topic="hold")) if post else client.watch()
                self.assertEqual(stream.metadata.status,200)
                self.assertEqual(next(stream).ready,"first")
                self.assertEqual(counts()[1],0)
                self.assertEqual(client.read().value,"unary")
                stream.close(); dropped(counts)
                self.assertEqual(client.read().value,"unary")
                self.assertFalse(http.is_closed)

    def test_drop_before_and_after_read_releases_owned_loop(self):
        for consume in (False,True):
            with rust("hold") as (url,counts), StreamsClient(url) as client:
                stream=client.watch()
                thread=stream._thread
                if consume: self.assertEqual(next(stream).ready,"first")
                del stream;gc.collect()
                dropped(counts)
                self.assertFalse(thread.is_alive(),"dropping sync stream must join its owned thread")

    def test_rust_cancel_before_consume_midread_idle_and_queued_item(self):
        for mode in ("before", "midread", "idle", "queued"):
            with self.subTest(mode=mode), rust("pending" if mode=="midread" else "hold") as (url, counts), StreamsClient(url) as client:
                cancellation=threading.Event()
                stream=client.watch(cancellation=cancellation)
                if mode=="idle":
                    stream.close(); dropped(counts); continue
                done=threading.Event(); caught=[]
                def consume():
                    try: next(stream)
                    except BaseException as error: caught.append(error)
                    finally: done.set()
                if mode=="midread":
                    thread=threading.Thread(target=consume); thread.start(); time.sleep(.05)
                elif mode=="queued":
                    time.sleep(.05)
                cancellation.set()
                if mode!="midread": consume()
                self.assertTrue(done.wait(1))
                self.assertIsInstance(caught[0],asyncio.CancelledError)
                dropped(counts)
                stream.close()
                if mode=="midread": thread.join()

    def test_deadline_idle_read_and_credentials(self):
        for mode in ("hold","pending"):
            with rust(mode) as (url,counts), StreamsClient(url,timeout=.15) as client:
                stream=client.watch()
                time.sleep(.2)
                dropped(counts)
                with self.assertRaises(TimeoutError): next(stream)
                stream.close()
        with raw() as (url,requests,_), StreamsClient(url,timeout=.05,token=lambda: (time.sleep(.08),"token")[1]) as client:
            with self.assertRaises(TimeoutError): client.watch()
            self.assertEqual(requests,[])

    def test_pre_cancel_and_buffered_mode_guard(self):
        event=threading.Event();event.set()
        with raw() as (url,requests,_), StreamsClient(url,token=lambda: self.fail("credentials must not execute")) as client:
            with self.assertRaises(asyncio.CancelledError): client.watch(cancellation=event)
            with self.assertRaises(ValueError): client._call("Watch")
            with self.assertRaises(ValueError): client._stream_call("Read")
            self.assertEqual(requests,[])

    def test_shared_caller_transport_is_not_closed(self):
        with raw() as (url,requests,_), httpx.Client() as http:
            with StreamsClient(url,http=http) as client:
                with self.assertRaises(ValueError): client.watch()
                self.assertEqual(requests,[])
            self.assertFalse(http.is_closed)
            with StreamsClient(url,http=http,stream_http_factory=httpx.AsyncClient) as client:
                with client.watch() as stream: self.assertEqual(len(list(stream)),1)
            self.assertFalse(http.is_closed)

    def test_opening_retries_auth_middleware_and_unsafe_post(self):
        for post, expected in ((False,2),(True,1)):
            with raw(statuses=[503,200]) as (url,requests,_), StreamsClient(url,token="token",base_delay=0,middleware=[lambda r:r.headers.update({"x-test":"middleware"})]) as client:
                if post:
                    with self.assertRaises(WorkFailed): client.publish(PublishInput(topic="send"))
                else:
                    with client.watch() as stream: self.assertEqual(len(list(stream)),1)
                self.assertEqual(len(requests),expected)
                self.assertTrue(all(b"authorization: bearer token" in r.lower() and b"x-test: middleware" in r.lower() for r in requests))

    def test_raw_midread_close_and_queued_item_cancellation(self):
        with raw(b"",hold=True) as (url,_,disconnected), StreamsClient(url) as client:
            stream=client.watch();done=threading.Event();caught=[]
            def consume():
                try: next(stream)
                except BaseException as error: caught.append(error)
                finally: done.set()
            thread=threading.Thread(target=consume);thread.start();time.sleep(.05)
            stream.close()
            self.assertTrue(done.wait(1));self.assertTrue(caught)
            self.assertTrue(disconnected.wait(1));thread.join()
        with raw(ITEM+ITEM,hold=True) as (url,_,disconnected), StreamsClient(url) as client:
            event=threading.Event();stream=client.watch(cancellation=event)
            self.assertEqual(next(stream).ready,"first");event.set()
            with self.assertRaises(asyncio.CancelledError): next(stream)
            self.assertTrue(disconnected.wait(1));stream.close()

    def test_decode_crossing_deadline_does_not_disclose(self):
        with raw() as (url,_,disconnected), StreamsClient(url,timeout=.1) as client:
            convert=client._codec.convert
            def slow(target,value,**options):
                result=convert(target,value,**options)
                if target and target.endswith("#Progress"): time.sleep(.15)
                return result
            client._codec.convert=slow
            with client.watch() as stream:
                with self.assertRaises(TimeoutError): next(stream)

    def test_failed_decode_crossing_lifetime_reports_lifetime(self):
        # The watchdog cannot run during synchronous parsing or decoding, so a
        # failure there must be rechecked against the deadline and cancellation.
        for path in ("item","json"):
            for lifetime in ("deadline","cancel"):
                with self.subTest(path=path,lifetime=lifetime), raw() as (url,_,_), StreamsClient(url,timeout=.1 if lifetime=="deadline" else 2) as client:
                    event=threading.Event()
                    def cross():
                        if lifetime=="deadline": time.sleep(.15)
                        else: event.set()
                    convert=client._codec.convert;parse=stream_module._json
                    def failing_convert(target,value,**options):
                        if target and target.endswith("#Progress"): cross();raise ValueError("invalid")
                        return convert(target,value,**options)
                    def failing_json(data):
                        cross();raise StreamProtocolError("malformed UTF-8 or JSON")
                    if path=="item": client._codec.convert=failing_convert
                    else: stream_module._json=failing_json
                    try:
                        with client.watch(cancellation=event) as stream:
                            with self.assertRaises(TimeoutError if lifetime=="deadline" else asyncio.CancelledError): next(stream)
                    finally:
                        stream_module._json=parse

    def test_interrupted_sync_opening_finishes_task_and_telemetry(self):
        finished=[];futures=[];accepted=[]
        class Hook(TelemetryHook):
            def operation_finished(self,event): finished.append(event)
        submit=stream_module.ResponseStream._submit
        with socket.create_server(("127.0.0.1",0)) as listener:
            listener.settimeout(5)
            def interrupted(stream,coroutine):
                if futures: return submit(stream,coroutine)
                futures.append(asyncio.run_coroutine_threadsafe(coroutine,stream._loop))
                # The opening is in flight (its request is connected) when the
                # caller is interrupted, as Ctrl-C would during result().
                accepted.append(listener.accept()[0])
                raise KeyboardInterrupt
            stream_module.ResponseStream._submit=interrupted
            try:
                with StreamsClient(f"http://127.0.0.1:{listener.getsockname()[1]}",telemetry=Hook()) as client:
                    with self.assertRaises(KeyboardInterrupt): client.watch()
            finally:
                stream_module.ResponseStream._submit=submit
                for connection in accepted: connection.close()
        self.assertTrue(futures[0].done(),"close cancels and awaits the pending opening")
        self.assertEqual(len(finished),1,"the interrupted opening still reports operation finished")

    def test_encoded_success_refused_and_closed_before_cleanup(self):
        with raw(gzip.compress(ITEM+COMPLETE),encoding="gzip",hold=True) as (url,requests,disconnected), StreamsClient(url) as client:
            with self.assertRaises(StreamProtocolError): client.watch()
            self.assertTrue(disconnected.wait(1))
            self.assertEqual(len(requests),1)
            self.assertIn(b"accept-encoding: identity",requests[0].lower())

    def test_interrupted_http_body_retains_network_error_and_never_replays(self):
        with raw(ITEM,truncate_http=True) as (url,requests,_), StreamsClient(url) as client, client.watch() as stream:
            self.assertEqual(next(stream).ready,"first")
            with self.assertRaises(NetworkError): next(stream)
            self.assertEqual(len(requests),1)

    def test_bounded_compressed_opening_errors_and_safe_retry(self):
        for encoding,compress in (("gzip",gzip.compress),("deflate",zlib.compress),("deflate",lambda b:zlib.compress(b)[2:-4])):
            small=compress(b'{"__type":"WorkFailed","message":"opening","reason":"broken"}')
            with raw(status=503,encoding=encoding,error_body=small) as (url,requests,_), StreamsClient(url,max_stream_event_bytes=256) as client:
                with self.assertRaises(WorkFailed) as caught: client.publish(PublishInput(topic="send"))
                self.assertEqual(caught.exception.message,"opening")
                self.assertEqual(str(caught.exception.reason),"broken")
                self.assertEqual(len(requests),1)
            large=compress(json.dumps({"__type":"WorkFailed","message":"x"*100000}).encode())
            with raw(statuses=[503,200],encoding=encoding,error_body=large) as (url,requests,_), StreamsClient(url,max_stream_event_bytes=256,base_delay=0) as client:
                with client.watch() as stream: self.assertEqual(len(list(stream)),1)
                self.assertEqual(len(requests),2)

    def test_concatenated_gzip_opening_error_members_share_the_ceiling(self):
        body=b'{"__type":"WorkFailed","message":"opening","reason":"broken"}'
        members=gzip.compress(body[:20])+gzip.compress(body[20:40])+gzip.compress(body[40:])
        # Bytes after the last member that do not start another one end the
        # body, as on the unary path (HTTPX) and in the gzip module.
        for name,wire in (("members",members),("nul-padding",gzip.compress(body)+b"\0"*8),("crlf",gzip.compress(body)+b"\r\n"),
                          ("members-crlf",members+b"\r\n")):
            with self.subTest(wire=name), raw(status=503,encoding="gzip",error_body=wire) as (url,requests,_), StreamsClient(url,max_stream_event_bytes=256) as client:
                with self.assertRaises(WorkFailed) as caught: client.publish(PublishInput(topic="send"))
                self.assertEqual(caught.exception.message,"opening")
                self.assertEqual(str(caught.exception.reason),"broken")
                self.assertEqual(len(requests),1)
        # Each member is under the ceiling, together they exceed it: the body
        # is oversized, so only the HTTP status classifies it (a safe retry).
        large=json.dumps({"__type":"WorkFailed","message":"x"*300}).encode()
        members=gzip.compress(large[:160])+gzip.compress(large[160:])
        with raw(statuses=[503,200],encoding="gzip",error_body=members) as (url,requests,_), StreamsClient(url,max_stream_event_bytes=256,base_delay=0) as client:
            with client.watch() as stream: self.assertEqual(len(list(stream)),1)
            self.assertEqual(len(requests),2)

    def test_large_gzip_members_decode_linearly(self):
        # Decoding is synchronous, so neither the watchdog nor the deadline can
        # interrupt it: 64 members carrying ~1.5 MiB must stay linear.
        limit=2*1024*1024
        message=base64.b64encode(random.Random(7).randbytes(1100000)).decode()
        body=json.dumps({"__type":"WorkFailed","message":message}).encode()
        size=-(-len(body)//64)
        wire=b"".join(gzip.compress(body[i:i+size],mtime=0) for i in range(0,len(body),size))
        self.assertLessEqual(len(wire),limit)
        with raw(status=503,encoding="gzip",error_body=wire) as (url,requests,_), StreamsClient(url,max_stream_event_bytes=limit,timeout=60) as client:
            start=time.monotonic()
            with self.assertRaises(WorkFailed) as caught: client.publish(PublishInput(topic="send"))
            self.assertLess(time.monotonic()-start,2.0,"gzip members must decode in linear time")
            self.assertEqual(caught.exception.message,message)
            self.assertEqual(len(requests),1)

    def test_excess_gzip_members_are_rejected_quickly(self):
        # 104857 empty members (2 MiB): quadratic before the member bound.
        limit=2*1024*1024
        members=gzip.compress(b"",mtime=0)*(limit//20)
        with raw(statuses=[503,200],encoding="gzip",error_body=members) as (url,requests,_), StreamsClient(url,max_stream_event_bytes=limit,base_delay=0,timeout=60) as client:
            start=time.monotonic()
            with client.watch() as stream: self.assertEqual(len(list(stream)),1)
            self.assertLess(time.monotonic()-start,2.0,"excess gzip members must be rejected without quadratic work")
            self.assertEqual(len(requests),2)
        # Beyond 64 members the body is malformed: only its status classifies it.
        body=b'{"__type":"WorkFailed","message":"opening","reason":"broken"}'
        for count,decoded in ((64,True),(65,False)):
            wire=b"".join(gzip.compress(body[i:i+1]) for i in range(count-1))+gzip.compress(body[count-1:])
            with self.subTest(members=count), raw(status=503,encoding="gzip",error_body=wire) as (url,requests,_), StreamsClient(url) as client:
                with self.assertRaises(Exception) as caught: client.publish(PublishInput(topic="send"))
                # Without the errorDocs base an undecoded 503 still maps to
                # WorkFailed by status, so the decoded message is what differs.
                self.assertEqual(getattr(caught.exception,"message",None)=="opening",decoded)
                self.assertEqual(len(requests),1)

    def test_opening_error_at_and_past_the_limit(self):
        # Exactly max_stream_event_bytes keeps the modelled fields in every
        # encoding; one byte more is oversized, so a valid prefix is not decoded.
        # A partly random message keeps the compressed body (~170 bytes) under
        # the limit but puts its gzip trailer past the first input slice, and
        # an empty member may follow a body already at the limit.
        rng=random.Random(3);message="".join(rng.choice("abcdefgh") for _ in range(200))
        document=json.dumps({"__type":"WorkFailed","message":message}).encode()
        encodings=(("identity","",lambda b:b),("gzip","gzip",gzip.compress),
                   ("gzip-empty-member","gzip",lambda b:gzip.compress(b)+gzip.compress(b"")),("deflate","deflate",zlib.compress))
        for name,encoding,compress in encodings:
            for size,decoded in ((256,True),(257,False)):
                body=document+b" "*(size-len(document))
                with self.subTest(encoding=name,size=size), raw(status=503,encoding=encoding,error_body=compress(body)) as (url,requests,_), StreamsClient(url,max_stream_event_bytes=256) as client:
                    with self.assertRaises(Exception) as caught: client.publish(PublishInput(topic="send"))
                    self.assertEqual(getattr(caught.exception,"message",None)==message,decoded)
                    self.assertEqual(len(requests),1)

    def test_opening_error_fixture_reads_separate_post_body_before_response(self):
        class DelayedBody(httpx.AsyncByteStream):
            def __init__(self,data): self.data=data
            async def __aiter__(self):
                await asyncio.sleep(.1)
                yield self.data
        def separate_body(request): request.stream=DelayedBody(request.content)
        error=gzip.compress(b'{"__type":"WorkFailed","message":"opening","reason":"broken"}')
        with raw(status=503,encoding="gzip",error_body=error) as (url,requests,_), StreamsClient(url,middleware=[separate_body]) as client:
            with self.assertRaises(WorkFailed) as caught: client.publish(PublishInput(topic="send"))
            self.assertEqual(caught.exception.message,"opening")
            self.assertEqual(len(requests),1)
            self.assertEqual(requests[0].partition(b"\r\n\r\n")[2],b'{"topic":"send"}')

    def test_compressed_opening_error_obeys_deadline_and_cancel(self):
        for timeout in (False,True):
            with raw(status=503,encoding="gzip",error_body=gzip.compress(b'{"message":"opening"}')[:12],hold=True,hold_errors=True) as (url,requests,disconnected), StreamsClient(url,timeout=.2 if timeout else 2) as client:
                event=threading.Event();done=threading.Event();caught=[]
                def opening():
                    try: client.watch(cancellation=event)
                    except BaseException as error:caught.append(error)
                    finally:done.set()
                thread=threading.Thread(target=opening);thread.start()
                deadline=time.monotonic()+1
                while not requests and time.monotonic()<deadline:time.sleep(.01)
                if not timeout:event.set()
                self.assertTrue(done.wait(1));thread.join()
                self.assertIsInstance(caught[0],TimeoutError if timeout else asyncio.CancelledError)
                self.assertTrue(disconnected.wait(1))

    def test_late_errors_document_fields_defaults_unknown_metadata(self):
        for name,fields,error in (("WorkFailed",{"message":"late","reason":"broken"},WorkFailed),
                                  ("HeaderFailure",{"message":"late","reason":"broken"},HeaderFailure),
                                  ("PayloadFailure",{"reason":"future"},PayloadFailure),
                                  ("DefaultReason",{"message":"late"},DefaultReason),
                                  ("DefaultEmptyReason",{"message":"late"},DefaultEmptyReason),
                                  ("FutureError",{"message":"future"},Exception)):
            body=ITEM+b"event: crucible.error\ndata: "+json.dumps({"type":name,"status":503,"error":fields}).encode()+b"\n\n"
            with raw(body) as (url,requests,_), StreamsClient(url) as client, client.watch() as stream:
                self.assertEqual(next(stream).ready,"first")
                with self.assertRaises(error) as caught: next(stream)
                self.assertEqual(caught.exception.metadata.status,503)
                self.assertEqual(caught.exception.metadata.request_id,"profile")
                self.assertEqual(caught.exception.name,name)
                if name in ("HeaderFailure","PayloadFailure"):
                    self.assertEqual(str(caught.exception.reason),fields["reason"])
                if name=="DefaultReason": self.assertEqual(str(caught.exception.reason),"broken")
                if name=="DefaultEmptyReason": self.assertEqual(str(caught.exception.reason),"")
                if name=="PayloadFailure": self.assertEqual(caught.exception.message,"HTTP 503")
                self.assertEqual(len(requests),1)


class AsyncStreams(unittest.IsolatedAsyncioTestCase):
    async def test_physical_frame_bytes_contiguous_and_split_crlf(self):
        for split in (False, True):
            for name, wire, valid, expected_items in physical_frame_cases():
                with self.subTest(case=name, split=split), raw(wire, bytewise=split, split_crlf=split) as (url, requests, _):
                    async with AsyncStreamsClient(url, max_stream_event_bytes=256) as client:
                        async with await client.watch() as stream:
                            items=[];error=None
                            try:
                                async for value in stream: items.append(value)
                            except StreamProtocolError as caught: error=caught
                            self.assertEqual(error is None, valid)
                            self.assertEqual(len(items), expected_items)
                            self.assertTrue(stream._closed)
                            self.assertEqual(len(requests), 1)

    async def test_deep_json_is_protocol_error_and_closes_before_manual_cleanup(self):
        wire=b'data: {"future":'+b'['*20000+b'0'+b']'*20000+b'}\n\n'+COMPLETE
        self.assertEqual(len(wire),40055)
        with raw(wire,hold=True) as (url,requests,disconnected):
            async with AsyncStreamsClient(url) as client:
                async with await client.watch() as stream:
                    with self.assertRaises(StreamProtocolError) as caught: await anext(stream)
                    self.assertEqual(str(caught.exception),"malformed UTF-8 or JSON")
                    self.assertEqual(len(requests),1,"JSON depth failure never replays successful headers")
                    self.assertTrue(stream._closed)
                    self.assertTrue(await asyncio.to_thread(disconnected.wait,1),"response lease closes before helper cleanup")

    async def test_raw_midread_cancel_idle_deadline_and_queued_item(self):
        for mode in ("midread","idle","deadline","queued"):
            with raw(ITEM+ITEM if mode=="queued" else b"",hold=True) as (url,_,disconnected):
                async with AsyncStreamsClient(url,timeout=.15 if mode=="deadline" else 2) as client:
                    event=asyncio.Event();stream=await client.watch(cancellation=event)
                    if mode=="queued": self.assertEqual((await anext(stream)).ready,"first")
                    if mode=="midread":
                        task=asyncio.create_task(anext(stream));await asyncio.sleep(.05);event.set()
                        with self.assertRaises(asyncio.CancelledError): await task
                    elif mode=="deadline":
                        await asyncio.sleep(.2)
                        with self.assertRaises(TimeoutError): await anext(stream)
                    else:
                        event.set()
                        with self.assertRaises(asyncio.CancelledError): await anext(stream)
                    self.assertTrue(await asyncio.to_thread(disconnected.wait,1),f"{mode}: release before fallback cleanup")
                    await stream.aclose()

    async def test_rust_stream_task_cancel_releases_body_and_caller_pool(self):
        with rust("pending") as (url,counts):
            async with httpx.AsyncClient() as http, AsyncStreamsClient(url,http=http) as client:
                stream=await client.watch()
                self.assertEqual((await client.read()).value,"unary")
                task=asyncio.create_task(anext(stream))
                await asyncio.sleep(.05); task.cancel()
                with self.assertRaises(asyncio.CancelledError): await task
                await asyncio.to_thread(dropped,counts)
                await stream.aclose()
                self.assertFalse(http.is_closed)
                self.assertEqual((await client.read()).value,"unary")

    async def test_async_event_cancellation_and_explicit_idle_close(self):
        for mode in ("event","close","timeout"):
            with rust("hold") as (url,counts):
                async with AsyncStreamsClient(url,timeout=.15 if mode=="timeout" else 2) as client:
                    event=asyncio.Event();stream=await client.watch(cancellation=event)
                    if mode=="event": event.set()
                    elif mode=="close": await stream.aclose()
                    else: await asyncio.sleep(.2)
                    await asyncio.to_thread(dropped,counts)
                    if mode!="close":
                        with self.assertRaises(asyncio.CancelledError if mode=="event" else TimeoutError): await anext(stream)
                    await stream.aclose()

    async def test_lifetime_interrupts_withdraw_their_task_cancellation(self):
        task=asyncio.current_task()
        for mode in ("deadline","event"):
            with self.subTest(mode=mode), raw(b"",hold=True) as (url,_,_):
                async with AsyncStreamsClient(url,timeout=.15 if mode=="deadline" else 2) as client:
                    event=asyncio.Event();stream=await client.watch(cancellation=event)
                    if mode=="deadline":
                        with self.assertRaises(TimeoutError): await anext(stream)
                    else:
                        asyncio.get_running_loop().call_later(.05,event.set)
                        with self.assertRaises(asyncio.CancelledError): await anext(stream)
                    # The lifetime's own cancel() must not stay pending on the
                    # caller's task once it has been reported as an error.
                    self.assertEqual(task.cancelling(),0)
                    await stream.aclose()
        with socket.create_server(("127.0.0.1",0)) as listener:
            async with AsyncStreamsClient(f"http://127.0.0.1:{listener.getsockname()[1]}",timeout=.15) as client:
                with self.assertRaises(TimeoutError): await client.watch()
            self.assertEqual(task.cancelling(),0)
        # A caller cancel in the same tick as the watchdog's still wins, as
        # with asyncio.timeout(): the reader ends cancelled, not timed out.
        interrupt=stream_module._Lifetime.interrupt
        def with_caller_cancel(lifetime,reader):
            reader.cancel("caller");interrupt(lifetime,reader)
        stream_module._Lifetime.interrupt=with_caller_cancel
        try:
            with raw(b"",hold=True) as (url,_,_):
                async with AsyncStreamsClient(url,timeout=.15) as client:
                    stream=await client.watch()
                    reader=asyncio.create_task(anext(stream))
                    with self.assertRaises(asyncio.CancelledError): await reader
                    self.assertTrue(reader.cancelled())
                    await stream.aclose()
        finally:
            stream_module._Lifetime.interrupt=interrupt
        async with asyncio.timeout(1):
            async with asyncio.TaskGroup() as group:
                child=group.create_task(asyncio.sleep(.01,"ok"))
        self.assertEqual(child.result(),"ok")

    async def test_cross_task_close_during_failed_read_cleanup(self):
        # The reader is already releasing the response after a protocol error;
        # a sibling aclose() then must neither cancel it nor change its error.
        task=asyncio.current_task()
        for point in ("finishing","closing-response"):
            with self.subTest(point=point), raw(b'data: {bad\n\n',hold=True) as (url,_,disconnected):
                async with AsyncStreamsClient(url) as client:
                    stream=await client.watch()
                    def reached():
                        return stream._closed if point=="finishing" else stream._lifetime._close_task is not None
                    async def closer():
                        while not reached(): await asyncio.sleep(0)
                        await stream.aclose()
                    sibling=asyncio.create_task(closer())
                    try:
                        with self.assertRaises(StreamProtocolError): await anext(stream)
                        await sibling
                        self.assertEqual(task.cancelling(),0)
                    finally:
                        while task.cancelling(): task.uncancel()
                    self.assertTrue(await asyncio.to_thread(disconnected.wait,1))

    async def test_async_post_and_late_error(self):
        with rust("hold") as (url,counts), self.assertRaises(WorkFailed):
            async with AsyncStreamsClient(url) as client:
                async with await client.publish(PublishInput(topic="typed")) as stream:
                    self.assertEqual((await anext(stream)).ready,"first")
                    await anext(stream)


if __name__=="__main__": unittest.main()

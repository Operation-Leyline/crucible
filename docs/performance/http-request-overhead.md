# HTTP request overhead measurements

This change removes the HTTP adapter's boxed request future, retains static
request/version header names, and avoids duplicate header lookups. Public
interfaces, validation rules, protocol negotiation and context scoping stay
the same. The keep-alive integration test covers suspended successful and
rejected requests, peer metadata and distinct request IDs on one connection.

## Work per request

Callgrind profiles of the Item API show approximately three fewer calls to
`malloc`/`calloc`/`realloc` per request. Exclusive instruction counts fell by
6.9% for GET, 8.0% for PUT, 2.6% for list, 8.0% for create retry, 6.7% for
not found and 5.6% for invalid input. These are instruction counts, not time
samples. Linux perf access was unavailable. Small monitor wakeup requests
around counter reset/dump introduce fractional overhead in normalized counts.

## Native throughput and latency

Measurements use `Operation-Leyline/crucible-load-test` at
`c06ee4aa96cc37bee676254fe53b99bf31312c93`, with the same shared in-memory
store and its plain Axum baseline. The Crucible binary includes this runtime
change atop `63597f380ae6840131212c496d762f38e4b68d58`. Both binaries use
release optimizations, thin LTO, one codegen unit and line information.
Toolchain: Rust 1.90.0; load generator: oha 1.16.0.

The host exposes five CPUs on an AMD EPYC 9V74. Servers use CPUs 0–1 and the
load generator CPUs 2–4. Each scenario/implementation starts a fresh process,
seeds 100 items, warms for two seconds and measures 40,000 requests/sec plus
an unthrottled peak. There are three rounds of ten-second steps, 128
connections and alternating implementation order. Shared-rate latency uses
oha's latency correction. This is not a full saturation ramp.

Median results (C = Crucible, A = Axum):

| Scenario | C peak req/s | A peak req/s | C / A | Peak CPU µs/req C / A | p99 ms at 40k C / A |
|---|---:|---:|---:|---:|---:|
| GET | 151,505 | 174,507 | 0.87× | 13.1 / 11.3 | 1.34 / 1.18 |
| PUT | 145,759 | 170,103 | 0.86× | 13.7 / 11.7 | 1.03 / 1.37 |
| List | 68,707 | 72,018 | 0.95× | 27.3 / 25.6 | 5.40 / 5.54 |
| Create retry | 158,767 | 186,356 | 0.85× | 12.5 / 10.6 | 1.33 / 1.13 |
| Not found | 142,023 | 141,176 | 1.01× | 14.0 / 13.8 | 1.12 / 1.08 |
| Invalid input | 138,390 | 137,444 | 1.01× | 14.3 / 14.4 | 1.20 / 1.37 |

All 36 shared-rate steps sustained 40k requests/sec. Across 72 measured
steps and 65,048,213 requests there were no unexpected statuses or request
errors; the harness's 17-request wire-parity sequence also passed.
[Raw per-round measurements](http-request-overhead.csv) retain the variation;
latency columns are milliseconds and `cpu_us` is server user+system CPU time
per request. Normal load-generator deadline cancellation is excluded from
request errors by the runner.

An earlier paired original/modified comparison on one server core measured
median throughput changes of +15.0% GET, +7.1% PUT, +2.8% list, +11.5% create
retry, +15.5% not found and -0.3% invalid input (two rounds, five-second
steps). The paired two-core comparison was mixed, ranging from -5.2% to
+12.5%, with substantial round-to-round variation. These short shared-host
runs do not establish statistical significance, a universal throughput gain
or Axum parity across all operations and CPU topologies.

To reproduce the latest comparison, build the load-test workspace with its
runtime and wire Git dependencies patched to this local Crucible checkout,
preserving its thin-LTO release profile. Keep its pinned lockfile unchanged
in the submitted change. Then run:

```sh
python bench/run.py --no-build --scenarios get,put,list,create-retry,not-found,invalid \
  --rates 40000 --duration 10 --warmup 2 --rounds 3 --connections 128 --out results/retest
```

The current suite covers six Item scenarios, not every API pattern. Further
regex-cache and percent-decoding experiments were discarded because their
service-level benefit was not repeatable or their fast paths regressed other
inputs. They are not part of this change.

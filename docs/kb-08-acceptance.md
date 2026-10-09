# KB-08 acceptance record

Date: 2026-10-09. Closes KB-08 (documentation and qualification) for the
**macOS beta scope** defined in [deployments.md](deployments.md). KB-08 changed
documentation only; the Lua plugin, the vendored modules and the Rust service
are the bytes reviewed in KB-07. Two defects uncovered by the qualification are
recorded against KB-07, their owning feature, and are not fixed here.

## Revisions and hashes

| Item | Value |
| --- | --- |
| mtpnxk code revision qualified | `2de832e` (KB-07 review, second pass); the KB-08 commits on top of it touch `docs/`, `KEYBOARD.md`, `README.md`, `service/README.md` and `tools/ma3/VENDOR.md` only |
| `tools/ma3/mtpnxk_surface.lua` 0.1.0 | sha256 `ba4819e760be8f9199fc209cbba8a8a67dc8e8e6e92ea26705dde481e2b46515` |
| `tools/ma3/mtpnxk_surface.xml` | sha256 `b3f2b7e35abff1e1e41e917b87ab294e373a8828108754aa6aaf6b6e6872d1dd` |
| `tools/ma3/gma3_mcp_hardkeys.lua` 0.5.0 (API 1) | sha256 `d73a8e10a53260275b9bf45b9182cb5601b48843646ec1812376ff1095f6266e`, identical to `plugin/gma3_mcp_hardkeys.lua` at GrandMA3MCP `2d226dd` (PR #12, merged into `main` as `6e0d9c1`) |
| `tools/ma3/gma3_mcp_feedback.lua` 0.2.0 (API 1) | sha256 `349bb2ed1cc88bdbaf197aa20e957811df44306133c04652663da01a585d2cf9`, identical to upstream (`c18559a` and `6e0d9c1`) |
| Installed copies on the test console | byte-identical to the three files above (hashes checked on 2026-10-09) |
| Service | `service/` 0.1.0 at `2de832e`, Rust 1.97.1, `nusb` 0.2, release build |
| Protocol | surface protocol 1, SipHash-2-4 MAC, lease 2000 ms, heartbeat 250 ms, retransmit 4 × 60 ms |

## Environments

| Role | Environment |
| --- | --- |
| Live console | grandMA3 onPC 2.5.1.0 Release on macOS 26.5.1 (`bdsmbpm401`), show `mcp-test-disposable`, user Admin, profile Default, US layout, one display; MCP bridge 0.8.0 running as the read-only instrument (Lua enabled); NX-K not attached (sim and bench only) |
| Historical hardware evidence | the same console on the same day at `2de832e`'s predecessors with the NX-K attached: [probes/kb-07-live-macos-2.5.1.md](probes/kb-07-live-macos-2.5.1.md) |
| Automated | stock Lua 5.5.1, Rust 1.97.1, Python 3.9.6 (relay), macOS 26.5.1 |

## Commands and results

| # | Command | Result | Record |
| --- | --- | --- | --- |
| A1 | `lua tools/ma3/test/surface_plugin_test.lua` | 107 passed, 0 failed | — |
| A2 | `cd service && cargo test --release` | 19 passed, 0 failed | — |
| A3 | `sh tools/ma3/test/e2e.sh` (real service, real plugin under stock Lua with a stubbed console, real UDP relay) | `E2E PASSED` | [probe §4](probes/kb-08-qualification-macos-2.5.1.md) |
| L1 | `mtpnxk bench --taps 400 --rate 10` (plugin `bench`) | ack round trip 33.5 / 50.2 / 53.3 / 64.3 ms (n=798), press-to-effect 51 / 105 / 156 / 158 ms (n=359), 0 lost | probe §1 |
| L2 | `bench --taps 200 --rate 5`, `--taps 100 --rate 2`, `--taps 60 --rate 1` with an independent `cmdtext` poller | 0 lost at every rate; every tap landed on the command line; the plugin's instrument misses its own observation for ~1–7 % of taps (defect D1) | probe §1 |
| L3 | `sim --script "Record:down@800"` (bye) | hold released on `bye` | probe §2 L1 |
| L4 | `sim … --abandon` | hold released by the lease 1.99 s after the last packet | probe §2 L2 |
| L5 | plugin `stop` / start while the service held a key | stop released the key, no record; link down in 1.5 s with Link LED blink; re-paired without restart; nothing replayed | probe §2 L3 |
| L6 | `Login Guest` / `Login Admin` while paired | feedback invalidated within 0.93 s, epochs 2 and 3, service state replaced | probe §2 L4 |
| L7 | `Highlight On/Off` × 8 | HighLight LED write 84–194 ms after the command | probe §2 L5 |
| L8 | `status`, `recover` on an idle plugin | both answer; `recover: 0 released, 0 still unresolved, 0 record(s) not adopted` | probe §2 L6 |
| L9 | `bind=0.0.0.0 allow=<ip>` | `127.0.0.1` refused (`notAllowed`), the allowed address paired | probe §2 L7 |
| F1 | 2000 unauthenticated datagrams/s for 10 s from the service's own address | service locked out for the throttle window; **limit not met** (defect D2) | probe §3 |
| F2 | same flood from a second address | legitimate packets delayed behind the flood, 98 events lost; **limit not met** (defect D2) | probe §3 |

Regression coverage is cross-referenced, not duplicated: the harness list in
[KEYBOARD.md](../KEYBOARD.md) (107 checks) and the service tests (19) cover
disconnect and expiry cleanup, control-call Cleanup, conflicting consumers,
reordered and partial press/release pairs, resource ownership and kept records;
the saved-show module-loading path is the MCP repository's KB-02 evidence.

## Verdict against the section 8 limits

| Limit | Result |
| --- | --- |
| Press latency median ≤ 60 ms at 10 taps/s | met (51 ms) |
| Press latency p99 ≤ 120 ms, worst ≤ 250 ms at 10 taps/s | **not demonstrated**: measured p99 156 ms (n=359). The tail is attributed to the bench instrument (D1) with supporting evidence (band analysis, 1 tap/s run with p99 53 ms, independent poller showing no lost key), but a measured 156 ms is not reported as a pass |
| Ack round trip median ≤ 50 ms, p99 ≤ 100 ms | met (33.5 / 53.3 ms) |
| Lost-connection cleanup ≤ lease + 1 frame | met (1.99 s, one sample at 50 ms resolution) |
| LED freshness median ≤ 250 ms, p99 ≤ 500 ms | met (median ≈ 126 ms, worst 194 ms, n=8, upper bounds) |
| Stale indication ≤ 1500 ms | met (service watchdog at 1.5 s; Link LED write observed; physical LED observed in KB-07) |
| Flood resilience (2000 pps for 10 s) | **not met** in both variants (D2) |

## Defects filed against KB-07 (not fixed in KB-08)

- **D1, bench instrument.** `mtpnxk_surface.lua` takes the `before` snapshot of
  `cmdtext` after `hardkeys:press()` returns, so a key the console applies within
  that call is never seen by the watch; the sample then inherits the next tap's
  change (or is reported as no effect at ≤ 1 tap/s). Fix in the plugin, rerun the
  10 taps/s bench with ≥ 360 samples, and report p99 again. Until then the p99
  limit is open.
- **D2, flood handling.** (a) The authentication-failure throttle is per source
  address, so a flood from the service's own address locks the service out for
  10 s per window; on loopback any local process can do this. (b) The loop reads
  at most 32 datagrams per iteration and still has to read and discard datagrams
  from an ignored address, so at 2000 pps the kernel queue delays the legitimate
  packets past the retransmit budget. Candidates: drop ignored sources with a
  larger read budget per iteration (bounded by time, not count), rate the flood
  limit per deployment (loopback vs LAN), or revise the limit. Decide in KB-07.

## Remaining exclusions

Unqualified: separate-machine LAN deployment, Windows (WinUSB), Linux hosts, a
second display, non-US layouts, other onPC versions, physical consoles, a profile
switch and a key held across a user switch, show save/reload with the plugin
running, M-Touch/M-Play, encoders. The NX-K was not attached for the KB-08 run;
hardware LED evidence is the KB-07 record. The exception and quarantine recovery
paths are harness-only. These are stated in [deployments.md](deployments.md) and
must stay stated until a record exists.

## Decision

KB-08 **closes** with a bounded macOS beta scope: the documentation now matches
the implementation, the lifecycle behaviour has been observed on the console,
and the two measured shortfalls are recorded as open KB-07 defects rather than
presented as passes. The first beta release can cite this record together with
[deployments.md](deployments.md) and [operator-guide.md](operator-guide.md).

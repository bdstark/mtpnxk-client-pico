# KB-08 qualification record: onPC 2.5.1.0, macOS, 2026-10-09

Live qualification of the surface integration at the revision reviewed in KB-07
(code commit `2de832e`; no Lua or Rust source changed for KB-08). It reruns the
latency bench with larger samples, exercises the lifecycle paths on a disposable
show, and measures the flood case. The KB-07 record
([kb-07-live-macos-2.5.1.md](kb-07-live-macos-2.5.1.md)) stays as the historical
evidence for the first run, the hands-on NX-K checks and the two design changes it
caused. Every number below was produced on this day by the commands given;
observations through the console are labelled as such, and the paths that stay
harness-only are listed at the end.

## Environment

| Item | Value |
| --- | --- |
| Host | macOS 26.5.1 (Darwin 25.5.0), hostname `bdsmbpm401`, one physical display, US keyboard layout |
| Console | grandMA3 onPC 2.5.1.0 Release, show `mcp-test-disposable`, user Admin, profile `Default` (the only profile in the show), Lua 5.5 inside onPC |
| Surface plugin | `mtpnxk_surface` 0.1.0, protocol 1, installed copies identical to the repository (`mtpnxk_surface.lua` `ba4819e7…`, `gma3_mcp_hardkeys.lua` 0.5.0 `d73a8e10…`, `gma3_mcp_feedback.lua` 0.2.0 `349bb2ed…`); started from the MCP bridge with `key=<fresh 64 hex> input=keyboard bench force` |
| Why `force` | the MCP bridge 0.8.0 was running inside onPC with Lua enabled and its own input enabled (keyboard backend, no sessions, its own hardkeys copy 0.4.0); the courtesy check refuses without `force`. The bridge was the read-only instrument for every console-side observation (`lua` op reading `_G.__mtpnxk_surface`, `CmdObj().cmdtext`, `CurrentUser()`; `cmd` op for `Plugin "mtpnxk_surface" …`, `Login`, `Highlight`) |
| Service | `service/` at `2de832e`, release build, Rust 1.97.1, nusb 0.2; `sim` and `bench` only |
| NX-K | **not attached** during this run. LED behaviour is observed as the service's LED writes (`sim` echoes them); the physical LEDs were last observed by the operator in the KB-07 record |
| Polling instrument | a Node script holding one bridge connection and evaluating a Lua expression every 50–100 ms (hold count of the plugin's hardkeys instance, session count, last log line, `cmdtext`); timestamps are the poller's, so every "observed at" below is late by up to one poll interval plus one console frame |

Command line: `./service/target/release/mtpnxk --key-file <file> --id <id> …`
against the plugin at `127.0.0.1:9810`. Logs quoted below are the plugin's
(`mtpnxk: …`, read through the bridge) or the service's (`link: …`, `led …`).

## 1. Latency bench, final revision

`mtpnxk bench --taps N --rate R` (taps of `5`, a `Clear` every tenth tap, each
tap a press and a release 60 ms apart). Ack round trip is measured at the service
from the first transmission of an event to its acknowledgment. Press-to-effect is
reported by the plugin's `bench` mode: the time from its dispatch of a `NUM` press
to the first loop iteration at which `CmdObj().cmdtext` differs from the snapshot
it took, in the plugin's own clock (the service-to-plugin transit is not included).
Every run: zero events lost, zero refused, nothing superseded.

| taps/s | taps | events | retransmitted | ack round trip median / p95 / p99 / worst (ms), n | press-to-effect median / p95 / p99 / worst (ms), n | taps reported without effect | plugin loop |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 10 | 400 | 798 | 2 | 33.5 / 50.2 / 53.3 / 64.3, n=798 | **51 / 105 / 156 / 158**, n=359 | 0 | 24–28 Hz during the run (KB-07 value; not re-counted) |
| 5 | 200 | 400 | 0 | 33.4 / 50.3 / 53.0 / 58.1, n=400 | 36 / 240 / 253 / 257, n=180 | 0 | — |
| 2 | 100 | 200 | 1 | 34.5 / 50.4 / 54.1 / 61.9, n=200 | 37 / 541 / 550 / 552, n=90 | 0 | — |
| 1 | 60 | 120 | 0 | 34.3 / 50.7 / 51.6 / 52.1, n=120 | 36 / 53 / 53 / 53, n=52 | 2 | — |

Against the section 8 limits of [surface-protocol.md](../surface-protocol.md):

- Ack round trip median ≤ 50 ms and p99 ≤ 100 ms: **met** at every rate.
- Press-to-effect at 10 taps/s: median ≤ 60 ms **met** (51 ms); p99 ≤ 120 ms
  **not met as measured** (156 ms, n=359); worst ≤ 250 ms met (158 ms). The
  KB-07 table had carried the 40-tap p95 (67 ms) in the p99 column; the 40-tap
  p99 was 183 ms, which did not pass either.

### Where the tail comes from

The tail is not uniform. At 10 taps/s, 322 of 359 samples are ≤ 70 ms (one or two
console frames), 12 are 71–99 ms, and 25 are ≥ 100 ms. Of those 25, 15 lie in
133–158 ms, which is the 100 ms tap interval plus one or two frames; at 5 taps/s
all 13 samples over 70 ms lie in 228–257 ms (interval 200 ms), and at 2 taps/s all
5 lie in 541–552 ms (interval 500 ms). Every run also has samples of exactly 0 ms
(17, 8, 4 and 2). The slow samples follow the tap interval, not the console.

The 1 tap/s run separates the cases: with an interval longer than the plugin's
500 ms watch window, the same taps are reported as "no command-line change within
500 ms" (2 of 54 `NUM` taps) instead of inheriting the next tap's change, and the
remaining 52 samples have p99 = 53 ms. During that run an independent poller read
`cmdtext` through the bridge every 100 ms (670 reads): every one of the six
nine-tap groups reached exactly `555555555` and every `Clear` emptied the line.
No key press was lost at the console; the plugin's instrument missed its own
observation for those taps.

Cause, from reading the plugin (not changed here): `bench` mode takes its
`before` snapshot of `cmdtext` *after* `hardkeys:press()` returns. When the console
applies the injected key within that call, the snapshot already contains the
effect and the watch cannot see a change until something else alters the line
(the next tap, or never). The 0 ms samples are the mirror case (the previous tap's
change landing in the same iteration). This is a measurement defect of the KB-07
bench instrument and is filed against KB-07 in [KEYBOARD.md](../../KEYBOARD.md).
Until the instrument is fixed and the bench rerun, the p99 limit at 10 taps/s is
**not demonstrated**; the 1 tap/s figures and the band analysis are supporting
evidence, not a pass.

## 2. Lifecycle, observed on the console

Poller at 50 ms unless stated. "active" is the number of holds of the plugin's
hardkeys instance whose state is not `released`.

| # | Scenario | Procedure | Observed | Verdict |
| --- | --- | --- | --- | --- |
| L1 | Service stops cleanly while holding a key | `sim --script "Record:down@800"` (exits with `bye` while Record is down) | active 1 → 0 in the poll after the service exit (service exit 18:15:57.123, poll 18:15:57.131: `session … closed: bye`, active 0); no unresolved record | releases on `bye`, nothing kept |
| L2 | Service disappears (cable pull) while holding a key | `sim --script "Record:down@800" --seconds 1.5 --abandon` (no `bye`) | last packet at service exit 18:16:02.835; `lease expired, its holds were released; nothing is re-pressed` and active 0 observed at 18:16:04.827: **1.99 s** after the last packet (lease 2000 ms; 50 ms poll resolution); session kept for revival, closed `silent` 10 s later | limit "≤ lease + 1 frame, p99 ≤ 2100 ms": met for this single sample |
| L3 | Plugin stopped and restarted while the service keeps a key physically down | `sim --script "Record:down@800,Record:up@11000,Clear:tap@500" --seconds 14 --verbose`; `Plugin "mtpnxk_surface" "stop"` at +3 s, start at +7 s | stop: `stop requested` → `stopped` 37 ms later with active 0 and no unresolved record (the hold was released by the stop). Service: `link down: no plugin packet for 1.5 s` with `led Link <- blink`; `led Record <- on` from the local fallback while unpaired; `no plugin packet for 4.0 s: pairing again`; after the restart `session … opened … 1 key(s) physically down, not pressed`, active stayed 0 (no replay), `led Record <- off` once console state was fresh again; the physical release at 11 s was acknowledged without dispatch and the final `Clear` tap worked: `events=4 acked=4 lost=0 link_downs=1` | stale indication within 1.5 s, re-pairing without a service restart, **no replay** |
| L4 | Console user changed while paired | `Login Guest` then `Login Admin` through the bridge (`sim` idle and paired, `--verbose`) | plugin: `feedback invalidated: user-changed (epoch 2); the surface sees every item unknown until the next full state` 0.93 s after the command (identity is checked at most once per second), epoch 3 after the switch back; service: `plugin generation/epoch changed (7-c125b295/1 -> 7-c125b295/2): state replaced` and again for epoch 3; session kept, no event lost | identity change invalidates feedback end to end. A key held across the switch was not tested |
| L5 | Console state change → LED (feedback freshness) | `Highlight On` / `Highlight Off` four times each through the bridge while `sim` echoed LED writes with timestamps | `led HighLight <- blink` / `off` 84–194 ms after the command was *sent* to the bridge (8 samples: 126, 110, 158, 142, 126, 194, 84, 112 ms; the bridge CLI's own start-up is inside these numbers) | limit "median ≤ 250 ms, p99 ≤ 500 ms": met (median ≈ 126 ms, worst 194 ms, n=8) |
| L6 | Operator commands on an idle plugin | `Plugin "mtpnxk_surface" "status"` and `"recover"` | `status`: `running=true bind=127.0.0.1:9810 input=keyboard sessions=0 gen=10-634832b0`, counters; `recover`: `recover: 0 released, 0 still unresolved, 0 record(s) not adopted` | commands work with nothing to do; the paths with records stay harness-tested (below) |
| L7 | Allow list on a LAN binding | restart with `bind=0.0.0.0 allow=192.168.5.59` | service from `127.0.0.1`: never paired, plugin counter `notAllowed=1`; service to `192.168.5.59:9810` (source address `192.168.5.59`): paired, `Clear` tap acknowledged (`events=2 acked=2`) | the allow list refuses a source that holds the key; this is the one machine's two addresses, not a LAN deployment |

The command line was empty after every scenario; Highlight was left off; the
plugin was restarted on its default binding (`127.0.0.1:9810`) afterwards; plugin
slot and macros 110/111 remain in the disposable show.

## 3. Flood at the console

Unauthenticated datagrams (`MTX1`, a junk 16-hex MAC, a well-formed `key` body)
at 2000 per second for 10 s, 20 000 sent, while `bench --taps 100 --rate 10`
ran. Plugin loop rate from the `ticks` counter read once per second.

| Variant | Flood source | Service source | Plugin | Service | Verdict against "ack p99 ≤ 150 ms; no hold outlives its release by more than one frame" |
| --- | --- | --- | --- | --- | --- |
| F1 | `127.0.0.1` | `127.0.0.1` | 50 MAC failures, then `ignoring 127.0.0.1 for 10000 ms after repeated authentication failures` (twice); 17 593 datagrams counted as ignored; loop **55 Hz** throughout | never paired for the whole bench: `hellos=6 paired=false`, 200 events dropped unpaired; paired again after the ignore window | **not met**: the per-address throttle shares the service's address, so a flood from the same host locks the service out for the throttle window |
| F2 | `192.168.5.59` (the host's LAN address; plugin on `bind=0.0.0.0`) | `127.0.0.1` | flood address ignored after 50 failures, but ~1100–1300 of its datagrams per second still had to be read and discarded; loop fell to **36–42 Hz**; `Clear released by heartbeat reconciliation (its release event never arrived)` | `events=195 acked=341 lost=98 retransmitted=392 superseded=97 refused=16`; every ack arrived after the retransmit budget (no round-trip sample recorded); 51 of 90 `NUM` taps had a measured effect (median 50 ms, worst 253 ms) | **not met**: at most 32 datagrams are read per iteration, below the ~50 per frame arriving, so the socket buffer queues the legitimate packets behind the flood until the kernel drops them |

Both variants are measured failures of a limit chosen in KB-07 and are filed
against KB-07 (the throttle and the per-iteration read budget are plugin
design parameters). The loop itself never stalled and no hold outlived the
heartbeat reconciliation: the lease and heartbeat rules held; the latency and
pairing rules did not.

## 4. Automated evidence at the same revision

| Suite | Command | Result |
| --- | --- | --- |
| Plugin harness (stock Lua 5.5.1, stubbed console and socket) | `lua tools/ma3/test/surface_plugin_test.lua` | 107 passed, 0 failed |
| Service unit tests | `cd service && cargo test --release` | 19 passed |
| Real service against the real plugin under stock Lua behind a UDP relay (no console) | `sh tools/ma3/test/e2e.sh` | `E2E PASSED`: pairing, a retransmission with zero losses, two refused keys, an abandoned session released by the lease, 100 taps at 20/s with ack round trip 7.7 / 18.8 / 20.3 / 21.8 ms (n=166; 34 presses superseded by their releases through the relay's 16 ms frame; the synthetic press-to-effect of the stub says nothing about the console) |

The exception and quarantine paths (`service()` raising, `dispose()` raising,
records kept and adopted, `recover` exporting a quarantined instance, rejected
adoptions) are covered by the harness only; they were not provoked on the
console.

## 5. Not covered in this run

- The NX-K itself (not attached): no LED re-observed on hardware, `run` not
  exercised. The KB-07 record holds the operator's LED confirmations at the
  previous revision (the LED renderer did not change since).
- Show save and reload: not issued. `SaveShow`/`LoadShow` through the bridge is
  documented as unsafe in the MCP repository (a dialog on the plugin thread; the
  bridge itself dies on load), and no console-side way to restart the plugins
  without an operator existed in this run. KB-02 (MCP repository) recorded that
  Lua state survives `LoadShow` while plugin chunks re-run; the surface plugin's
  kept-records path across a reload is harness-tested only.
- Profile switch: the show has one profile.
- A key held across a user switch; Windows; Linux; a second display; non-US
  layouts; a service on a separate machine; a physical grandMA3 console;
  M-Touch and M-Play; encoders (unsupported by design).

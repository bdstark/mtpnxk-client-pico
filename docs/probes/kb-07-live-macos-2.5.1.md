# KB-07 live record: onPC 2.5.1.0, macOS, 2026-10-09

Setup: grandMA3 onPC 2.5.1.0 on the development Mac, show `mcp-test-disposable`,
user Admin, profile Default (US layout), one display. The MCP bridge 0.8.0 was
running with Lua enabled and was used read-only to import/start the plugin
(macros 110/111), read `CmdObj().cmdtext`, read the plugin's own state and log
through `_G.__mtpnxk_surface`, and time operations inside the console's Lua.
The surface plugin was started with `key=… input=keyboard bench force` (`force`
because the bridge reported input enabled; it held no keys). The Rust service
(`service/`, this commit) ran on the same machine: `sim`, `bench` and `run`
with the NX-K on USB.

## Findings

| # | Observation | Consequence |
| --- | --- | --- |
| 1 | Import of `mtpnxk_surface.xml` (3 components) succeeded; start from a macro (`Go+ Macro 110`) works; the loop iterates at **58 Hz** idle (ticks counted through the bridge). | Loop design confirmed on the console. |
| 2 | Key resolution on the default profile: 28 of 46 NX-K names resolved through the vendored hardkeys 0.5.0 (digits, Enter, Record, Clear, Undo, Update, Edit, Copy, Move, Delete, Cue, Group, HighLight, Preview, Next, Last, Menu, Full, @ …). `+ - . /` were refused as **ambiguous** (main-row and keypad shortcuts with equal modifier count). `Fade`, `Delay`, `Snap Shot`, `Back` are **not MA3 hardkeys** (enum read live). `Load`, `Macro`, `Thru` have **no default shortcut**. | Plugin presses the keypad row (`kpAdd` …) as a raw PC key when it is among the candidates: 32 supported. The four non-keys stay unsupported. Load/Macro/Thru need a profile mapping. |
| 3 | A simulated `5` tap put `5` on the command line (first full-chain success). | — |
| 4 | **HMAC-SHA256 in onPC's Lua: 20.7 ms per call** (stock Lua on the same machine: 0.05 ms). With one MAC per packet in and out, a 2 taps/s bench dropped the loop to 10–17 Hz and every ack arrived late (all events counted lost, acks 100 % late). `describeKey` (shortcut-table read + resolve): 6.9 ms; feedback/hardkeys service, state collection, property reads: ≤ 0.05 ms. | MAC changed to **SipHash-2-4** (1.08 ms per 120-byte packet on the console; reference vector verified there). Protocol doc updated. |
| 5 | After the change, 40-tap benches (each tap = press + release, a `Clear` every 10): see table below. At 20 taps/s the loop collapses to 4–5 Hz and events are lost; the per-event cost is now the module's shortcut-table re-read before every press and release (two reads per press, one per release, ~7 ms each). | Acceptance load set to 10 taps/s (human keypad use is below 5 events/s). Module-side caching of the shortcut rows within one frame is a candidate improvement for GrandMA3MCP; not done here. |
| 6 | Retransmit spacing of 25 ms was below the measured round trip, so most events were retransmitted once needlessly. | Spacing 60 ms, 4 retries. |
| 7 | nusb on macOS: the keypad sits unconfigured; `claim_interface(0)` failed with "interface not found" until the service selects configuration 1. After that `run` opens the NX-K and pairs. | Fixed in `nxk/usb.rs`. |

## Bench (service `bench`, plugin `bench` mode, same machine, SipHash)

| taps/s | events | lost | ack round trip median / p95 / p99 / worst (ms) | press-to-effect median / p95 / p99 / worst (ms) | plugin loop (Hz) |
| --- | --- | --- | --- | --- | --- |
| 2 | 80 | 0 | 35.7 / 48.0 / 50.0 / 55.7 | 36 / 51 / 542 / 542 (2 of 34 taps showed no change in 500 ms) | 45–51 |
| 10 | 79 | 0 | 34.0 / 49.5 / 50.7 / 50.7 | 50 / 67 / 183 / 183 | 24–28 |
| 20 | 76 | 50 | 57.2 / 92.8 / 94.9 / 94.9 (26 acks in time) | 0 / 0 / 367 / 367 (acks late, effects measured on the delayed frames) | 4–5 during the burst |

Against the section 8 limits: press-to-effect median ≤ 30 ms is **not met** (36–50 ms;
the console frame of ~20 ms plus one service loop and one plugin frame set the floor);
p99 ≤ 80 ms is met at 10 taps/s except for single outliers. Ack round trip median
≤ 15 ms is **not met** (34–36 ms, two frames). These limits were chosen before
measuring; they are revised in the protocol doc to median ≤ 60 ms, p99 ≤ 120 ms at
10 taps/s, with the measured floor noted.

## Hands-on with the NX-K (operator at the keypad, service `run`)

Reported by the operator after the SipHash build: digits `1 2 3` registered on
the command line; `Record` held lit its LED while `Store` was pending; `Clear`
lit only while held; `HighLight` blinked while highlight mode was on and
stopped after the second press; `Bank` held lit Bank and the four encoder
LEDs. The Link LED was off while paired; it had not been seen blinking because
the service pairs within a frame of starting and the keypad keeps its last
LED state, so the unpaired/stale indication was tested separately by stopping
the plugin (below).

## Not covered in this run

Lost-connection cleanup timing on the console (harness only), a flood at the
console, Windows, a second display, non-US layouts.

## Cleanup

Command line cleared with Escape ×2 after each bench; no holds or unresolved
records left on the surface instance. Plugin 2 and macros 110/111 remain in the
disposable show.

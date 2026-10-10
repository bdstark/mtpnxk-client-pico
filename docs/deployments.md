# Supported deployments (KB-08)

Updated 2026-10-10. What has been qualified, on what, with which evidence, and
what has not. Testing one row never establishes another. "Qualified" means a
recorded live run on a disposable show at a stated revision; "harness" means
stock-Lua or Rust tests with stubbed console dependencies; "unqualified" means
no evidence, whatever the code is expected to do.

Two independent consumers exist, and their evidence is kept apart:

| Consumer | What it is | Where its evidence lives |
| --- | --- | --- |
| **MCP bridge** (`gma3_mcp_bridge`, Node.js MCP server) | agents driving onPC over loopback TCP; owns its own hardkeys and feedback instances | [bdstark/GrandMA3MCP `docs/compatibility.md`](https://github.com/bdstark/GrandMA3MCP/blob/main/docs/compatibility.md) and its `docs/probes/` (KB-01 to KB-06) |
| **Surface** (this repository: `mtpnxk_surface` plugin + Rust service + NX-K) | a physical keypad driving onPC over authenticated UDP, LEDs from console state | [KEYBOARD.md](../KEYBOARD.md), [probes/kb-07-live-macos-2.5.1.md](probes/kb-07-live-macos-2.5.1.md), [probes/kb-08-qualification-macos-2.5.1.md](probes/kb-08-qualification-macos-2.5.1.md), [kb-08-acceptance.md](kb-08-acceptance.md) |

Bridge support does not imply surface support: the surface vendors the bridge's
console modules, but its transport, pairing, loop and LED path are its own and
were qualified only as listed below.

## Surface: console and host

| Console | Host OS | Service host | Status | Evidence |
| --- | --- | --- | --- | --- |
| grandMA3 onPC 2.5.1.0 | macOS 26.5.1 (Apple silicon, `bdsmbpm401`) | same machine, `127.0.0.1` | **Qualified (beta)**: plugin import/start/stop/status/recover, pairing, key dispatch for 32 of 46 NX-K keys on the default profile, lifecycle (service stop, cable pull, plugin restart, user switch), feedback freshness, stale indication, allow list; NX-K over nusb with LEDs confirmed by the operator (KB-07). Open: press-to-effect p99 at 10 taps/s not demonstrated (instrument defect), flood limits not met | KB-07 and KB-08 records |
| grandMA3 onPC 2.5.1.0 | macOS | **separate machine on the LAN** (`bind=0.0.0.0 allow=<ip>`) | Unqualified. The binding and allow list were exercised between the two addresses of one machine only; latency over a real network, a NAT or a second host's clock were not measured | KB-08 L7 |
| grandMA3 onPC 2.5.1.0 | Windows 11 | any | Unqualified for the surface. The service needs WinUSB bound to the NX-K (Zadig) and that path has not been run; the console modules were probed on Windows only in KB-01 (bridge 0.3.4, exploratory) | MCP KB-01 Windows record |
| grandMA3 onPC 2.5.1.0 | Linux | any | Unqualified (onPC is not available for Linux; a Linux *service* host against a macOS or Windows console is the LAN case above, also unqualified; a udev rule would be needed for non-root USB access) | — |
| other onPC versions | any | any | Unqualified. The plugin reports the console version in `welcome.console` and refuses nothing; routes and readers are verified on 2.5.1.0 only | — |
| physical grandMA3 consoles | — | — | Unqualified and not planned (no LuaSocket UDP listener qualified there) | — |

## Surface: layout, profile, displays

| Dimension | Qualified | Not qualified |
| --- | --- | --- |
| Keyboard layout | US (profile `Default`): 32 keys resolve, `+ - . /` through the keypad row | Any non-US layout. Key routes come from the user profile's shortcut table, so a different layout can change which keys resolve; start the plugin and read the `key X unsupported` lines |
| User profile | `Default` only (one profile in the test show) | Profiles without shortcuts for Load, Macro, Thru keep those keys unsupported; a profile switch while paired is untested (a user switch is qualified: feedback is invalidated within ~1 s) |
| Displays | One physical display (`display=1`) | Two displays. Input is **not display-routed** on 2.5.1 (MCP KB-01): a second-display test would show where pop-ups land, not route input; `display=` is API context only |
| Show lifecycle | plugin stop/start, user switch | Show save and reload with the plugin running (harness only for the kept-records path; not issued live) |
| Hardware | NX-K (VID `11be`, PID `e102`) over nusb on macOS | M-Touch (PID `f808`) and M-Play (PID `f80c`): decoder and output encoders implemented from documented evidence (MTouchPlay `e48eb2c`), regression-tested (17 tests, [mtouch-protocol-reuse.md](mtouch-protocol-reuse.md)); **Qualified on macOS (protocol level, 2026-10-10)**: every documented control, report type, LED/bar/display write, idle poll, queued-at-open packets, unplug detection and replug on both devices ([M-Touch record](probes/kb-16-hardware-mtouch-macos.md), [M-Play record](probes/kb-16-hardware-mplay-macos.md)); not wired into the link (KB-20+), Windows and Linux hosts unqualified. Encoders on any surface (carried as `wheel`, acknowledged `unsupported`) |

## Surface: coexistence with the MCP bridge

Qualified only as the **operating rule** of section 6 of
[surface-protocol.md](surface-protocol.md): one injector with input enabled at
a time. The surface refuses to enable input while the bridge reports input
enabled unless started with `force`; the KB-08 run used `force` because the
bridge was the measurement instrument and held no keys. Two injectors with
input enabled at once are unqualified and not supported.

## Automated evidence (any host with stock Lua and Rust)

| Suite | What it establishes | Does not establish |
| --- | --- | --- |
| `lua tools/ma3/test/surface_plugin_test.lua` (187 checks) | protocol, pairing, dedup, reconciliation, leases, floods on the stubbed loop, feedback deltas and the KB-17 context, KB-18 control events (admission, coalescing, order, loss, boundaries, lapses, kept releases), kept records, quarantine, two instances, Cleanup | anything about the real `Keyboard()` route, the real readers or console timing |
| `cargo test` in `service/` (45 tests) | framing and MAC, link state machine with a fake plugin (keys, context, KB-18 control events: binding, coalescing, rate and age bounds, boundaries, loss and refusals, repair), LED map and local fallback, NX-K decoder, M-Touch/M-Play decoder and output encoders against the MTouchPlay captures | USB behaviour, console behaviour |
| `sh tools/ma3/test/e2e.sh` | the real service and the real plugin (stock Lua, stubbed console) across a real UDP socket, including rotary `ctl` events refused honestly by a console without an encoder bar | console latency, LEDs on hardware, anything moving on a console |

Record new evidence as in the MCP repository's "Recording additional
qualification": revision and hashes, OS, onPC version, layout, displays,
user/profile, procedure, observed effects, cleanup, and what was not tested.

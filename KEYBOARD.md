# KB-07 in mtpnxk: the surface consumer

Updated: 2026-10-09. The feature series KB-01 to KB-08 is defined and tracked in
[bdstark/GrandMA3MCP `KEYBOARD.md`](https://github.com/bdstark/GrandMA3MCP/blob/main/KEYBOARD.md).
KB-01 to KB-06 are complete there (macOS, onPC 2.5.1.0) and provide the reusable
console modules this repository vendors. KB-07, the surface consumer, lives here.

Direction change (2026-10-09): the consumer is a **cross-platform Rust service**
on the PC, not Pico firmware. The Pico tree is archived under
[legacy/pico](legacy/pico/ARCHIVED.md) and tagged `pico-firmware-final`.

## KB-07 — Integrate the independent mtpnxk surface consumer

**Request:** As a surface user, I want responsive keypad input and trustworthy
LED state across network interruptions.

**Depends on:** KB-03, KB-04, KB-06 (vendored). No MCP bridge change was
needed. One reusable-module change was made in the MCP repository and
re-vendored rather than forked: `gma3_mcp_hardkeys` 0.5.0 resolves any
`Enums.VirtualKeyCode` name through the shortcut table (the NX-K's Edit, Copy,
HighLight, … keys), with regressions in that repository's harness.

**Design:** [docs/surface-protocol.md](docs/surface-protocol.md). The six
decisions it settles, in the order they were asked for:

1. Feedback schema reconciled against the KB-06 readers: tri-state items
   (`0`, `1`, `"?"`), derived `pending` and `preview`, units, unknown/stale
   rules, protocol and module versions (section 1).
2. Pairing and packet semantics: HMAC-SHA256 over every datagram with a
   32-byte pairing key, fresh random session ids, strictly increasing sequence
   numbers, event ids with retransmission, acks, lease and forget times,
   validation before any module call (section 2).
3. Event delivery separated from held-key reconciliation: duplicates
   deduplicated, releases owner-scoped, heartbeats release lost releases and
   never press, nothing replayed after either side restarts (section 3).
4. Loop budget: deadlines first, feedback reads spread over frames, bounded
   packets per frame, per-session rate limit, auth-failure throttle (section 4).
5. Freshness: plugin generation + feedback epoch, stale and identity-uncertain
   values rendered unknown, console state kept apart from the surface's own
   holds (section 5).
6. Exclusive use as an operating rule, with a courtesy check against the MCP
   bridge that is explicitly not arbitration (section 6).

Measurable limits (press latency, ack round trip, cleanup, LED freshness,
stale indication, flood resilience) are chosen in section 8 and reported as
median / p95 / p99 / worst.

### Implementation

| Part | Where | Verified by |
| --- | --- | --- |
| Surface plugin (Lua, runs in onPC) | [tools/ma3/mtpnxk_surface.lua](tools/ma3/mtpnxk_surface.lua), [mtpnxk_surface.xml](tools/ma3/mtpnxk_surface.xml) | `lua tools/ma3/test/surface_plugin_test.lua` — 87 checks under stock Lua with stubbed console and socket: crypto vectors, pairing/replay/MAC rejection, press/release/dedup/old-seq/reordering, unsupported keys, heartbeat reconciliation (lost release released, lost press reported not pressed), lease expiry/revival/forgetting, restart via new hello, capacity, per-session rate limit, a 10 000-datagram flood not starving a lapsed lease, feedback deltas/full/unknown/stalled reader/epoch bump/identity uncertainty, keyboard backend calls, bench mode, the bridge courtesy check, two independent instances, allow list, control calls and Cleanup |
| Vendored console modules | [tools/ma3/gma3_mcp_hardkeys.lua](tools/ma3/gma3_mcp_hardkeys.lua) 0.5.0, [gma3_mcp_feedback.lua](tools/ma3/gma3_mcp_feedback.lua) 0.2.0, [VENDOR.md](tools/ma3/VENDOR.md) | the MCP repository's harnesses (`npm test` there, 362 + 149 + 360 Lua checks) |
| Surface service (Rust) | [service](service) — `auth` (framing/HMAC), `protocol`, `link` (pairing, seq/ev, retransmit, heartbeat, watchdog, state cache), `leds` (NX-K map, local fallback), `nxk` (decoder, USB via nusb), `sim`/`bench` | `cargo test` — 18 tests incl. a fake plugin exercising retransmission and loss, nonce mismatch, old seqs, full/delta/epoch freshness, watchdog and re-pairing, `no-session` recovery, forged packets, LED diffs and the local fallback |

### Status

- **Verified end to end without a console** (`sh tools/ma3/test/e2e.sh`): the real
  plugin under stock Lua with a stubbed console, behind `udp_pipe_bridge.py` on a
  real UDP port, driven by the real service. Pairing, acks, a retransmission
  with zero losses, two refused local keys, an abandoned session released by
  the lease, and a 100-tap bench (ack round trip median 9 ms, p99 26 ms
  through a 16 ms relay frame; press-to-effect samples come from the stub's
  synthetic command line and say nothing about the console yet).
- Implemented and harness-tested, both sides. **Not yet run against onPC**:
  the plugin has not been imported into a console, the `Keyboard()` route and
  the readers are exercised only through the vendored modules' own live
  evidence (KB-04/KB-06), and the NX-K has not been driven from the Rust
  service (nusb path untested on hardware; Windows needs WinUSB via Zadig).
- Wheels are carried in the protocol and acknowledged `unsupported`: no
  verified Lua route for encoder input exists (needs a probe).
- The VirtualKeyCode names for Update, Edit, Copy, Move, Delete, Load, Cue,
  Group, Macro, Fade, Delay, HighLight, Preview, Next, Last, Menu, Snap Shot,
  Thru, Full, @, +, −, ., / and Back are **unverified**; the plugin reports
  each at start (`key X unsupported: …`) and in `welcome.keys`. Keys without a
  default shortcut need a profile mapping by the operator.
- Qualification (section 8 limits) has not been measured. Tooling exists:
  `mtpnxk bench` plus the plugin's `bench` start option.

### Next steps

1. On the onPC Mac: copy `tools/ma3/*.lua` and the XML into
   `~/MALightingTechnology/gma3_library/datapools/plugins`, import, start with
   `key=<64 hex> input=fake bench`, run `mtpnxk sim` then `mtpnxk bench`
   from the same machine; record the `key … unsupported` lines and fix the
   VirtualKeyCode table. Then `input=keyboard` on a disposable show.
2. Plug in the NX-K, `mtpnxk run`, confirm LEDs follow pending keywords,
   HighLight and the Link LED; record the section 8 numbers in `docs/probes/`.
3. Probe encoder input (`Encoder` keyword or an attribute-level call) before
   routing wheels; keep them unsupported until a route is verified.
4. KB-08 documentation and the Windows qualification.

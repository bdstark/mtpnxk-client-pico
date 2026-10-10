# KB-07, KB-08 and KB-15 in mtpnxk: the surface consumer and its qualification

Updated: 2026-10-09 (KB-15 surface half added, see the last section). The feature series KB-01 to KB-09 is defined and tracked in
[bdstark/GrandMA3MCP `KEYBOARD.md`](https://github.com/bdstark/GrandMA3MCP/blob/main/KEYBOARD.md).
KB-01 to KB-06 are complete there (macOS, onPC 2.5.1.0) and provide the reusable
console modules this repository vendors. KB-07 (the surface consumer) and KB-08
(documentation and qualification of the surface) live here; KB-09 (shortcut-table
cache, operator-managed profile shortcuts) is module work in the MCP repository.

Direction change (2026-10-09): the consumer is a **cross-platform Rust service**
on the PC, not Pico firmware. The Pico tree is archived under
[legacy/pico](legacy/pico/ARCHIVED.md) and tagged `pico-firmware-final`.

Operator documents produced by KB-08: [docs/operator-guide.md](docs/operator-guide.md),
[docs/deployments.md](docs/deployments.md), [docs/kb-08-acceptance.md](docs/kb-08-acceptance.md).

## KB-07 — Integrate the independent mtpnxk surface consumer

**Request:** As a surface user, I want responsive keypad input and trustworthy
LED state across network interruptions.

**Depends on:** KB-03, KB-04, KB-06 (vendored). No MCP bridge change was
needed. One reusable-module change was made in the MCP repository and
re-vendored rather than forked: `gma3_mcp_hardkeys` 0.5.0 resolves any
`Enums.VirtualKeyCode` name through the shortcut table (the NX-K's Edit, Copy,
HighLight, … keys) and lets the consumer prefer the row of a same-target tie
(`+ - . /` on the keypad row), with regressions in that repository's harness
(PR #12, merged as `6e0d9c1`).

**Design:** [docs/surface-protocol.md](docs/surface-protocol.md). The six
decisions it settles, in the order they were asked for:

1. Feedback schema reconciled against the KB-06 readers: tri-state items
   (`0`, `1`, `"?"`), derived `pending` and `preview`, units, unknown/stale
   rules, protocol and module versions (section 1).
2. Pairing and packet semantics: a SipHash-2-4 MAC over every datagram under
   the first 16 bytes of a 32-byte pairing key (HMAC-SHA256 was the first
   choice and cost 20 ms per packet in onPC's Lua), fresh random session ids,
   strictly increasing sequence numbers, event ids with retransmission
   (4 × 60 ms), acks, lease and forget times, validation before any module
   call (section 2).
3. Event delivery separated from held-key reconciliation: duplicates
   deduplicated, superseded events refused, releases owner-scoped, heartbeats
   release lost releases and never press, nothing replayed after either side
   restarts, unresolved releases kept as records across restarts (section 3).
4. Loop budget: deadlines first, feedback reads spread over frames, bounded
   packets per frame, per-session rate limit, auth-failure throttle (section 4).
5. Freshness: plugin generation + feedback epoch, stale and identity-uncertain
   values rendered unknown, console state kept apart from the surface's own
   holds (section 5).
6. Exclusive use as an operating rule, with a courtesy check against the MCP
   bridge that is explicitly not arbitration (section 6).

Measurable limits (press latency, ack round trip, cleanup, LED freshness,
stale indication, flood resilience) are chosen in section 8, which now also
carries each limit's measured status.

### Implementation

| Part | Where | Verified by |
| --- | --- | --- |
| Surface plugin (Lua, runs in onPC) | [tools/ma3/mtpnxk_surface.lua](tools/ma3/mtpnxk_surface.lua) 0.1.0, [mtpnxk_surface.xml](tools/ma3/mtpnxk_surface.xml) | `lua tools/ma3/test/surface_plugin_test.lua` — 107 checks under stock Lua with stubbed console and socket: SipHash vectors, pairing/replay/MAC rejection, press/release/dedup/old-seq/reordering/superseded, duplicate acks replaying the original outcome, unsupported keys, heartbeat reconciliation (lost release released, lost press reported not pressed), lease expiry/revival/forgetting, restart via new hello, capacity, per-session rate limit, a 10 000-datagram flood not starving a lapsed lease, feedback deltas/full/unknown/stalled reader/epoch bump/identity uncertainty, keyboard backend calls, bench mode, the bridge courtesy check, two independent instances, allow list, kept records adopted across a restart, `recover`, a quarantined instance whose `dispose()` raises, control calls and Cleanup |
| Vendored console modules | [tools/ma3/gma3_mcp_hardkeys.lua](tools/ma3/gma3_mcp_hardkeys.lua) 0.10.0 (0.5.0 at the KB-07/KB-08 records), [gma3_mcp_feedback.lua](tools/ma3/gma3_mcp_feedback.lua) 0.2.0, [VENDOR.md](tools/ma3/VENDOR.md) (commits and hashes) | the MCP repository's harnesses (`npm test` there) |
| Surface service (Rust) | [service](service) — `auth` (framing, SipHash), `protocol`, `link` (pairing, seq/ev, retransmit, heartbeat, watchdog, state cache), `leds` (NX-K map, local fallback), `nxk` (decoder, USB via nusb), `sim`/`bench` | `cargo test` — 19 tests incl. a fake plugin exercising retransmission and loss, a release stopping its press's retransmission, nonce mismatch, old seqs, full/delta/epoch freshness, watchdog and re-pairing, `no-session` recovery, forged packets, LED diffs and the local fallback |
| The two together, no console | [tools/ma3/test/e2e.sh](tools/ma3/test/e2e.sh) | the real plugin under stock Lua with a stubbed console behind a UDP relay, driven by the real service: pairing, acks, a retransmission with zero losses, two refused keys, an abandoned session released by the lease, a 100-tap bench |

### Status

- **Verified end to end without a console** (`sh tools/ma3/test/e2e.sh`), see the
  table above (press-to-effect samples come from the stub's synthetic command
  line and say nothing about the console).
- **First run against onPC 2.5.1.0 on 2026-10-09**
  ([docs/probes/kb-07-live-macos-2.5.1.md](docs/probes/kb-07-live-macos-2.5.1.md),
  historical evidence): import and macro start work, 32 of 46 NX-K keys resolve
  on the default profile, a simulated tap reaches the command line, benches at
  2 and 10 taps/s lose nothing, the NX-K opens over nusb on macOS once the
  configuration is selected, and the operator confirmed the LEDs on the keypad
  (Record while Store pending, Clear while held, HighLight blinking, Bank and
  the encoder LEDs, the Link LED blinking when the plugin stops). Two findings
  changed the design during that day: HMAC-SHA256 → SipHash-2-4, and the
  module's shortcut-table re-read per event (7 ms) caps the plugin at roughly
  10 taps/s (KB-09). The record's "keypad row as a raw PC key" workaround was
  removed in the review below.
- **Qualified in KB-08** on the final revision: see below.
- Wheels are carried in the protocol and acknowledged `unsupported`: no
  verified Lua route for encoder input exists (separate work).
- Fade, Delay, Snap Shot and Back are not MA3 hardkeys; Load, Macro and Thru
  have no shortcut in the default profile and need one in the user profile
  (KB-09 part B plans an assisted way).

### Review of 2026-10-09 (five findings, all fixed)

1. A retransmitted press could execute after its release: event ids now order the events of a key
   (older than the newest processed → `superseded`, nothing dispatched), and the service stops
   retransmitting a press once it sends the release.
2. Unresolved releases were dropped across a restart: records are kept on stop, Cleanup and
   `service()` errors, adopted before input is enabled at the next start (key reserved, new press
   refused `conflict`), and released by the new `recover` command.
3. The keypad fallback bypassed route safeguards: removed; `+ - . /` now resolve through hardkeys
   0.5.0's `spec.prefer` with enablement, collision and route rechecks intact.
4. A retransmitted refusal was acknowledged as success: the original acknowledgment is cached per
   event id and replayed.
5. (second pass) A `dispose()` that raises no longer loses ownership: the instance is quarantined with
   its records, input is blocked at the next start, and `recover` exports and releases them before
   re-enabling the requested input mode.

Harness: 107 checks (was 87); service: 19 tests; `e2e.sh` passes. The shared module's preference change
is pushed as GrandMA3MCP `2d226dd` (PR #12, merged) and the vendored copy matches it byte for byte.

### Open defects found by KB-08 (owned here, not fixed in KB-08)

- **D1, bench instrument.** `bench` mode snapshots `cmdtext` after `hardkeys:press()` returns; a key the
  console applies within that call is invisible to the watch and the sample inherits the next tap's
  change. At 10 taps/s this produced a measured press-to-effect p99 of 156 ms (n=359) while an independent
  poller showed every tap landing. Fix the snapshot order, rerun with ≥ 360 samples, and report p99
  again; until then the p99 ≤ 120 ms limit is **not demonstrated**.
- **D2, flood handling.** The auth-failure throttle is per source address (a flood from the service's own
  address locks the service out for 10 s), and the 32-datagram read budget per iteration lets a 2000 pps
  flood from another address queue legitimate packets in the kernel past the retransmit budget (98 of
  195 events lost). The flood limit of section 8 is **not met**; decide between a time-bounded read
  budget that discards ignored sources cheaply, a deployment-specific limit, or a revised limit.

## KB-08 — Document and qualify supported deployments

**Request:** As a user, I want an accurate setup and compatibility statement for the delivered capabilities.

**Lua changes: none.** Documentation and qualification only; the two defects above are recorded
against KB-07. Caching (KB-09), automatic shortcut creation (KB-09 part B) and encoder support stay
separate.

**Delivered (2026-10-09):**

1. Documentation reconciled with the implementation: SipHash instead of HMAC, the raw-key workaround
   gone, 107 harness checks and 19 service tests, the protocol's retransmit parameters, section 8 with
   measured status, VENDOR.md with commits and hashes; the MCP repository's module pin, lock file,
   example version checks and compatibility matrix updated to hardkeys 0.5.0 and the surface
   evidence. The KB-07 probe record is kept unchanged as history.
2. The performance discrepancy resolved: the KB-07 table had used the 40-tap p95 (67 ms) as p99; the
   p99 was 183 ms. The rerun at the final revision with 359 samples measures 51 / 105 / 156 / 158 ms
   and is reported as not meeting the p99 limit, with the tail traced to defect D1
   ([docs/probes/kb-08-qualification-macos-2.5.1.md](docs/probes/kb-08-qualification-macos-2.5.1.md) §1).
3. Lifecycle observed on the console (same record, §2): clean service stop, cable pull (lease release
   1.99 s), plugin stop/restart with a physically held key (stale LED, re-pairing, no replay), user
   switch (feedback invalidated within 1 s), LED freshness (≈ 126 ms median), `status`/`recover`, the
   allow list. Exception and quarantine paths stay harness-tested and are labelled so.
4. A specific deployment matrix: [docs/deployments.md](docs/deployments.md) (MCP vs surface,
   same machine vs LAN, macOS/Windows/Linux, layouts, profiles, displays, hardware), with every
   untested combination named as unqualified.
5. An operator guide: [docs/operator-guide.md](docs/operator-guide.md) (install and update all three
   plugin components, pairing key, USB, supported and unmapped keys, exclusive input, status, stop,
   `recover`, what to do when something stays unresolved; `force` kept out of the normal start).
6. A reproducible acceptance record: [docs/kb-08-acceptance.md](docs/kb-08-acceptance.md) (commits,
   hashes, environments, commands, results, verdicts, exclusions; the real-service/stubbed-console
   e2e run beside the Lua and Rust suites).

**Closed** with the macOS beta scope of docs/deployments.md and the exclusions listed there and in the
acceptance record. Not closed as a pass on the p99 or flood limits.

## Follow-ups filed

- [bdstark/GrandMA3MCP#12](https://github.com/bdstark/GrandMA3MCP/pull/12) (merged): hardkeys 0.5.0 and the
  **KB-09** write-up: bounded per-iteration cache of the shortcut rows (lifts the ~10 taps/s ceiling), same-target tie
  resolution, and operator-managed profile shortcuts for Load, Macro and Thru.
- KB-07 defects D1 and D2 above.
- Unqualified deployments (Windows with WinUSB, a separate LAN host, a second display, non-US layouts,
  show reload with the plugin running) each need a recorded run before they are claimed.

## Next steps

1. Fix D1 in the plugin's bench mode, rerun `mtpnxk bench --taps 400 --rate 10` on the console and
   update section 8 and the acceptance record.
2. Decide D2 (read budget, throttle scope or limit) and re-measure the flood case.
3. With the NX-K attached, repeat the hands-on LED checks at the final revision and add them to the
   KB-08 record.
4. Windows qualification (WinUSB via Zadig) and a separate-machine LAN run when hardware allows.
5. Probe encoder input before routing wheels; keep them unsupported until a route is verified.

## KB-15 — Surface defaults, migration and qualification (surface half, 2026-10-09)

The module and bridge half is in the MCP repository (hardkeys 0.10.0, bridge 0.12.0, branch
`feat/kb15-mixed-backend`). This repository vendors 0.10.0 unchanged ([tools/ma3/VENDOR.md](tools/ma3/VENDOR.md))
and adds the surface side in `mtpnxk_surface.lua` 0.2.0:

- `bank=<quickey>/<page>.<first>-<last>` provisions the owned Quickey bank from the plugin argument only
  (`authorized = true` is the operator's argument, never a surface request); codes default to the nine with
  KB-10 evidence (`bankcodes=hardkeys` for all 94). The record is kept across `stop` and adopted at the next
  start; `bank status | verify | teardown` are control calls on the running instance.
- `input=mixed` attaches `mixedBackend({ quickey = quickeyBackend(inst), keyboard = keyboardBackend(deps) })`
  with the policy `{ default = "quickkey", keys = <every key without KB-10 hold evidence -> shortcut> }`; the
  table is explicit (`NXK_QUICKKEY_HOLD`: NUM1, NUM5, THRU, PLEASE, CLEAR, STORE) because the surface holds every
  key, so a tap-only code (OOPS) is not a Quickey key. `input=quickey` is `{ default = "quickkey" }` alone.
  `route=<key>:<method>` overrides single keys; the module validates every method against the attached
  backend (`policy-unavailable`) and the surface refuses the start when an override resolves to no usable
  route. Without a bank the Quickey modes report the KB-12 requirement and press nothing.
- The start and `status` report the method, effective route and dispatching part per key (`describeRoute`);
  the welcome carries `backend`. `recover` attaches the part the kept records name (keyboard, quickey or
  mixed), as the bridge does.
- A stale run left by `ReloadAllPlugins` (state marked running, loop dead, socket still bound) is taken over
  by the next start instead of blocking it (found during this run).
- 0.2.1 (KB-16 review): a temporary shortcut-mode change that `dispose()` could not restore (KB-14: a dependent
  key still held, the restore delay not elapsed, or the profile/mode changed meanwhile) is no longer dropped at
  stop, Cleanup or quarantine export. The record is kept in `state.modeRecord`, reported by `status`, adopted at
  the next start as an unresolved restoration (every press refused `busy` until it is restored) and restored by
  `recover` on the original profile, exactly as the bridge does; harness: stop with a pending restoration,
  restart, refusal, status, recover, presses accepted again.

Harness: 143 checks (36 new: parsing, no-bank refusal, provisioning, the per-key report, Quickey executor
press/Unpress, both unqualified-mix directions, bank status/teardown refusals, record kept and adopted,
an unresolved Quickey record recovered through the quickey part, Quickeys-only refusals, policy refusals,
inert overrides, teardown and the no-fallback afterwards, stale-run takeover). The KB-14 mode operation
changed one KB-07 check: a key whose shortcut row is off is pressed with a temporary enable and restored.
`sh tools/ma3/test/e2e.sh` passes unchanged on the keyboard path.

**Verified live** (macOS, onPC 2.5.1.0, show `mcp-test-disposable`, the hardware service paired throughout):
[docs/probes/kb-15-surface-macos-2.5.1.md](docs/probes/kb-15-surface-macos-2.5.1.md). Not established: a
second display, show save/reload with the bank, Windows/Linux, LED behaviour seen by an operator, the
press-to-effect figure for Quickey taps (the bench's tap length dominates it, see the record).

## KB-16 — Qualify existing hardware protocols (surface half, 2026-10-10)

The Rust service gains `src/mtouch/`: control tables, report decoder and output encoders for the Martin
M-Touch (`11be:f808`) and M-Play (`11be:f80c`), ported from the MTouchPlay repository at `e48eb2c` and
regression-tested against its capture logs (17 tests; `cargo test` now runs 36), plus the operator commands
`mtouch-listen` and `mtouch-led-test`. What was reused, what was not re-verified, the regression table and
the live qualification procedure (recorded into `docs/probes/kb-16-hardware-<device>-<os>.md`) are in
[docs/mtouch-protocol-reuse.md](docs/mtouch-protocol-reuse.md). Both devices were qualified live on macOS the same day
([M-Touch](docs/probes/kb-16-hardware-mtouch-macos.md), [M-Play](docs/probes/kb-16-hardware-mplay-macos.md):
every control, report type and output write, idle polls, queued reports at open, unplug and replug), and nothing is wired
into the link (`run`/`sim`/`bench` remain NX-K only; integration is KB-20+). Hardware protocol
qualification does not establish grandMA3 behaviour: the console-semantics half of KB-16 lives in
bdstark/GrandMA3MCP `ENCODERS.md`.

## KB-17 — Shared control-context and binding snapshots (surface half, 2026-10-10)

The module half is in the MCP repository (`gma3_mcp_feedback` 0.3.0, bridge 0.13.0, branch `feat/kb17-context-snapshot`,
live record `docs/probes/kb-17-context-macos-2.5.1.md` there, 31/31). This repository vendors the 0.10.0/0.3.0 pair unchanged
([tools/ma3/VENDOR.md](tools/ma3/VENDOR.md)) and adds the surface side in `mtpnxk_surface.lua` 0.3.0:

- the context items (data pool, executor page, encoder bank and slots of the configured display, one target per
  `execs=` executor) are watched next to the state items, so the loop's bounded reads keep them observed;
- every tick builds the module's cached `contextSnapshot()` and sends it as a `context` message
  ([docs/surface-protocol.md](docs/surface-protocol.md) section 1a) when its binding generation, `known` flag or epoch
  moved, and with the full-state cadence otherwise. No grandMA3 semantics are reconstructed here: unavailable parts
  carry the module's reason, and while a part is unobserved `known = 0` and no generation is claimed;
- the welcome carries `context: 1`; `status` counts `contexts`.
- the Rust service parses the message (`FromPlugin::Context`), keeps the last one as data (`Link.context`), counts
  received contexts and generation moves and logs each move with bank/page/context and the slot/executor counts.
  The context is dropped on link down, re-pairing and every new welcome, and `Link::context_view(now)` presents one
  only while paired with the link up, received in this pairing within `state_stale` and marked known by the plugin
  (`cargo test`: 38, two new).

Harness: 159 checks (8 new: the watch list, the message after the parts are observed with the missing encoder bar
explicit, cadence, an executor-page change moving the generation in the frame it is observed, back again, a show
change withdrawing the generation until every part is observed, resumption in the new epoch). `sh tools/ma3/test/e2e.sh`
unchanged on the keyboard path.

**Not verified live in this repository:** the surface plugin against onPC with the 0.3.0 pair (the MCP repository's
probe exercised the same module through the bridge); an encoder bar is only present on display 1 there, so the
plugin's default `display=1` is the one that answers. Wiring the context into encoder bindings is KB-18/KB-19.

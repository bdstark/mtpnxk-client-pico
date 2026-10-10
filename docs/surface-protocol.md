# Surface protocol and KB-07 design (2026-10-09, status updated by KB-08)

How a surface service (the Rust process that owns the NX-K) and the
`mtpnxk_surface` Lua plugin inside grandMA3 onPC talk to each other, what each
side guarantees, and the limits the integration is qualified against. This
supersedes the OSC transport in [ma3-feedback.md](ma3-feedback.md); the LED
findings and the NX-K map in that file still apply.

The console-side building blocks are the reusable modules from
[bdstark/GrandMA3MCP](https://github.com/bdstark/GrandMA3MCP) (`KEYBOARD.md`
there, KB-01 to KB-06), vendored unchanged into [tools/ma3](../tools/ma3). The
plugin owns transport, pairing, packet semantics and surface-key mapping; the
modules own console semantics (key routes, ownership, leases, readers).

## 1. Feedback schema, reconciled against the KB-06 readers

The KB-06 feedback module hands over *observations*, each `available=true`
with a value, or `available=false` with a reason. The surface schema keeps that
distinction end to end: every state item is tri-state, and the service renders
"unknown" explicitly (section 5).

| State key | KB-06 reader | Scope | Wire value | Meaning for the surface |
| --- | --- | --- | --- | --- |
| `blind`, `highlight`, `solo` | `blind` / `highlight` / `solo` (`FADERENABLED`) | show | `0`, `1`, `"?"` | mode on/off; `"?"` when unavailable or stale |
| `preview` | `previewMode` (`ACTIVEENVIRONMENT`, a string) | profile | `0`, `1`, `"?"` | derived: `"Preview"` → 1, `"Live"`/`"Normal"` → 0, any other name → `"?"` (never guessed off). The raw name is in `previewEnv`. |
| `previewBar` | `previewBar[display=1]` | display | `0`, `1`, `"?"` | pending Preview on display 1; not a routing promise |
| `ma` | `maState` (`Root().MASTATE`) | console | `0`, `1`, `"?"` | aggregate of every Shift source; never the surface's own hold |
| `shortcuts` | `shortcutsActive` | profile | `0`, `1`, `"?"` | shortcut-table routes need 1; the service shows it on the Link LED pattern |
| `pending` | derived from `commandText` (`CmdObj().cmdtext`) | ui | lower-case first word (`"store"`, `"edit"`, …), `""` (none), `"?"` | the keyword awaiting input. Derivation: first alphabetic word of the command line, lower-cased, if it is in the plugin's keyword list; otherwise `""`. Raw text is not sent. |
| `page` | `page` (`CurrentExecPage()`) | user | integer, `"?"` | current executor page number |
| `freeze` | `freeze` | show | always `"?"` | KB-01 found no readable state |
| `x<n>` | `sequenceActive` via `executor[n]` assignment | show/page | `0`, `1`, `"?"` | executor n on the current page has an active playback (`"?"` when empty) |
| `f<n>` | `fader[executor=n]` | show | integer 0–100, `"?"` | master fader level, rounded |

Units: fader levels are percent (the reader's `value` 0..100, rounded to an
integer). Page numbers are the page's `No`. Booleans are integers so the
service parser has one scalar type per key; `"?"` is reserved for unknown.

**Unknown / stale.** An item is `"?"` when the reader reports
`available=false` (nil, unrecognised value, missing object, reader error), when
the feedback snapshot marks it `stale` (older than `staleMs`, 2000 ms) or when
the module reports the identity (show file, user, profile) uncertain. The
service additionally treats *every* item as unknown when no `state` packet
arrived for 1500 ms or when the plugin generation / epoch changed since the
last full state (section 5).

**Protocol versions.** `v` in `hello`/`welcome` is the surface-protocol
version (1). `welcome.modules` carries the vendored module versions
(`gma3_mcp_hardkeys` 0.10.0, `gma3_mcp_feedback` 0.3.0, API 1) so the service can
log what it is paired with. onPC 2.5.1.0 is the only console version the
readers and routes are verified on (KB-01 to KB-06, macOS); the plugin
refuses nothing on other versions but reports `console` in `welcome`.

Not carried: `lastCommand` (shared history, no LED use), `selectedSequence`
(M-Touch later), raw `commandText` (quoting and size; `pending` is enough for
the NX-K).

### 1a. Control context (`context` message, KB-17)

The plugin also sends `{t:"context", sid, seq, gen, epoch, known, cg?, display, pool, page, enc, slots[], sel?, slotsWhy?, ex[]}`:
a compact copy of the vendored feedback module's `contextSnapshot()` (`gma3_mcp_feedback` 0.3.0, built from the same
watched observations as `state`, with no read of its own). It says what each encoder slot and executor would
operate right now; the service keeps it as data (`Link.context`) and renders or binds nothing from it yet (KB-18+).

| Field | Meaning |
| --- | --- |
| `known` | `1` when every part was observed in this epoch and the selection identity is complete; `0` otherwise, and then `cg` is absent (no generation is claimed, as the module does) and `why` carries the module's reason |
| `cg` | the module's binding generation for this plugin's spec (configured display and executors): it moves when an input's meaning changed (identity, bank/page/context, the selection's fixtures, a slot's object, resolution, readout, channel function, layer or availability, the executor page, an executor's assignment, functions or target status, or any of these becoming unreadable), never for a value, level, activity or label alone. Comparable only within one `(gen, epoch)` |
| `display`, `pool`, `page` | the authoritative display (the plugin's `display=` argument, default 1; never another display), the data pool name, the executor page number (`"?"` when unknown) |
| `enc` | `{bank, bankName, page, pageName, ctx, attr}` (1-based, `attr = 1` for the `Default` attribute-editing context) or `{why}` when that display's encoder bar is unavailable |
| `slots[]` | per pool slot: `{n, kind}` plus, for `attribute`, `name, label, unit, readout, res, layer, cf` (channel function), `avail` (`no-selection | available | unavailable | mixed`), `val` (`none | value | empty | mixed | unavailable`) and `abs` when a programmer value exists; `kind = "other"` carries `ref` (phaser/editor slots, unsupported); `kind = "empty"` nothing |
| `sel` / `selIncomplete` / `slotsWhy` | the selection count / `1` when the module could not walk the whole selection identity (then `known = 0`) / why the slots are unavailable |
| `ex[]` | per configured executor: `{n, empty, tgt, cls, name, kp, ku, fd, lvl, tok, act, rgba, why}`: `tgt = 0` for a Quickey object or an executor reserved by an owned Quickey bank (never a playback target), `lvl` the level of the **configured** fader function (`tok`), `act` `0/1/"?"` |

Sent in the frame `cg`, `known` or `epoch` changes and with the full-state cadence (1000 ms) otherwise. The
welcome carries `context: 1` when the vendored module supports it. The plugin's `display=` argument names the
authoritative encoder bar; it is not discovered.

## 2. Pairing and packet semantics

### Transport

UDP, one JSON object per datagram: at most 512 bytes towards the plugin, at most
1024 bytes from it (the welcome carries key lists). Datagram layout:

```
MTX1 <mac> <json>
```

`MTX1` is the magic and version, `<mac>` is SipHash-2-4 of the `<json>`
bytes under the first 16 bytes of the pairing key, as 16 lower-case hex
characters, and `<json>` is the exact byte string that was authenticated. No
canonicalisation is needed: the MAC covers the bytes as sent.

Why SipHash and not HMAC-SHA256: the plugin computes the MAC in pure Lua
inside onPC, where HMAC-SHA256 was measured at 20 ms per packet (2026-10-09,
onPC 2.5.1 on macOS), enough to collapse the console frame rate at a few
dozen packets per second. SipHash-2-4 is a keyed PRF with a 128-bit key and
a 64-bit tag; forging a tag online is bounded by the per-address throttle
below, and the key never leaves the two configured endpoints. Both
implementations are checked against the SipHash paper's reference vector.

The plugin binds `127.0.0.1:9810` by default (the service on the same PC).
`bind=0.0.0.0` (or an interface address) and an optional `allow=<ip>[,<ip>]`
list open it to the LAN; without `allow`, any source that holds the key is
accepted.

### Authorization

The pairing key is a 32-byte secret configured on both sides (`key=<64 hex>`
at plugin start, `--key` / `--key-file` / `MTPNXK_KEY` for the service); the
MAC uses its first 16 bytes, the rest is reserved. A
packet whose MAC does not verify is dropped before it is parsed; nothing it
contains is trusted, including its sender or session id. **Possession of the
key is the authorization; a sender id is a label.** Per source address, after
50 MAC failures in 10 s the plugin ignores that address for 10 s (bounded
work under a flood). The plugin without a configured key refuses to start.

### Sessions

```
service → plugin   hello   { t:"hello", v:1, id, gen, nonce, surface, fw, held:[...] }
plugin  → service  welcome { t:"welcome", v:1, sid, nonce, gen, lease, hb, keys:{...}, modules, console, seq:0 }
```

- `id` names the surface (`nxk-<serial or host>`), `gen` is a random 32-bit
  value drawn at every service start (the surface generation), `nonce` a
  random 64-bit value per hello.
- `sid` is a fresh random 64-bit session id drawn by the plugin. It is never
  reused and never derived from anything the service sent.
- `welcome.nonce` echoes the hello nonce; the service accepts a welcome only
  for the nonce of its latest hello. A hello whose `nonce` the plugin has seen
  in the last 60 s is dropped (replayed hello cannot restart a session).
- `welcome.gen` is the **plugin generation** (section 5), `lease` the session
  lease in ms (2000), `hb` the heartbeat interval in ms (250). `welcome.keys`
  is `{ ok: [...], unsupported: [...] }`, sorted surface key names; the reason
  for each unsupported key is in the plugin's log (`key X unsupported: …`).
  `welcome.input` is the plugin's input mode (`keyboard`, `fake` or `off`).
- A hello for an `id` with an open session closes that session first (its
  holds are released) and opens a new one: a restarted service never inherits
  holds. The `held` list in the hello is **informational**: those keys are
  physically down at the service, and the plugin does not press them
  (section 3).

### Sequence numbers and acknowledgments

- Every packet after the hello carries `sid` and `seq`. `seq` starts at 1 and
  increases by one per packet, per direction, per session. The receiver keeps
  the last accepted `seq` and drops any packet with `seq` ≤ it. There is no
  reorder window: a reordered press/release pair collapses to nothing (a
  missed tap), never to a stuck key.
- Events (`key`, `wheel`) carry an event id `ev`, monotonic per service start,
  independent of `seq`. A retransmission of an event is a new packet (new
  `seq`) carrying the same `ev`. The plugin deduplicates on `ev` with a window
  of the last 64 ids per session; a duplicate is answered with the **original**
  acknowledgment (a refusal stays a refusal, `dup:1` added) and is never
  dispatched again. An id whose outcome left the window is answered
  `ok:0, code:"outcome-expired"`.
- Event ids order the events of one key: a key event with an `ev` lower than
  the newest one already processed for that key is **superseded** and
  acknowledged `ok:0, code:"superseded"` without dispatch. So a press whose
  first copy was lost cannot execute after its release was processed, and a
  stale release cannot end a newer press. The service also stops
  retransmitting a press once it sends that key's release (counted
  `superseded`); the tap is lost, never late.
- The plugin answers every `key` and `wheel` with
  `ack { t:"ack", sid, seq, ev, ok:0|1, code?, why?, hold? }`. `ok:1` means the
  event was dispatched to the module (for a press: the hold exists and the
  press was dispatched; for a release: the release was dispatched or the hold
  was already released). `ok:0` carries the module's error `code`
  (`unsupported`, `input-disabled`, `conflict`, `exclusive-hold`,
  `route-changed`, `capacity`, …).
- The service retransmits an unacknowledged `key` event up to 4 times at 60 ms
  spacing (the measured ack round trip on onPC is 34 ms median, 53 ms p99), then
  drops it and counts it as lost. `wheel` events are never
  retransmitted (a stale wheel delta is worse than a lost one).
- Everything else (`hb`, `state`, `err`) is unacknowledged.

### Heartbeat, lease and expiry

- The service sends `hb { t:"hb", sid, seq, held:[...] }` every 250 ms. The
  plugin answers `hb { t:"hb", sid, seq, held:[plugin-held], unsynced:[...] }`.
- Every valid packet of a session renews its hardkeys lease (2000 ms). No
  packet for 2000 ms: the hardkeys session lease expires and the module
  releases the session's holds (serviced every frame). No packet for 10 s: the
  session is forgotten; the next packet gets `err { e:"no-session" }` and the
  service sends a new hello.
- Between 2 s and 10 s a packet revives the session (`hb` reply carries
  `resynced:1`); nothing is re-pressed.
- Service watchdog: no plugin packet (hb reply, ack or state) for 1500 ms →
  link down: Link LED blinks, every state item becomes unknown. After 4 s of
  silence, or on `err no-session` for its session, the service pairs again
  (hello every 2 s until a welcome arrives).

### Validation before any module call

A datagram is rejected, counted and (where a session exists) answered with
`err` when: it is longer than 512 bytes; the prefix is not `MTX1`; the MAC
does not verify; the JSON does not parse to an object; `t` is unknown; `sid`
is unknown; `seq` is not an increasing integer; a `key` has no known `k` or
`d` not in {0, 1}; a `wheel` has `w` outside 1..4 or `dx` outside −127..127;
`held` is not a list of strings or longer than 16. Only a packet that passes
every check reaches `hardkeys`/`feedback`.

## 3. Events versus held-key reconciliation

- A `key` with `d:1` is a **press**: the plugin opens a hold through
  `hardkeys:press(session, now, { key = <logical> })` (never exclusive, never
  a tap: the surface decides how long the key stays down). A `key` with `d:0`
  is a **release** of the hold the plugin recorded for that surface key of
  that session; a release for a key that is not held is acknowledged `ok:1`
  with `noop:1` and dispatches nothing.
- Deduplication: duplicate `ev` → acknowledged, not dispatched. Duplicate
  press without a duplicate `ev` (the surface says "down" again while the
  plugin holds the key) → acknowledged `ok:1, duplicate:1`, nothing is
  injected (the module's own duplicate rule).
- Releases are accepted only for the session that owns the hold; the module
  enforces ownership (`not-owner` for anyone else).
- **Heartbeats renew leases and reconcile releases only.** For every key the
  plugin holds for the session that is absent from `hb.held`, the plugin
  releases the hold (a lost release). For every key in `hb.held` that the
  plugin does not hold, the plugin reports it in `unsynced` and does **not**
  press it: a press that never arrived is a lost event, retransmission covers
  it, and pressing late would land in a context the operator has moved past.
  A hold whose physical release the service observed is therefore never
  re-pressed from a heartbeat, and a tap is never replayed: taps are two
  events with their own ids, and ids are never regenerated.
- **Unresolved releases survive a restart.** A release the module could not
  confirm (refused, raised, or the route changed during the hold) is kept as
  a record when the plugin stops, is detached after a `service()` error, or
  is cleaned up by the console. The next start **adopts** those records
  before enabling input: their key stays reserved (a new press of it is
  refused with `conflict`) until the operator runs
  `Plugin "mtpnxk_surface" "recover"`, which re-attempts the releases through
  the current backend (attached for cleanup only when input is off) and
  reports what is still unresolved. Records the module rejects at adoption are
  kept for the next start; `status` lists both. If `dispose()` itself raises,
  the instance is **quarantined** with its records instead of dropped, input
  stays blocked at the next start, and `recover` first re-attempts the
  releases on that instance, exports its records, adopts them and only then
  re-enables the input mode requested at start.
- **Restart.** After a plugin restart every session is gone (new `sid`, new
  plugin generation). After a service restart the hello carries a new
  surface `gen` and the keys physically down in `held`; the plugin opens a
  fresh session and holds nothing until it receives press events. Keys that
  were down across the restart stay unpressed on the console until released
  and pressed again. Remembered holds are never turned into presses.
- Capacity: the hardkeys instance allows 12 simultaneous holds; the 13th press
  is acknowledged `ok:0, code:"capacity"` and nothing is dispatched.

## 4. Loop budget

The plugin loop runs inside the `Plugin` call and yields once per console
frame (the KB-02/KB-03 pattern; a Timer hand-off would be cleaned up by
onPC). Per iteration, in this order:

1. `hardkeys:service(now)`: lease expiry, due releases (at most
   `maxWorkPerService` = 4 attempts), console-state snapshot.
2. `feedback:service(now)`: identity check (≤ 1/s) and at most 4 watched
   reads (`maxReadsPerService`), round robin, each item at most every 100 ms.
   The NX-K watch list has 9 items, so a full pass takes 3 frames and
   feedback never costs a frame more than 4 reads.
3. Receive up to 32 datagrams (`receivefrom` with timeout 0), validate each
   (section 2) and dispatch. The rest wait in the socket buffer; the OS drops
   when it is full. Per session at most 400 packets/s are processed; beyond
   that packets are dropped and counted (`dropped.rate`).
4. Send what the iteration produced (acks, hb replies) and, if due, a `state`
   delta (on change) or the full state (every 1000 ms).
5. `coroutine.yield()`.

Deadline servicing comes first so a packet flood cannot starve a due release
or a lease expiry. The harness checks that a flood of 10 000 packets in one
iteration still releases a due hold and expires a lapsed lease on that
iteration, and that a stalled feedback reader (one that raises or returns
nothing) costs one unavailable item, not the loop.

## 5. Feedback freshness and generations

- **Plugin generation** `gen` (in `welcome` and every `state`) is
  `<start counter>-<random 32-bit>`, drawn at every plugin start. The
  feedback module's `epoch` restarts at 1 for every instance, so it cannot
  identify a plugin run by itself; the pair (gen, epoch) can. The service
  keys its freshness on that pair: a change of either means every cached
  item is unknown until the next full state.
- Every `state` carries `gen`, `epoch`, `full` (1 for a complete state, 0 for
  a delta of changed items) and `s` (the items). A delta is sent in the frame
  an item changes; the full state every 1000 ms. The service applies a delta
  only when (gen, epoch) match its current full state; otherwise it waits for
  the next full state.
- Stale on the plugin side: an item whose observation is older than 2000 ms,
  or any item while the identity is uncertain, is sent as `"?"`. Stale on the
  service side: no `state` for 1500 ms → all unknown.
- The surface's own holds (what it pressed) are kept apart from console
  state. `ma` is the console's aggregate MA state and is rendered as such;
  which keys the surface holds comes from its own records and the `hb` reply.

Rendering unknown on the NX-K (single-colour LEDs, no dim level): unknown is
off, and the Link LED carries the aggregate: off = paired and fresh, slow
blink (1 Hz) = link down or feedback unknown. The ambiguity between "off" and
"unknown" for a single key is accepted on this surface and documented; the
M-Touch's colours can show it per key later.

## 6. Exclusive use

The surface plugin and the MCP bridge each own a hardkeys instance with its
own ownership records. **Those instances do not arbitrate the shared console
keyboard.** Both inject into the same key state, and so does the physical
keyboard; an interaction lock in one plugin does not stop the other.

Operating rule: run only one of them with input enabled at a time. The surface
plugin refuses to enable input while `_G.__gma3_mcp_bridge.input.enabled` is
true and says so (start with `force` to override); it publishes
`_G.__mtpnxk_surface = { running, inputEnabled, sid }` so the bridge can do
the same in a later version. This is a courtesy check in one Lua state, not a
lock: it cannot see a physical operator, another surface or a bridge that is
started after the check.

## 7. Key mapping (NX-K)

The service sends the NX-K control names from the decoder. The plugin maps
them to logical keys of the hardkeys module (resolved at start through the
user profile's shortcut table; unresolved keys are reported in
`welcome.keys` and acknowledged `unsupported`):

| NX-K | Logical key | Route on the default profile (KB-01) |
| --- | --- | --- |
| `0`–`9` | `NUM0`–`NUM9` | shortcut `0`–`9` |
| `Enter` | `PLEASE` | native Enter redirect |
| `Record` | `STORE` | shortcut `S` |
| `Clear` | `CLEAR` | shortcut `Delete` |
| `Undo` | `OOPS` | shortcut `Backspace` |
| `Update`, `Edit`, `Copy`, `Move`, `Delete`, `Load`, `Cue`, `Group`, `Macro`, `Fade`, `Delay`, `HighLight`, `Preview`, `Next`, `Last`, `Menu`, `Snap Shot`, `Thru`, `Full`, `@`, `+`, `-`, `.`, `/`, `Back` | the `Enums.VirtualKeyCode` name in the plugin's table (`UPDATE`, `EDIT`, … `PREV` for Last, `SNAPSHOT`, `THRU`, `FULL`, `AT`, `PLUS`, `MINUS`, `DOT`, `SLASH`, `BACKSPACE`) | **unverified**: resolved through the shortcut table if the profile maps them; otherwise reported unsupported. The names are checked against the console's enum at start; a wrong name is reported, never guessed. |
| `Bank`, `Rotary1`–`Rotary4` (turn and press), `Swap Prog`, `Link` | none | wheels are carried as `wheel` events and acknowledged `unsupported` until a verified Lua route for encoder input exists; Bank is the service's wheel modifier; Link is the link LED |

Hardkeys 0.5.0 resolves any `Enums.VirtualKeyCode` name through the shortcut
table (the fixed `MA` and native `PLEASE` routes stay special), and lets the
consumer name the row of a same-target tie (`spec.prefer`): the default
profile maps `+ - . /` on the main row and the keypad, and the plugin prefers
the keypad row (`kpAdd`, `kpSubtract`, `kpDecimal`, `kpDivide`) through the
normal route, so shortcut enablement, the collision check and the rechecks
before every release all apply. Both extensions were made in the MCP
repository (PR #12) and re-vendored, not forked here. Fade, Delay, Snap Shot
and Back are not MA3 hardkeys and stay unsupported; Load, Macro and Thru need
a shortcut in the user profile.

## 8. Measurable limits and their status

Measured on the target setup (service on the onPC machine or on the LAN,
onPC 2.5.1.0), under load = 10 taps/s from the service plus the full NX-K
watch list. Report median, p95, p99 and the worst observation, never only a
mean. (Chosen before measuring at 20 taps/s and revised after the first live
run, [probes/kb-07-live-macos-2.5.1.md](probes/kb-07-live-macos-2.5.1.md):
the console frame of about 20 ms sets a floor of two frames for a round trip,
and the vendored module's shortcut-table re-read before every press and
release costs about 7 ms per event on this console, so 20 taps/s saturates
the plugin frame; human keypad use stays below 5 events/s.)

| Measure | How measured | Limit | Status (KB-08, 2026-10-09) |
| --- | --- | --- | --- |
| Press latency, plugin dispatch → console effect | plugin `bench` mode polls `cmdtext` after a `NUM` press and reports the frame it changed (`effect` packet); timed in the plugin from its dispatch, so the service→plugin leg is excluded | median ≤ 60 ms, p99 ≤ 120 ms, worst ≤ 250 ms at 10 taps/s | median **met** (51 ms); p99 **not demonstrated**: 156 ms, n=359. The tail follows the tap interval and is attributed to the instrument's snapshot order (defect D1 in [kb-08-acceptance.md](kb-08-acceptance.md)); at 1 tap/s p99 is 53 ms with the affected taps reported as unmeasured |
| Ack round trip | event sent → ack received, at the service | median ≤ 50 ms, p99 ≤ 100 ms | **met**: 33.5 / 53.3 ms (n=798 at 10 taps/s) |
| Lost-connection cleanup | service stops sending (simulated cable pull); the plugin releases every hold of the session when the lease expires | ≤ lease (2000 ms) + 1 frame; p99 ≤ 2100 ms | **met**: 1.99 s after the last packet (one sample, 50 ms poll) |
| LED freshness | a console change (Highlight toggled on the console) → LED write at the service | median ≤ 250 ms, p99 ≤ 500 ms | **met**: ≈ 126 ms median, 194 ms worst (n=8, upper bounds including the command's own delivery) |
| Stale indication | plugin stopped → Link LED blinking | ≤ 1500 ms | **met**: watchdog at 1.5 s, Link LED write observed (KB-08), physical LED observed (KB-07) |
| Flood resilience | 2000 packets/s of unauthenticated datagrams at the plugin for 10 s | ack round trip p99 stays ≤ 150 ms; no hold outlives its release by more than 1 frame | **not met**: from the service's own address the throttle locks the service out for 10 s; from another address the 32-datagram read budget per iteration lets the kernel queue delay legitimate packets (98 of 195 events lost). Defect D2, owned by KB-07 |

Measurements, procedure and the honest tail analysis are in
[probes/kb-08-qualification-macos-2.5.1.md](probes/kb-08-qualification-macos-2.5.1.md);
the first run at a smaller sample is [probes/kb-07-live-macos-2.5.1.md](probes/kb-07-live-macos-2.5.1.md).

The harness proves the logic (lost releases, reordering, duplicates, floods,
feedback stalls, restarts, capacity, two consumers); the limits above are
measured live with `mtpnxk bench` and the plugin's `bench` start option, and the
numbers are recorded in `docs/probes/`. A limit that is not met is reported as
such, never trimmed into a pass.

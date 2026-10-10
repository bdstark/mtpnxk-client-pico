# Operator guide: setup, operation and recovery

For the person running an NX-K against grandMA3 onPC with this repository's
surface plugin and service. Qualified scope: [deployments.md](deployments.md)
(macOS, onPC 2.5.1.0, same machine, US layout, beta). Design and protocol:
[surface-protocol.md](surface-protocol.md).

## What you install

| Component | File(s) | Goes where |
| --- | --- | --- |
| Surface plugin entry component | `tools/ma3/mtpnxk_surface.lua` | the onPC user library plugins folder |
| Vendored console modules (two) | `tools/ma3/gma3_mcp_hardkeys.lua` (0.10.0), `tools/ma3/gma3_mcp_feedback.lua` (0.2.0) | same folder; they are components of the same plugin |
| Import wrapper | `tools/ma3/mtpnxk_surface.xml` | same folder |
| Surface service | `service/` → `mtpnxk` binary | the machine the NX-K is plugged into |

The plugin is one onPC plugin with three components. All three `.lua` files and
the XML must be copied together; the plugin checks the modules' versions at
start and refuses to run with a mismatch. Check the files against the hashes in
[tools/ma3/VENDOR.md](../tools/ma3/VENDOR.md) when packaging.

### 1. Build the service

```bash
cd service && cargo build --release
```

The binary is `service/target/release/mtpnxk`. `mtpnxk list` prints the USB
devices it can see; the NX-K is `11be:e102`.

### 2. Install the plugin in onPC

macOS plugins folder: `~/MALightingTechnology/gma3_library/datapools/plugins`.
Copy the three `.lua` files and the XML there, then in the onPC command line,
with a free Plugin pool slot (example: slot 2):

```text
Import Plugin Library "mtpnxk_surface.xml" At Plugin 2
```

Save the show to keep the imported plugin. The plugin must be started again
after every show load.

### 3. Make a pairing key

The pairing key is a 32-byte secret shared by the plugin and the service. It is
the authorization: whoever holds it can send key events to the console.

```bash
./service/target/release/mtpnxk keygen > ~/.mtpnxk.key
```

Keep the file private. The service reads it with `--key-file`; the plugin takes
the same 64 hex characters as `key=…` in its start command. Generate a new key
to revoke an old one (restart both sides).

### 4. Start the plugin (normal case)

```text
Plugin "mtpnxk_surface" "key=<64 hex>"
```

Defaults: listens on `127.0.0.1:9810` (service on the same machine), input on
the keyboard backend, display 1. The start log (System Monitor / command-line
history) lists every NX-K key with the method and backend that would press it
(`key 1: NUM1 via shortcut (keyboard, shortcut-table)`), ending with
`keys: 32 supported, 12 unsupported` on the default US profile, then
`listening on 127.0.0.1:9810 (v0.2.0, protocol 1, gen …)`.

### 4a. Quickey dispatch (KB-15, the preferred path once set up)

```text
Plugin "mtpnxk_surface" "key=<64 hex> bank=900/1.178-189 input=mixed"
```

`bank=<quickey>/<page>.<first>-<last>` is the operator's decision to create show
objects: one Quickey per hardkey code with KB-10 evidence (nine on onPC 2.5.1:
`MA1 FIXTURE STORE NUM1 NUM5 THRU PLEASE OOPS CLEAR`, names `MCP <CODE>`, from
pool index 900) plus a code-less `MCP RESERVED` placeholder, and the executor
range reserved for holds (12 executors, one per concurrently held key; the
range must be empty, an unowned object anywhere in it refuses the whole setup
and nothing is created). The bank is verified at every start, kept as a record
across `stop`, re-verified by `bank verify`, shown by `bank status`, and removed
only by `bank teardown` (refused while a Quickey is held). The show must be
saved for the objects to survive a reload.

`input=mixed` presses the keys with hold evidence (`1`, `5`, `Thru`, `Enter`,
`Record`, `Clear`) as executor presses of their Quickeys and every other key
through the shortcut table as before; the start log names the part per key.
`input=quickey` uses Quickeys only (the other keys are reported unsupported).
Rules the module enforces, reported to the service as refusals and never
worked around: a Quickey route without a bank or for a code without evidence is
refused, nothing is typed instead; a shortcut-table key while a Quickey is held,
or the reverse, is refused (`unqualified-mix`); so is a second Quickey next to
one whose code has no chord evidence. `Thru` becomes usable this way on the
default profile (it has no shortcut row).

`route=<key>:<method>,…` overrides single keys (`quickkey` or `shortcut`); an
override the backend cannot serve, or that resolves to no usable route, refuses
the start instead of being accepted inert. `bankcodes=hardkeys` provisions every
command-area code (94 Quickeys) instead of the qualified nine.

Options, appended to the same quoted string:

| Option | Meaning |
| --- | --- |
| `port=9811` | another UDP port |
| `bind=0.0.0.0 allow=192.168.1.20` | accept the service from the LAN; `allow` is a comma-separated list of source addresses (without it any source holding the key is accepted). Unqualified deployment, see deployments.md |
| `input=mixed`, `input=quickey` | Quickey dispatch through the bank (section 4a); need `bank=` once |
| `bank=900/1.178-189`, `bankcodes=…`, `route=…` | section 4a |
| `input=off` | feedback only: LEDs follow the console, no key is ever pressed |
| `input=fake` | lifecycle testing: events are recorded and acknowledged, no key is pressed |
| `bench` | report press-to-effect timing for digit taps (measurement only) |
| `force` | start even though the MCP bridge reports input enabled (see "Exclusive input") |

### 5. Start the service

With the NX-K plugged in:

```bash
./service/target/release/mtpnxk --key-file ~/.mtpnxk.key run
```

`--plugin host:port` points at a plugin that is not on `127.0.0.1:9810`;
`--verbose` prints pairing, refusals and losses; `--id` names the surface in the
plugin's log (default `nxk-<hostname>`). The service pairs within a frame of
starting, keeps pairing every 2 s while the plugin is down, and reopens the
keypad if it is unplugged and plugged back in.

Without hardware, `mtpnxk sim --script "Record:down,5:tap@50,Record:up@300"`
plays a key script and echoes the LED writes it would make.

### USB notes

- macOS: nothing to install. The keypad enumerates unconfigured; the service
  selects configuration 1 before claiming the interface.
- Windows: bind the **WinUSB** driver to the NX-K with Zadig before `run`
  (unqualified).
- Linux: a udev rule for `11be:e102` for non-root access (unqualified).

## Supported and unmapped keys (default US profile, onPC 2.5.1.0)

| Resolved (32) | How |
| --- | --- |
| `0`–`9`, `Enter`, `Record`, `Clear`, `Undo`, `Update`, `Edit`, `Copy`, `Move`, `Delete`, `Cue`, `Group`, `HighLight`, `Preview`, `Next`, `Last`, `Menu`, `Full`, `@` | the profile's shortcut table (Enter through the native Please route) |
| `+`, `-`, `.`, `/` | the keypad row of the shortcut table (`kpAdd`, `kpSubtract`, `kpDecimal`, `kpDivide`) |

| Unsupported (12) | Why | What you can do |
| --- | --- | --- |
| `Load`, `Macro`, `Thru` | no keyboard shortcut maps to them in the default profile | add a shortcut for `LOAD`, `MACRO`, `THRU` in your user profile; the plugin resolves them at its next start. KB-09 (MCP repository) plans a guided way to do this |
| `Fade`, `Delay`, `Snap Shot`, `Back` | not grandMA3 hardkeys (not in `Enums.VirtualKeyCode`) | nothing; they stay unsupported |
| `Bank`, `Rotary1`–`Rotary4`, `Swap Prog`, `Link` | not console keys: Bank is the encoder modifier, rotaries are wheels (no verified Lua route for encoder input), Link is the link LED | encoders stay unsupported until a route is verified (separate work) |

The plugin never guesses a route: a key that does not resolve is reported at
start and every event for it is acknowledged `unsupported`.

## Exclusive input

The surface plugin and the MCP bridge both inject into the same console
keyboard; neither can stop the other or a physical operator. Run only one of
them with input enabled at a time. The plugin refuses to enable input while the
bridge reports input enabled and says so in its log; stop the bridge's input (or
start the bridge without input) rather than reaching for `force`. `force` is for
measurement sessions where the bridge is known to hold no keys, as in the
qualification records.

## Status and stop

```text
Plugin "mtpnxk_surface" "status"
Plugin "mtpnxk_surface" "stop"
```

`status` prints running state, binding, input mode, generation, every session
with its held keys, counters, and any unresolved release records. `stop`
releases every key the plugin holds, closes the sessions and keeps any release
it could not confirm as a record for the next start. The service notices a
stopped plugin within 1.5 s: the Link LED blinks and every other LED goes off
(unknown), and it pairs again on its own when the plugin comes back.

## LEDs

Off means "not active" **or** "unknown": the NX-K has single-colour LEDs. The
Link LED carries the difference: off while paired with fresh console state,
slow blink while unpaired, link down or state stale. Keyword keys (Record,
Update, Edit, …) follow the pending command-line keyword, HighLight blinks while
highlight mode is on, Clear/Undo/Next/Last/Menu light while held, Bank lights
itself and the four encoder LEDs while held.

## Recovery

A release the console could not confirm (the module refused it, raised, or the
key's route changed while it was held) is kept as a **record**. Records survive
`stop`, a loop error and the console's Cleanup; the next start **adopts** them
before enabling input and keeps the affected key reserved, so a new press of it
is refused with `conflict` until you act.

1. Read the start log or `status`: adopted records are listed
   (`adopted unresolved record …: its key stays reserved until "recover"
   releases it`).
2. Check the console: if the key is visibly stuck, press and release it on the
   physical keyboard once.
3. Run

   ```text
   Plugin "mtpnxk_surface" "recover"
   ```

   It re-attempts every unresolved release through the current backend (attached
   for cleanup only when input is off) and logs
   `recover: N released, M still unresolved, K record(s) not adopted`.

4. **If something stays unresolved**: the log names the key, the session and the
   module's error. Common causes are shortcuts turned off (`shortcutsActive=0`
   on the Link LED pattern; turn ShCuts back on and run `recover` again), a
   profile or user change that removed the key's shortcut (restore it, or press
   the key physically), or the module raising. A record the module rejects at
   adoption is kept for the next start and listed by `status`; it is not
   dropped. Nothing is ever retried automatically and no press is replayed.
5. If `stop` itself reported `dispose failed … the instance is quarantined`,
   input stays blocked at the next start until `recover` exports the
   quarantined records; the same command then re-enables the input mode you
   asked for at start.

After a show load the plugin is not running; start it again and the kept
records (if any) are adopted as above.

## Updating

1. Copy the new `.lua` files and the XML into the plugins folder (all three
   `.lua` files, even if only one changed: the versions are checked together).
2. In onPC, with the plugin in slot 2:

   ```text
   Plugin "mtpnxk_surface" "stop"
   Delete Plugin 2 /NoConfirmation
   Import Plugin Library "mtpnxk_surface.xml" At Plugin 2
   ReloadAllPlugins
   Plugin "mtpnxk_surface" "key=<64 hex>"
   ```

   `ReloadAllPlugins` is needed on 2.5.1: delete and re-import alone can keep
   the cached Lua running (MCP KB-02). It reloads every plugin, so pick the
   moment. The reload kills the running loop without its shutdown; since 0.2.0
   the next start notices the stale run (`has not ticked for …; taking over its
   state`), releases its holds through the old module instances, keeps their
   records and the bank record, and continues. Check the version in the
   `listening on …` line, then save the show.
3. Rebuild the service (`cargo build --release`) and restart `run`. The service
   logs the plugin's module versions in `paired: …` and in `welcome.modules`.
4. Re-vendoring the console modules is described in
   [tools/ma3/VENDOR.md](../tools/ma3/VENDOR.md); never edit them in place.

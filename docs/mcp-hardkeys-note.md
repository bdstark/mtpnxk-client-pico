# Note for bdstark/GrandMA3MCP: hardkey and keyboard ops

Drafted 2026-10-09 in mtpnxk-client-pico (`docs/handoff.md` has the
context). Move this file into the MCP repo and turn it into an issue or a
design doc there.

## Why

The bridge runs complete commands through `Cmd()`. It cannot do what a
person at the console does between commands: hold Store or MA while
pressing something else, hit Please, Clear, Oops or Esc, or type into a
pop-up. Two consumers want that:

1. **MCP clients.** Agents testing or demonstrating console behaviour,
   driving interactive flows (store pop-ups, pending keywords, dialogs)
   instead of bypassing them with `Cmd()`.
2. **mtpnxk surfaces.** A Pico that hosts an NX-K keypad (later M-Touch and
   M-Play) and talks to a small Lua plugin over UDP. Its keypad is all
   hardkeys. It also needs console state for LED feedback. It will
   `require` the module described here; it will not share the bridge
   process.

Nothing in grandMA3 presses a console hardkey from outside: OSC `/Key`
addresses executors (and is reported dead on onPC 2.4.2.2), DMX and MIDI
remotes target executors and objects. The two in-application mechanisms
are below.

## Mechanisms to implement, in probe order

### 1. `Keyboard()` (probe first)

Undocumented object-free function present in the community API dump
(grandma3.bambinito.net, v2.3):

```
Keyboard(display_index, type, char_or_keycode, shift, ctrl, alt, numlock)
  type: 'press' | 'char' | 'release'
```

Injects keyboard events inside the application, so OS window focus is
irrelevant. Unknowns: key code names (`HelpLua` on the console exports the
real function list), whether `'press'` with a hardkey code behaves like
the key (hold semantics, Store pop-up on long press), whether `'char'`
lands on the command line when an input field has focus. No published use
presses Store; the MA forum thread on it (9022) ends unresolved.

Probe plan on onPC 2.5.1, in the live test harness:

1. `Keyboard(1,'char','5')`: does `5` appear on the command line?
2. `Keyboard(1,'press','Please')` then `'release'` (name from `HelpLua`):
   does it execute the line?
3. `'press'` Store, hold 1 s, `'release'`: pop-up or not?
4. With a pop-up open, does `'char'` type into it; does Esc close it?

If 1 and 2 work the keypad path is `Keyboard()` alone. If 3 works the
Quickey pool is unnecessary.

### 2. Quickey pool (proven pattern)

A Quickey's KeyCode can be any hardkey (Store, Please, Thru, Clear, digits,
MA, ...). riksolo's RBOSCKeys plugin proves the pattern: a pool of blank
Quickeys assigned to executor buttons on a parked page; on press, take a
free slot, set its KeyCode, press its executor; on release, release the
executor and free the slot. True press, hold and release, N keys held at
once.

Design for the module:

- `hardkeys.init{quickey_from=..., count=N, exec_page=..., exec_from=...}`
  reserves and caches the slots once per plugin start; refuses to overwrite
  non-empty Quickeys or executors.
- `hardkeys.press(code)` / `hardkeys.release(code)`; `hardkeys.tap(code)`;
  `hardkeys.release_all()` on stop and in `Cleanup`.
- Open question to settle first: **how to press and release the executor
  button from Lua**. RBOSCKeys drives it with OSC `/Page/Key`, which we do
  not have. Candidates: `Cmd()` with the button's key function, the
  executor object's methods, or assigning the Quickey directly and using
  whatever `Keyboard()` turns out to support. Dump an executor and a
  Quickey object to see what is callable.

### 3. State readers (for feedback)

Also in the module, because the MCP `programmer` op already does part of
it and the surface plugin needs the rest: pending command-line keyword
(`CmdObj()` text, property name unconfirmed), programmer modes (Blind,
Highlight, Preview, Freeze, Solo: no documented getters, find via
`Dump()`), current page, executor active/level. Details and the message
schema the surface uses: mtpnxk `docs/ma3-feedback.md`.

## Bridge ops to add

| op | args | notes |
|----|------|-------|
| `hardkey` | `code`, `action: press/release/tap`, optional `hold_ms` for tap | via `Keyboard()` if the probe passes, else the pool |
| `type` | `text` | `Keyboard(...,'char',c)` per character |
| `hardkeys_status` | | slots in use, mechanism in effect |

Gate them like `lua`: off by default, enabled at plugin start, since a
held Store changes what the next command does.

## Packaging

Ship `hardkeys.lua` as a second `ComponentLua` in `gma3_mcp_bridge.xml`
and `require` it from the bridge. The mtpnxk surface plugin copies the
same file into its own package. Keep the module free of bridge state so
both can load it.

## Non-goals

- The bridge stays loopback-only and single-coroutine. The surface's
  LAN-facing UDP listener lives in its own plugin (security boundary,
  latency isolation, separate release cadence).
- No OSC. The surface uses a LuaSocket UDP server after the bridge's
  `settimeout(0)` + `coroutine.yield()` loop.

# grandMA3 LED feedback (design draft, 2026-10-08)

What the surface LEDs should show when the board drives grandMA3 onPC, and
how the state gets from MA3 back to the Pico. Nothing here is implemented
yet; the firmware can write NX-K LEDs (README "NX-K LEDs") but has no
receive path. The plugin skeleton is `tools/ma3/SurfaceFeedback.lua`.

## What MA3 itself does with key LEDs

From the 2.5 manual (help.malighting.com, researched 2026-10-08):

- **The manual does not describe per-key LED behaviour.** None of the 80
  key pages mentions an LED. The only inventory is the Lua `SetLED` page
  (`lua_objectfree_setled.html`), which lists every Master Module key with
  an LED, among them Blind, Highlight, Preview, Freeze, Solo, Store, Update,
  Edit, Copy, Move, Delete, Cue, Group, Time, Learn, Go, GoBack, Pause,
  Select, Menu, Next, Prev, XKeys and the executor buttons. Its note that
  "after around two seconds, the system automatically sets the LED values
  to what it believes it should be" confirms MA drives them from state, but
  not which state.
- Desk Lights has three levels: LED Background (idle keys), LED Feedback
  ("respond to input") and an LED Master. So MA keys sit dim and go bright,
  rather than off/on. The NX-K has no dim level, so "off" stands in for
  background.
- The on-screen status icons for Highlight, Lowlight, Solo and Blind blink
  by default; Preview does not. That the key LEDs follow them is likely
  but **unconfirmed**; check against a console or a command wing.
- **Unconfirmed, needs a console:** whether Store/Update/etc. light while
  pending on the command line, whether Clear lights with programmer
  content, and what Learn and Pause do.

onPC drives LEDs only on MA hardware (`SetLED` takes a module from
`Root().UsbNotifier.MA3Modules`). A third-party surface gets nothing for
free: the state has to be read in Lua and sent out.

## Feedback path

| option | MA3 side | Pico side | verdict |
|--------|----------|-----------|---------|
| Native OSC Send | "Object Playback Feedback": playback events as enumerated addresses (`/13.13.1.6.1,sif,...`) | parse unstable numeric addresses | events only, no state dump, no programmer modes; not enough alone |
| **Lua plugin + `SendOSC`** | plugin polls state, `Cmd('SendOSC 1 "/sfb/...,i,1"')` on change | UDP listener, state model, per-surface LED map | **recommended**; the pattern every existing MA3 feedback project uses (ArtGateOne/MA3_OSC_FEEDBACK, xxpasixx/pam-osc) |
| Lua plugin + `SendMIDI` | same plugin, `SendMIDI "Note" ch/n vel` | Pico enumerates as USB MIDI too | works over the cable with no network, but 7-bit values, one MIDI port mode to manage, awkward for RGB bars and page numbers |
| HID output reports | none; MA3 never writes keyboard LEDs | | not possible |
| Serial (CDC) | Lua has no serial access | | not possible |

**Recommendation: OSC over UDP, with one message schema used by every
surface.** Wi-Fi carries it today. CDC-NCM on the device port (already on
the roadmap) carries the same UDP over the cable later without touching the
plugin. Only the destination IP changes, and macOS and Windows 11 both ship
an NCM driver. MIDI stays a fallback if NCM falls through. The schema
below maps directly onto MIDI notes if needed.

Two other design rules:

- **The plugin sends console state, never LED values.** It does not know
  which surface is attached. The firmware owns one map table per surface
  (NX-K, M-Touch, M-Play) from state to control address and LED value.
  Adding a surface means firmware work only.
- **State is refreshed, not just updated.** The plugin sends deltas as
  they happen and the full state every 2 s. A rebooted Pico or a dropped
  UDP packet heals within 2 s without needing a request channel back into
  MA3. A missing refresh for 5 s means the link is down.

### Message schema (plugin -> Pico)

Leave the OSC line's Prefix empty or set it to match the firmware. MA3
prepends it to every `SendOSC` address.

| address | args | meaning |
|---------|------|---------|
| `/sfb/hello` | s version | plugin started |
| `/sfb/alive` | i seq | keepalive, once per refresh |
| `/sfb/mode/<m>` | i 0/1 | `blind`, `highlight`, `preview`, `freeze`, `solo` (getters unconfirmed, see the plugin) |
| `/sfb/pending` | s keyword | command-line keyword awaiting input (`store`, `update`, `edit`, `copy`, `move`, `delete`, `load`, `cue`, `group`, `macro`, `fade`, `delay`, ...) or `none` |
| `/sfb/page` | i n | current executor page (M-Touch/M-Play displays) |
| `/sfb/exec/<n>` | i 0/1 | executor n on the current page has an active playback |
| `/sfb/fader/<n>` | i 0-100 | its master fader level (M-Touch/M-Play bars) |

`/sfb/pending` keeps the word only, never the command text, because
quoting arbitrary text through `Cmd('SendOSC ...')` is fragile.

## NX-K map

Addresses are `group << 8 | control` from `src/nxk_decode.c`. **Plugin**
means the LED follows a `/sfb/...` message. **Local** means the firmware
decides on its own, which also covers running with no plugin.

| button | id | sends today (route.c) | LED | source |
|--------|----|----------------------|-----|--------|
| Record | 0x5401 | `/key/Store` | on while `pending == store` | plugin; local fallback below |
| Update | 0x5402 | `/cmd Update ` | on while `pending == update` | plugin |
| Edit | 0x5101 | `/cmd Edit ` | on while `pending == edit` | plugin |
| Copy | 0x5104 | `/cmd Copy ` | on while `pending == copy` | plugin |
| Move | 0x5106 | `/cmd Move ` | on while `pending == move` | plugin |
| Delete | 0x5107 | `/cmd Delete ` | on while `pending == delete` | plugin |
| Load | 0x5411 | `/cmd Load ` | on while `pending == load` | plugin |
| Cue | 0x5413 | `/key/Cue` | on while `pending == cue` | plugin |
| Group | 0x5412 | `/key/Group` | on while `pending == group` | plugin |
| Macro | 0x2001 | `/cmd Macro ` | on while `pending == macro` | plugin |
| Fade | 0x4321 | `/cmd Fade ` | on while `pending == fade` | plugin |
| Delay | 0x4322 | `/cmd Delay ` | on while `pending == delay` | plugin |
| HighLight | 0x6001 | `/cmd Highlight` | blink while highlight on (matches MA's status blink) | plugin |
| Preview | 0x2002 | `/cmd Preview` | on while preview on | plugin |
| Clear | 0x5103 | `/cmd Macro 9950` | on while the programmer has values, if a getter exists; else echo | plugin / local |
| Undo | 0x5102 | `/cmd Undo` | echo while held | local |
| Next | 0x6402 | `/cmd Next` | echo while held | local |
| Last | 0x6401 | `/cmd Last` | echo while held | local |
| Menu | 0x2003 | `/cmd Menu` | echo while held | local |
| Snap Shot | 0x4331 | `/cmd Snapshot` | echo while held | local |
| Bank | 0x4332 | wheel modifier | on while held | local |
| Encoders 1-4 | 0x5901, 0x5911, 0x5921, 0x5931 | `/wheel/N`, `/key/WheelN` | on while Bank is held (wheels 101-104 active) | local |
| Link (red) | 0x6108 | unmapped | off when the plugin is alive, slow blink after 5 s without `/sfb/alive` | local |
| Swap Prog | 0x6411 | unmapped | off | |
| Back | 0x5215 | Backspace (HID) | probably none; check with `L5215 0001` | |

Digits, `.`, `/`, `-`, `+`, `@`, Enter, Thru and Full have no LED.

**Local fallback for pending keywords**, used while the plugin is not alive:
light a keyword's LED on its press, then clear all keyword LEDs on Enter,
Clear, another keyword (switch to that one) or after 10 s. This approximates
MA well enough for a keypad. When the plugin is alive its `pending` value
wins.

## M-Touch and M-Play (same schema, later)

- Playback strip buttons and MF/pressure buttons: `/sfb/exec/<n>` (red =
  off but assigned, green = running, once the plugin can tell "assigned"
  from "empty").
- Fader bars: `/sfb/fader/<n>` as the bar height, in the executor's colour
  when the plugin adds `/sfb/color/<n>`.
- Page displays: `/sfb/page`.
- Go (green) 0x5513, Pause (red) 0x5512: running/paused state of the
  selected sequence; Select, Rel, Snap, Beat as on the NX-K (echo, or
  Learn tempo for Beat if MA exposes it).
- HighLight, Edit, Record, Update, Load, Clear, Last, Next: same rules as
  the NX-K rows above, using the M-Touch IDs (several are the same
  addresses).

## Open questions, in order

1. On the onPC, run the plugin's `probe` to find where Blind, Highlight,
   Preview, Freeze and Solo state lives (`CmdObj():Dump()`,
   `Programmer():Dump()`). If there is no getter, the firmware tracks
   Highlight and Preview locally from its own presses and accepts drift
   when they are toggled on screen.
2. Confirm `CmdObj().cmdtext` (or whatever holds the command line) for
   `pending`.
3. Check whether `Cmd('SendOSC ...')` from a plugin clutters the command
   line history. If it does, drop the refresh rate or find a quieter call.
4. Firmware: OSC receive in `osc.c` (lwIP UDP bind on the configured port),
   a state model, the NX-K map above, and the Link watchdog.

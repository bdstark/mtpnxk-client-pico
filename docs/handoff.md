# State of play, 2026-10-08 (updated after the LED session and the Lua decision)

Written at the end of the first bring-up session so the next one can start at
the bench without re-deriving anything. Read the README first; this is what
the README does not say.

## Bench as it stands

- **Pico W** on **bdsbrxi501** (192.168.75.166, Linux), console at
  `/dev/ttyACM0`. Images are staged under `~/mtpnxk/` there
  (`mtpnxk-standalone.uf2` = normal build, `mtpnxk-debug.uf2` = TinyUSB log
  to console). Build on the Mac (toolchain under `~/Development/embedded/`),
  copy the UF2 over with scp, then reflash on the Linux box with the
  picotool there (`~/.pico-sdk/picotool/2.2.0-a4/picotool/picotool`):
  `reboot -u --vid 11914 --pid 4174 -f` under sudo, wait two seconds, then
  `load -x` of the UF2. That picotool is 2.2.0-a4: the device-selection
  flags only work on `reboot`, and only after `-u`; sudo is needed because
  the udev rule there does not cover product id 0x104e. The console `b`
  command is an alternative way into BOOTSEL.
- **NX-K** on the USB-A breakout: D+ GP16, D- GP17, each through two 47 Ω
  in parallel, VBUS from pin 40, GND, shield to GND.
- **Running image:** standalone build, 240 MHz, bench Wi-Fi, OSC target
  192.168.75.166:8000. Console commands: `s` status, `c` config, `d` replay
  log, `l` all LEDs, `L<id> <val>` one LED, `e` key echo, `r` reboot,
  `b` BOOTSEL, `u` otactl update handoff (slot builds only), `h` help.
- **Local `build_config.cmake`** (git-ignored) holds the bench Wi-Fi
  credentials and OSC target 192.168.75.166:8000; the standalone build on
  the board uses it and joins.
- Something on bdsbrxi501 opens `/dev/ttyACM0` at boot and consumes the
  mirrored log history; attach a reader and press `d` to replay it.

## Verified

- Enumeration of the NX-K through TinyUSB + Pico-PIO-USB at 240 MHz: 8-byte
  and full descriptor reads, SET_ADDRESS, configuration, alt setting 1,
  interrupt endpoint 0x82. At 120 MHz it fails (details in the README).
- Decoding of keys, encoder turns with velocity, encoder presses.
- HID keyboard: digits and Enter arrive as keypad usages, Thru as typed
  ` Thru `, confirmed from Linux's input layer. Cue takes the OSC path.
- cyw43 joins the bench Wi-Fi at 240 MHz after the PIO host is up
  (`net: up, ip 192.168.75.220`), so the start-up order holds.
- NX-K LEDs: protocol, addresses and state bits surveyed with a person at
  the keypad, see README "NX-K LEDs". Console `l`, `L`, `e` drive them.
- Composite device (CDC + HID + reset interface) binds on Linux. On macOS
  the serial port did not bind while grandMA3 onPC held the device open
  through five USB user clients, so do not bench on the Mac that runs gMA3.

## Direction change (decided 2026-10-08, evening)

The HID keyboard path has a focus problem: keystrokes only reach grandMA3
when onPC is the active window. The new plan drops HID and talks to a
**Lua plugin in onPC over OSC in both directions**:

- **Input:** every NX-K event goes to `/cmd` as a Lua call, e.g.
  `/cmd,s,Lua "SFB.key('Store',1)"` and `Lua "SFB.wheel(1,0.25)"`. OSC
  `/cmd` is native and documented; `Lua` as a command keyword with a quoted
  string is the thing to confirm first on the onPC (the bdstark/GrandMA3MCP
  project may already do it).
- **Hardkeys:** nothing in MA3 presses a console hardkey from outside, not
  OSC `/Key` (executors only, and reported dead on onPC 2.4.2.2), not DMX
  or MIDI remotes (they target executors and objects). The plugin uses the
  **Quickey trick**: assign the hardkey code (STORE, CLEAR, THRU, PLEASE,
  ...) to a spare Quickey and press/release it, as riksolo's RBOSCKeys
  plugin does. Keypad digits can be typed into the command line the same
  way, so the whole keypad leaves HID.
- **Feedback:** the plugin reads console state and sends `/sfb/...`
  messages back with `Cmd('SendOSC ...')`; the firmware maps them onto
  LEDs. Design and schema: `docs/ma3-feedback.md`; skeleton:
  `tools/ma3/SurfaceFeedback.lua` (not yet run on a console).

Lua has **no socket, serial or USB access** of any kind. OSC, MIDI and DMX
are the console's I/O, not the plugin's; the plugin only sees the command
line and the object tree. That is why OSC `/cmd` with Lua strings is the
cleanest coupling: arguments, floats and strings both ways. MIDI would
need one remote plus one macro per key, with no arguments.

### Transport

The OSC menu has **one global Interface setting** (plus Preferred IP), so
onPC sends and receives all OSC through a single adapter. Per-line settings
are only destination IP, port, mode and the send/receive flags. The claim
that onPC refuses loopback is a single forum report and unverified.

| transport | status | notes |
|-----------|--------|-------|
| Wi-Fi (Pico W, today) | works, joins bench Wi-Fi | zero new work; OSC interface = the PC's LAN adapter, which already serves any other OSC. Risk is show-floor Wi-Fi. |
| CDC-NCM over the USB cable | roadmap | the Pico becomes a second adapter; OSC would have to bind to it, so **no other OSC device can be used at the same time**. Only worth it if the PC has no LAN for OSC. Inbox NCM driver on Windows 10 2004+ and macOS; TinyUSB has an NCM device with lwIP. |
| Wired Ethernet on the LAN | option | no USB link to the PC at all (the plan needs none once HID is gone). **W5500-EVB-Pico** (RP2040 + W5500, WIZnet lwIP port) keeps this firmware: Pico-PIO-USB uses PIO, the W5500 uses SPI, so swap the cyw43 netif for a W5500 one. A Pi 4 is the Python/libusb route if a rewrite is preferred; ESP32 needs an S2/S3 for USB host plus a PHY or W5500. Caveat: the otactl bootstrap is Pico W / Wi-Fi only. |

Recommendation: prove the plugin over Wi-Fi now, no hardware change. If a
cable is wanted later, prefer Ethernet on the LAN over NCM because of the
single-interface binding; NCM only if the lighting PC has no LAN.

## Next steps, in order

1. Done: the board joins Wi-Fi and gets a 192.168.75.x address, same
   subnet as bdsbrxi501.
2. Run `python3 tools/osc_listen.py 8000` on bdsbrxi501, press Cue and turn
   an encoder. Expect `/key/Cue 1` then `/key/Cue 0`, and `/wheel/1 <float>`
   for Rotary1. Confirms the OSC path before the route change.
3. On the onPC: confirm `/cmd,s,Lua "..."` reaches a plugin function with
   arguments; run `Plugin "SurfaceFeedback" "probe"` and fill in the
   getters in `tools/ma3/SurfaceFeedback.lua` (open questions listed in
   `docs/ma3-feedback.md`); prototype one Quickey hardkey press/release.
4. Firmware: change `src/route.c` to send `/cmd` Lua calls for everything
   (keep the old `/key`, `/wheel` addresses behind a config flag until the
   plugin is proven); add OSC receive in `src/osc.c` (lwIP UDP bind on a
   listen port), a state model, the NX-K LED map from `docs/ma3-feedback.md`
   and the Link watchdog. Drop the HID interface from the descriptor last.
5. otactl slot build on a bootstrap-provisioned Pico W; the `u` command.
6. Decide the transport per the table above once the plugin works.
7. Hub support (CFG_TUH_HUB 1, CFG_TUH_DEVICE_MAX 3, per-device state in
   nxk_host.c), then M-Touch and M-Play from the MTouchPlay protocol docs;
   the NX-K is an Obsidian/Elation ONYX device, not ETC, and shares their
   protocol, so the MTouchPlay notes could gain an NX-K section.
8. Log sink over Wi-Fi (rotec's logserver pattern).

## Gotchas met along the way

- A charge-only micro-USB cable and 22 kΩ resistors (read as 22 Ω) cost
  most of a day. The bdsbrxi501 ports were fine all along.
- TinyUSB stalls SET_CONFIGURATION if any interface lacks a class driver;
  the reset interface needs the small driver in `src/usb_reset.c`.
- picotool sends its reset requests as class-type, not vendor-type.
- Killing a serial reader over ssh with a pattern that also matches the
  ssh command line kills the shell itself; use `fuser -k /dev/ttyACM0`.
- `system_profiler` prints nothing from the Claude tool shell on the Mac;
  use `ioreg -p IOUSB -l`.
- With `MTPNXK_TUSB_DEBUG=2` the device-side log of its own CDC writes
  feeds back into more CDC writes; `src/log.c` filters those lines.

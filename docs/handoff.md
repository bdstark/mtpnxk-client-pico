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

## Direction change (decided 2026-10-08, revised 2026-10-09)

The HID keyboard path has a focus problem: keystrokes only reach grandMA3
when onPC is the active window. The new plan drops HID and talks to **our
own Lua plugin in onPC over a plain UDP socket**, both directions:

- **Lua has sockets after all.** onPC ships LuaSocket and a JSON library
  (`shared/resource/lib_plugins/requirements`; `require("socket")`,
  `require("json")`). bdstark/GrandMA3MCP's `plugin/gma3_mcp_bridge.lua`
  is the working reference: a non-blocking TCP server (`settimeout(0)`)
  inside the plugin call, `coroutine.yield()` every frame so the console
  stays responsive, newline-delimited JSON, dispatch to `Cmd()` under
  `xpcall`. It must stay in the plugin call (onPC runs `Cleanup` when the
  call ends, so no `Timer` hand-off), and it has to be started again after
  every show load. Copy that loop; bind UDP instead of TCP
  (`socket.udp()`, `setsockname("*", port)`, `receivefrom`, `sendto`) so
  the Pico, or a bridge process on the PC, needs no connection state.
- **What OSC would have bought us, and why it loses.** OSC `/cmd` works
  with no plugin running and plays with other OSC tools. Against that: the
  OSC menu binds all OSC to **one interface**, so the Pico would compete
  with every other OSC device; `SendOSC` goes through the command line and
  needs string quoting; MA3 requires type tags; and the `/key` and
  `/wheel` addresses were never native. A socket in our plugin has none of
  those limits, carries structs both ways, and binds to all interfaces, so
  the Pico on the LAN and a PC-resident bridge on 127.0.0.1 look the same.
  Keep `/cmd` only as a degraded fallback if ever needed. (The transport
  verdict in `docs/ma3-feedback.md`, written before LuaSocket was
  confirmed, is superseded by this; its message schema still applies with
  UDP/JSON replacing OSC addresses.)
- **Hardkeys: nothing presses a console hardkey from outside.** OSC `/Key`
  means executors (and is reported dead on onPC 2.4.2.2); DMX and MIDI
  remotes target executors and objects. Two in-plugin mechanisms exist:
  1. **Quickey pool** (proven: riksolo's RBOSCKeys). A Quickey's KeyCode
     can be any hardkey (Store, Please, Thru, Clear, digits, MA ...).
     Reserve a range of N Quickeys and N executor buttons on a parked
     page at start-up, cache them; on press take a free slot, set its
     KeyCode, press its executor; on release release it and free the
     slot. True press/hold/release, N keys held at once. Our plugin does
     this itself; no dependency on RBOSCKeys. First thing to prototype is
     the press/release command for the executor from Lua (candidates:
     `Cmd` with the executor's button function, or the executor object's
     methods; RBOSCKeys drives it via OSC `/Page/Key` which we will not
     have).
  2. **`Keyboard()`** (undocumented object-free function, listed in the
     community API dump):
     `Keyboard(display_index, 'press'|'char'|'release', char_or_keycode,
     shift, ctrl, alt, numlock)`. Injects keyboard events inside the
     application, so OS focus is irrelevant. Nobody has published it
     pressing Store; key code names are undocumented (`HelpLua` exports
     the real list). If `'char'` types digits onto the command line it is
     the simplest path for the keypad; if `'press'` with a hardkey code
     works, it replaces the Quickey pool. Probe it before building the
     pool.
- **Feedback:** the plugin reads console state and sends it back on the
  same socket; the firmware maps state onto LEDs. Design: `docs/ma3-feedback.md`;
  skeleton: `tools/ma3/SurfaceFeedback.lua` (OSC-based, not yet run on a
  console; to be reworked onto the socket).

The only MA3 configuration left is importing and starting the plugin.

### Transport

The OSC menu has **one global Interface setting** (plus Preferred IP), so
onPC sends and receives all OSC through a single adapter. Per-line settings
are only destination IP, port, mode and the send/receive flags. The claim
that onPC refuses loopback is a single forum report and unverified.

| transport | status | notes |
|-----------|--------|-------|
| Wi-Fi (Pico W, today) | works, joins bench Wi-Fi | zero new work; the plugin's socket binds all interfaces so no MA3 network setting is involved. Risk is show-floor Wi-Fi. |
| Bridge process on the onPC machine | option | the Go `nxk` tool's USB + publisher parts, pointed at the plugin's socket on 127.0.0.1 (exactly the MCP bridge's shape). No new hardware; needs WinUSB/libusb for the NX-K on Windows and a service to keep it running. Fastest way to prove the plugin. |
| W5500-EVB-Pico-PoE (owned) | option, preferred box | PoE 802.3af, 8 W (5 V 1.6 A) in; W5500 on GPIO16-21 (SPI0); USB-C runs the RP2040 **native** host (TinyUSB `hcd_rp2040`, hub support), so Pico-PIO-USB and the 240 MHz requirement go. Open: whether PoE 5 V reaches the USB-C VBUS (measure pin 40 on PoE with nothing plugged in); use a self-powered hub regardless, the surfaces exceed the PoE budget. Console moves to UART0; cyw43 netif swaps for WIZnet's W5500 lwIP port; otactl bootstrap does not apply. |
| CDC-NCM over the USB cable | dropped | second adapter on the PC for no gain now that MA3's OSC interface binding is out of the picture. |
| Wired Ethernet on the LAN | option | no USB link to the PC at all (the plan needs none once HID is gone). **W5500-EVB-Pico** (RP2040 + W5500, WIZnet lwIP port) keeps this firmware: Pico-PIO-USB uses PIO, the W5500 uses SPI, so swap the cyw43 netif for a W5500 one. A Pi 4 is the Python/libusb route if a rewrite is preferred; ESP32 needs an S2/S3 for USB host plus a PHY or W5500. Caveat: the otactl bootstrap is Pico W / Wi-Fi only. |

Recommendation: prove the plugin with whatever is quickest (Wi-Fi Pico or
the PC-resident bridge; both speak the same UDP/JSON), then build the PoE
box. Hyper-V NICs, a second NIC or a routed VLAN are not needed: a socket
bound to all interfaces receives from the LAN and from 127.0.0.1 alike.

## Next steps, in order

1. Done: the board joins Wi-Fi and gets a 192.168.75.x address, same
   subnet as bdsbrxi501.
2. Run `python3 tools/osc_listen.py 8000` on bdsbrxi501, press Cue and turn
   an encoder. Expect `/key/Cue 1` then `/key/Cue 0`, and `/wheel/1 <float>`
   for Rotary1. Confirms the OSC path before the route change.
3. On the onPC, in this order: `HelpLua` for the function list and key
   code names; probe `Keyboard()` (`Lua "Keyboard(1,'char','5')"`, then a
   hardkey press/release); prototype one Quickey press/release from Lua;
   run `Plugin "SurfaceFeedback" "probe"` for the state getters (open
   questions in `docs/ma3-feedback.md`). Then write the plugin: UDP
   server after the MCP bridge's loop, JSON lines in (`key`, `wheel`),
   state out, Quickey pool or `Keyboard()` for hardkeys.
4. Firmware: replace the OSC publisher in `src/route.c`/`src/osc.c` with
   UDP/JSON to the plugin (keep OSC behind a config flag until the plugin
   is proven); add a UDP receive path, a state model, the NX-K LED map from
   `docs/ma3-feedback.md` and the Link watchdog. Drop the HID interface
   from the descriptor last.
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

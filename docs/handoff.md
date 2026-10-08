# State of play, 2026-10-08 (updated after the LED session)

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
- **Running image:** standalone build, 240 MHz, dummy Wi-Fi, OSC target
  127.0.0.1. Console commands: `s` status, `c` config, `r` reboot,
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

## Next steps, in order

1. Done: the board joins Wi-Fi and gets a 192.168.75.x address, same
   subnet as bdsbrxi501.
2. Run `python3 tools/osc_listen.py 8000` on bdsbrxi501, press Cue and turn
   an encoder. Expect `/key/Cue 1` then `/key/Cue 0`, and `/wheel/1 <float>`
   for Rotary1. `/cmd` with a string comes from HighLight, Undo, Next, Last
   and the other command buttons.
3. Move the board to the grandMA3 PC: keypad shortcuts with ShCuts on, and
   whether the `/key/...` and `/wheel/...` addresses do anything natively
   (they are inherited from nxk, not from the MA manual; `/cmd` is native).
4. otactl slot build on a bootstrap-provisioned Pico W; the `u` command.
5. LED policy: decide what the LEDs should show in normal operation (key
   echo, Bank held, OSC feedback from the console) now that the write path
   exists. Note the NX-K is an Obsidian/Elation ONYX device, not ETC, and
   shares the M-Touch protocol; the MTouchPlay notes could gain a section.
6. Hub support (CFG_TUH_HUB 1, CFG_TUH_DEVICE_MAX 3, per-device state in
   nxk_host.c), then M-Touch and M-Play from the MTouchPlay protocol docs.
7. Log sink over Wi-Fi (rotec's logserver pattern), CDC-NCM on the device
   port so OSC can ride the cable.

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

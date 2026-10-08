# mtpnxk-client-pico

Firmware for a Raspberry Pi Pico W that turns USB lighting control surfaces
into something a grandMA3 PC understands, with no software on the PC. The
first surface is the Obsidian Control Systems (Elation) **NX-K** ONYX
keypad; the Martin **M-Touch** and **M-Play** (see the MTouchPlay protocol
notes) are the planned next ones, hence the name. All three share one USB
protocol family (vendor 0x11BE), documented in MTouchPlay `docs/protocol.md`.

It runs as an [otactl](https://github.com/bdstark/otactl) runtime app: the
otactl bootstrap on the Pico owns Wi-Fi provisioning, device identity and
signed updates, and chain-loads this image from the runtime slot.

## How it works

```
NX-K keypad ──USB──▶ PIO USB host (core 1) ──queue──▶ decode ──▶ route ─┬─▶ HID keyboard (native USB) ──▶ grandMA3 PC
                                                                        └─▶ OSC over Wi-Fi ────────────▶ grandMA3 PC
```

- **Keypad keys become keystrokes.** grandMA3's keyboard-shortcut layer
  (ShCuts / F10) turns any keyboard into console keys, and that is the only
  third-party key input MA supports. Digits, `.`, `/`, `-`, `+`, `Enter`,
  `Back` and `@` go out as keypad usages; `Thru` and `Full` are typed as
  text, exactly as the macOS build of `nxk` in avrsvc does today.
- **Everything else is OSC**, with the same addresses and payloads as
  `nxk -publisher=osc`: command buttons send `/cmd` with a string, key-style
  buttons send `/key/<Name>` with 1/0, encoders send `/wheel/N` with a
  float velocity (`/wheel/10N` while Bank is held).
- **Native OSC cannot press hardkeys.** grandMA3's OSC input only addresses
  executors, faders, pages and `/cmd`; `/key` means executor buttons. That is
  why the keypad goes through HID, and why the `/key/...` and `/wheel/...`
  addresses above still need to be confirmed against the 2.5 console (they
  are inherited from `nxk`, not from the MA manual).

Wi-Fi carries OSC, logs and the otactl update handoff. Keystrokes never
depend on it.

### NX-K LEDs

Verified on hardware 2026-10-08. The NX-K speaks the M-Touch protocol: the
host selects interface 0 alternate setting 1, which exposes only interrupt
IN 0x82, and drives LEDs with vendor control requests on EP0
(`bmRequestType 0x40, bRequest 0x80, wValue = state, wIndex = control
address, wLength 0`). The control address is the one the keypad reports:
group in the high byte, control in the low byte (Record 0x5401, Edit
0x5101, HighLight 0x6001, Last 0x6401, Next 0x6402, the same numbers as on
the M-Touch). Encoder LEDs are at the press addresses 0x5901, 0x5911,
0x5921, 0x5931.

Every LED is single-colour (blue, except Link which is red) and uses only
the M-Touch green lane:

| wValue | result |
| ------ | ------ |
| 0x0000 | off |
| 0x0001 | on |
| 0x0011 | blink (bit 4 alone does nothing) |
| 0x0021 | off (bit 5 forces off, as on the M-Touch) |

Bits 1, 2, 6 and 8 have no effect. The keypad keys (digits, `.`, `/`, `-`,
`+`, `@`, `Enter`, `Thru`, `Full`) have no LED. The keypad keeps its LED
state while VBUS stays up, including across a Pico reboot. The firmware
does not yet drive LEDs in normal operation; the console commands below
exist for the bench.

## Hardware

- Raspberry Pi Pico W. (Not the Pico 2 W yet: the TinyUSB bundled with Pico
  SDK 2.3 only builds the PIO-USB host for RP2040, and the otactl bootstrap
  is RP2040-only as well.)
- The micro-USB port is the **device** side: plug it into the lighting PC.
  It enumerates as a HID keyboard and powers the board.
- The **host** port for the NX-K is bit-banged on two GPIOs. Wire a female
  USB-A breakout as follows:

  | Breakout | Pico W                          |
  | -------- | ------------------------------- |
  | VBUS     | VBUS (pin 40, 5 V from the PC)  |
  | GND      | GND                             |
  | D+       | GP16 through 22 Ω               |
  | D-       | GP17 through 22 Ω               |

  D+ and D- must be consecutive GPIOs (`MTPNXK_PIO_USB_DP_PIN` selects D+).
  Pico-PIO-USB enables the host pull-downs internally. GP0/GP1 stay free for
  UART0, which carries the log and a one-key bench console (`h` for help),
  also available on the USB serial port: `s` status, `c` config, `d` replay
  the log ring buffer, `l` all NX-K LEDs on/off, `L<id> <val>` one LED
  (hex, e.g. `L5101 0011`), `e` light a key while held, `u` otactl update,
  `r` reboot, `b` BOOTSEL.
- The system clock runs at 240 MHz with the core at 1.15 V. Pico-PIO-USB
  needs a multiple of 12 MHz, and its examples use 120 MHz, but at 120 MHz the
  PIO bit-clock dividers are fractional and the NX-K does not get through
  enumeration: the first descriptor read ends in a STALL, or SET_ADDRESS is
  acknowledged but never adopted. At 240 MHz the dividers are integers and it
  enumerates first time. `MTPNXK_SYS_CLOCK_KHZ` overrides it.

## Building

Requires the Pico SDK (2.3.0 tested) and the ARM GNU toolchain. The Mac
has both under `~/Development/embedded/`; `bdsbrxi501` has the same under
`~/.pico-sdk/`.

```bash
git submodule update --init
cmake -S . -B build \
  -DPICO_SDK_PATH="$HOME/Development/embedded/pico-sdk" \
  -DPICO_TOOLCHAIN_PATH="$HOME/Development/embedded/arm-gnu-toolchain-14.2.rel1-darwin-arm64-arm-none-eabi"
cmake --build build -j8
```

Output: `build/mtpnxk.uf2`, linked for the otactl runtime slot.

Options (`-D...` at configure time):

| Option                   | Default     | Meaning                                                                 |
| ------------------------ | ----------- | ----------------------------------------------------------------------- |
| `MTPNXK_OTACTL_SLOT`     | `ON`        | Link at flash offset 0x100000 and read Wi-Fi/options from the otactl store |
| `MTPNXK_PIO_USB_DP_PIN`  | `16`        | GPIO for host D+ (D- is the next one)                                   |
| `MTPNXK_WIFI_SSID` / `_PASSWORD` | empty | Bench credentials; also the fallback when the store has none        |
| `MTPNXK_OSC_HOST` / `_PORT` | `127.0.0.1` / `8000` | Default OSC target; the otactl options form overrides it     |
| `MTPNXK_SYS_CLOCK_KHZ`   | `240000`    | System clock; see Hardware for why not 120 MHz                          |
| `MTPNXK_TUSB_DEBUG`      | `0`         | TinyUSB log level routed to the console (bring-up only)                 |

A **standalone** build (`-DMTPNXK_OTACTL_SLOT=OFF`) links at flash base,
takes Wi-Fi from the CMake cache, and can be dragged onto a bare Pico W in
BOOTSEL mode for bench work without the bootstrap. Put bench credentials in
a git-ignored `build_config.cmake` next to `CMakeLists.txt` rather than on
the command line:

```cmake
set(MTPNXK_WIFI_SSID "mynet" CACHE STRING "" FORCE)
set(MTPNXK_WIFI_PASSWORD "secret" CACHE STRING "" FORCE)
set(MTPNXK_OSC_HOST "192.168.1.50" CACHE STRING "" FORCE)
```

Reflashing a running board on a Linux bench host, no BOOTSEL needed
(the vendor product id needs sudo unless a udev rule covers 2e8a:104e):

```bash
sudo picotool reboot -u --vid 11914 --pid 4174 -f && sleep 2 && picotool load -x build-standalone/mtpnxk.uf2
```

## otactl integration

The contract is defined by `otactl-boot-pico` (`include/runtime_slot.h`,
`include/flash_store.h`, `src/flash_store.c`); this firmware mirrors it in
`src/otactl_store.c` and `ld/otactl-slot/memory_flash.incl`.

- **Slot.** The runtime is linked at 0x10100000 with 0xF7000 bytes
  available; the bootstrap verifies a commit marker and jumps to the vector
  table 256 bytes in, skipping the image's own boot2. The UF2 therefore
  targets the slot and nothing else, which is what the updater insists on.
- **Flash store.** Wi-Fi credentials, the device id and the app's options
  form data are read from the newest valid config slot at the top of flash
  (schema version 4; the struct must stay byte-identical to the bootstrap's
  `flash_config_t`). Options are `application/x-www-form-urlencoded`, as
  submitted on the setup page; this app reads `osc_host` and `osc_port`.
- **Updates.** The runtime cannot install over itself. `u` on the console
  (later: an MQTT/HTTP command) writes the update-request magic to watchdog
  scratch register 0 and reboots; the bootstrap fetches and verifies the
  signed manifest and installs the new image.
- **Publishing.** Upload `build/mtpnxk.uf2` with `otactl boot-usb upload`
  as app `mtpnxk`, arch `pico-w`, format `uf2`, role runtime, and assign the
  app to the unit. The options form (OSC host and port) is attached to the
  app or release in otactl.

Note for the bootstrap's comments: they cite a Rust reference runtime
(`client-pico-rs`) that was never created. The headers are the contract;
this is the first runtime app.

## Bring-up checklist

Things that compile but have not run on hardware yet, in the order to test:

1. Console on the USB serial port (or UART0); `s` prints counters. Done.
2. HID keyboard enumerates on the PC and keypad presses arrive as keypad
   usages, `Thru` as typed text. Done on Linux 2026-10-08; the gMA3
   shortcut mapping itself is still to be verified on the lighting PC.
3. NX-K enumerates on the PIO port (`usb host: device 11be:e102`), alt
   setting 1 is selected, packets arrive, keys decode. Done on 2026-10-08
   at 240 MHz (see Hardware): keys, encoder turns with velocity, and
   encoder presses all decode. The keypad answers idle polls with
   zero-length packets, which the reader skips, as the Go version did.
4. Wi-Fi joins after the USB stacks are up. The cyw43 driver and
   Pico-PIO-USB both claim PIO state machines; the start-up order in
   `main.c` (host first, then cyw43) is what keeps them apart, and is the
   most likely thing to need adjusting.
5. OSC reaches the console: `/cmd` on a command button, `/wheel/1` on an
   encoder.
6. Slot build boots under the otactl bootstrap and `u` hands off an update.

## Roadmap

- Log sink over Wi-Fi (rotec's ring buffer + SSE page, MQTT line).
- CDC-NCM on the device port so OSC can ride the USB cable instead of Wi-Fi.
- M-Touch and M-Play hosting on the same board (hub support, LED feedback),
  using the protocol documented in MTouchPlay.
- Pico 2 W once the SDK's TinyUSB builds PIO-USB host for RP2350 and the
  bootstrap follows.

## Layout

```
CMakeLists.txt            build, options, otactl slot switch
ld/otactl-slot/           linker override placing FLASH at the runtime slot
lib/Pico-PIO-USB/         submodule (0.7.2), the PIO USB host
src/main.c                core 0 loop, start-up order, bench console
src/nxk_host.c            core 1: PIO host, NX-K enumeration, raw endpoint reads, LED writes
src/nxk_decode.c          NX-K packet decoder (port of avrsvc cmd/nxk/decode.go)
src/route.c               keypad -> HID, everything else -> OSC (port of publisher.go/osc.go)
src/hid_kbd.c             keyboard report queue and device callbacks
src/usb_descriptors.c     HID keyboard descriptors
src/osc.c                 OSC encoder over lwIP UDP
src/net.c                 cyw43 station mode with reconnect
src/otactl_store.c        otactl flash store reader, update handoff
src/config.c              effective configuration (store with CMake fallbacks)
src/log.c                 timestamped log with ring buffer
```

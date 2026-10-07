# mtpnxk-client-pico

Firmware for a Raspberry Pi Pico W that turns USB lighting control surfaces
into something a grandMA3 PC understands, with no software on the PC. The
first surface is the ETC/Shell **NX-K** keypad; the Martin **M-Touch** and
**M-Play** (see the MTouchPlay protocol notes) are the planned next ones,
hence the name.

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
  UART0, which carries the log and a one-key bench console (`h` for help).
- The system clock runs at 120 MHz, a multiple of the 12 MHz PIO-USB needs.

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

A **standalone** build (`-DMTPNXK_OTACTL_SLOT=OFF`) links at flash base,
takes Wi-Fi from the CMake cache, and can be dragged onto a bare Pico W in
BOOTSEL mode for bench work without the bootstrap.

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

1. UART log and console on GP0/GP1; `s` prints counters.
2. HID keyboard enumerates on the PC; `Thru` types text with ShCuts on.
3. NX-K enumerates on the PIO port (`usb host: device 11be:e102`), alt
   setting 1 is selected, packets arrive, keys decode.
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
src/nxk_host.c            core 1: PIO host, NX-K enumeration, raw endpoint reads
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

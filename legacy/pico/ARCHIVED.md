# Archived: Pico W firmware

This tree is the Raspberry Pi Pico W firmware that hosted the NX-K keypad on a
PIO USB port and spoke HID + OSC to the lighting PC. It was the project's first
direction and was verified on the bench through 2026-10-08 (enumeration,
decoding, LEDs, Wi-Fi). See [README.md](README.md) for what it does and how it
was built; the final committed state is also tagged `pico-firmware-final`.

On 2026-10-09 the project changed direction: the surface bridge is now a
cross-platform Rust service on the PC (or any machine) that talks to the NX-K
over USB and to our own grandMA3 Lua plugin over UDP. Nothing here is built or
maintained any more. What carries over:

- `src/nxk_decode.c`: the NX-K packet decoder and control addresses (ported
  again into the Rust service).
- `src/nxk_host.c`: the LED write (vendor request 0x80, wValue = state,
  wIndex = control address) and the alt-setting / endpoint details.
- The LED survey in README.md ("NX-K LEDs").

The build still works from this directory (`cmake -S legacy/pico -B build ...`)
with the submodule at `legacy/pico/lib/Pico-PIO-USB`.

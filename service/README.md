# mtpnxk service

Cross-platform Rust process that owns the NX-K keypad over USB and talks to the
`mtpnxk_surface` plugin inside grandMA3 onPC over an authenticated UDP link
([docs/surface-protocol.md](../docs/surface-protocol.md)).

```bash
cargo build --release
./target/release/mtpnxk keygen                      # a fresh 64-hex pairing key
./target/release/mtpnxk --key <hex> run             # drive the USB keypad
./target/release/mtpnxk --key <hex> sim --script "Record:down,5:tap@50,Record:up@300,Enter:tap"
./target/release/mtpnxk --key <hex> bench --taps 200 --rate 20
./target/release/mtpnxk list                        # USB devices
```

`--plugin host:port` (default `127.0.0.1:9810`) names the plugin; `--key-file`
or `MTPNXK_KEY` can carry the key instead of the command line; `--verbose`
prints pairing, refusals and losses as they happen.

On the console side the plugin is started with the same key:

```
Plugin "mtpnxk_surface" "key=<hex>"
```

## What it does

- `nxk`: decodes the keypad's interrupt packets (ported from the archived
  firmware, verified on hardware) and writes LEDs with the vendor control
  request. USB access is [nusb](https://crates.io/crates/nusb): no libusb.
  Windows needs the WinUSB driver bound to the keypad (Zadig); macOS and
  Linux need nothing (Linux may need a udev rule for non-root access).
- `link`: pairing (hello/welcome with nonce), HMAC on every datagram, strictly
  increasing sequence numbers, event ids retransmitted until acknowledged
  (3 × 25 ms, then counted lost), heartbeats with the physically held keys,
  a 1.5 s watchdog, re-pairing after 4 s of silence or a `no-session` error,
  and the console-state cache keyed on the plugin generation and feedback
  epoch.
- `leds`: the NX-K map from `docs/ma3-feedback.md`: keyword keys follow
  `pending`, HighLight blinks while on, Preview follows the mode, Clear/Undo/
  Next/Last/Menu/Snap Shot echo the hold, Bank and the encoders light while
  Bank is held, Link blinks whenever the state is not fresh. Unknown is off.
- `sim`: a scripted keypad for machines without the hardware; `bench` is a
  scripted tap run that prints ack round-trip percentiles and, when the plugin
  was started with `bench`, press-to-effect percentiles.

`cargo test` runs the unit tests, including a fake plugin that exercises the
link's retransmission, loss, freshness and re-pairing paths.

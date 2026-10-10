# mtpnxk service

Cross-platform Rust process that owns the NX-K keypad over USB and talks to the
`mtpnxk_surface` plugin inside grandMA3 onPC over an authenticated UDP link
([docs/surface-protocol.md](../docs/surface-protocol.md)).

```bash
cargo build --release
./target/release/mtpnxk keygen                      # a fresh 64-hex pairing key
./target/release/mtpnxk --key-file ~/.mtpnxk.key run # drive the USB keypad (or --key <hex>)
./target/release/mtpnxk --key <hex> sim --script "Record:down,5:tap@50,Record:up@300,Enter:tap"
./target/release/mtpnxk --key <hex> bench --taps 200 --rate 20
./target/release/mtpnxk list                        # USB devices
./target/release/mtpnxk mtouch-listen --seconds 240 # KB-16: decode an M-Touch / M-Play (--pid f808|f80c)
./target/release/mtpnxk mtouch-led-test             # KB-16: walk every LED, bar and display by eye
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
- `link`: pairing (hello/welcome with nonce), a SipHash-2-4 MAC on every
  datagram, strictly increasing sequence numbers, event ids retransmitted until
  acknowledged (4 × 60 ms, then counted lost; a press stops being retransmitted
  once its release is sent), heartbeats with the physically held keys,
  a 1.5 s watchdog, re-pairing after 4 s of silence or a `no-session` error,
  and the console-state cache keyed on the plugin generation and feedback
  epoch.
- `leds`: the NX-K map from `docs/ma3-feedback.md`: keyword keys follow
  `pending`, HighLight blinks while on, Preview follows the mode, Clear/Undo/
  Next/Last/Menu/Snap Shot echo the hold, Bank and the encoders light while
  Bank is held, Link blinks whenever the state is not fresh. Unknown is off.
- `mtouch`: the Martin M-Touch (`11be:f808`) and M-Play (`11be:f80c`):
  control tables for both models, the decoder for the four report types
  (single key, single fader, key bank, analog bank; bank unit count read from
  the packet; concatenated reports split), and pure output encoders for LED
  keys (`0x80`), fader bars (`0x61`) and the page display (`0x54`), ported
  from MTouchPlay `e48eb2c` and regression-tested against its capture logs.
  `mtouch-listen` and `mtouch-led-test` are the operator's hardware
  qualification tools ([docs/mtouch-protocol-reuse.md](../docs/mtouch-protocol-reuse.md));
  neither device has been connected to this service and neither is wired into
  the link (`run`, `sim` and `bench` are NX-K only).
- `sim`: a scripted keypad for machines without the hardware; `bench` is a
  scripted tap run that prints ack round-trip percentiles and, when the plugin
  was started with `bench`, press-to-effect percentiles.

`cargo test` runs the 45 unit tests, including a fake plugin that exercises the
link's retransmission, loss, freshness and re-pairing paths and 17 M-Touch /
M-Play regressions against the documented captures. Operator setup and
recovery: [docs/operator-guide.md](../docs/operator-guide.md); qualified
deployments: [docs/deployments.md](../docs/deployments.md).

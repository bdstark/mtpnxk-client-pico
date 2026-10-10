# KB-16: Martin M-Touch and M-Play protocol reuse (surface half)

Written 2026-10-10. This record says which facts about the M-Touch (VID `11be`,
PID `f808`) and M-Play (PID `f80c`) were carried into this service from the
MTouchPlay repository, how each is regression-tested here, which claims still
need hardware, and how an operator runs the live qualification. The decoder and output
encoders are a port of documented evidence; the live qualification of section 6 was run on
2026-10-10 on macOS with both devices: [probes/kb-16-hardware-mtouch-macos.md](probes/kb-16-hardware-mtouch-macos.md)
and [probes/kb-16-hardware-mplay-macos.md](probes/kb-16-hardware-mplay-macos.md) (logs in
`probes/kb-16-logs/`). Two operating facts came out of those runs: right after attaching, nusb's device
list can stay empty for a few seconds while macOS already shows the device (retry the open), and only one
process may hold the device (an LED test started while a listener was open was refused with IOKit
`0xe00002c5`).

Hardware protocol qualification does not establish grandMA3 integration
behaviour. Which console actions a fader, a pressure key or an LED should map
to is the console-semantics half of KB-16, kept in
[bdstark/GrandMA3MCP `ENCODERS.md`](https://github.com/bdstark/GrandMA3MCP/blob/main/ENCODERS.md);
wiring these devices into the link is later work (KB-20 and after). The
`run`, `sim` and `bench` commands remain NX-K only.

## 1. Source

| item | value |
| --- | --- |
| Repository | `MTouchPlay` (local checkout `/Users/bstark/Development/MTouchPlay`) |
| Revision | `e48eb2c` |
| Protocol | `docs/protocol.md` (M-Touch, status "complete for phase 1", 2026-09-23) |
| Model differences | `docs/mplay-notes.md` (M-Play, status "complete") |
| How the evidence was gathered | `docs/capture-playbook.md` |
| Reference implementation | `tools/mtouch-cli/src/main.rs` (`CONTROLS`, `CONTROLS_MPLAY`, `decode_report`, `led_value`, `bar_payload`, `page_payload`), `tools/mtouch-controls.tsv` (81 lines) |
| Captures | `captures/20260618-2215-mtouch-onyx-launch-initial.pcapng` (cap1), `captures/20260618-2224-mtouch-onyx-fader1-red-to-100.pcapng` (cap2), `captures/20260923-listen-session-b.log` (M-Touch input session, 1601 lines), `captures/20260923-mplay-listen-session.log` (M-Play input session, 252 lines), `captures/20260923-mplay-listen-decoded.txt` (expected decode of the M-Play bank reports, 229 lines; decoded line *k* = log line *k* + 18) |

The two `.pcapng` files were not re-read here; the frame numbers below are
quoted from `protocol.md`, which cites them.

## 2. What was reused

Implemented in `service/src/mtouch/mod.rs` (tables, decoder, encoders),
`service/src/mtouch/usb.rs` (transport) and `service/src/mtouch/tools.rs`
(operator commands).

**Transport** (protocol.md 1, 2): VID `0x11be`; interface 0, alternate
setting 1; interrupt IN `0x82`, 64-byte packets, zero-length packet on every
idle poll (8 ms); vendor control OUT `bmRequestType 0x40` with `wIndex` = the
control id; no handshake, reports flow as soon as the endpoint is polled; the
device keeps its last LED state. The device-open sequence is the NX-K's
(`nxk::usb::claim_vendor_interface`, now shared). Vendor requests
`0xA0..0xAF` (EZ-USB firmware load) are refused by the writer thread
(`mtouch::usb::FORBIDDEN_REQUESTS`) and asserted against in debug builds. Bulk
OUT `0x04` (DMX) is out of scope and not implemented.

**Addressing** (protocol.md 3, mplay-notes.md "Control map"): the full
control tables of both models with Onyx names and kinds (K LED key, V
pressure-sensing red/green key, L indicator, F fader with bar, D display, X
key without LED, B backlight), including the physical facts: M-Touch strip
*n* at `0x4200 + 0x10*(n-1)` with +1 upper capacitive button (blue, key
bank), +2 lower capacitive button (blue, key bank), +3 touch fader (RGB bar,
analog bank), +5 bottom pressure button (red+green, analog bank); base-channel
faders `0x61n1` (blue button, single key) / `0x61n2` (blue-only bar, single
fader); MF 1..10 at `0x5801..0x580A` (pressure, analog bank, stride 1); page
display `0x4401`. M-Play strips at `0x9203` (fader), `0x9205` (top button),
`0x92C5` (bottom button) + `0x10*(n-1)`, right block `0x9805` / `0x98C5`,
displays `0x9F01` (left) and `0x9E01` (right), page keys `0x9F12/13`,
`0x9E12/13`.

**The four report types** (protocol.md 5, mplay-notes.md "Reports"): single
key `01 02 lo hi state` (5 bytes); single fader `02 02 lo hi 00 value touch`
(7 bytes); key bank `C1 N base stride changed[N] state[N]`; analog bank
`C2 N base stride changed[N] value[N] touch[N]`. N is read from byte 1 of the
packet (`0x0A` M-Touch, `0x0C` M-Play), never assumed from the model. Two
short reports can share one packet, so the decoder consumes reports in
sequence by first byte and length and returns a remainder that fits nothing as
one `Unknown`. The lift report (touch 0 with the resting value), multi-touch
(units with `changed` 0 keep their value and touch flag) and the no-key-bank
fact of the M-Play are preserved.

**Output encodings** (protocol.md 4): LED key `bRequest 0x80`, `wValue` lanes
bit 0/1/8 on, 2/3/10 fast blink, 4/6/12 slow blink, 5/7/13 force off for
green/red/blue; fader bar `bRequest 0x61`, 9 bytes = three u24 little-endian
words in the order green, red, blue, bits 0..9 = LEDs from the bottom, bit 23
= blink that colour, base-channel bars blue-only; page display
`bRequest 0x54`, 6 bytes with digits at bytes 2 (right), 3 (middle), 4 (left)
as 7-segment masks `3F 06 5B 4F 66 6D 7D 07 7F 6F`, leading zeros blank,
bytes 0, 1, 5 written as zero (no visible effect in the survey; Onyx writes
`0x17` into byte 1).

**Model differences** (mplay-notes.md): identical descriptors, transport and
encodings; different control map; bank unit count 12; no key banks and no
7-byte single-fader reports on the M-Play; two page displays.

## 3. What was NOT re-verified here

Everything that needs a device. In particular: that either device enumerates
and can be claimed through nusb on macOS or Windows (the MTouchPlay evidence is
Windows/WinUSB only); the fader direction and 0..255 resolution; touch and
lift timing; the pressure ramp (soft ~200 ms vs hard jump to `0xFF`); the 8 ms
idle poll; the behaviour across unplug/replug; whether a report arrives at
open time before anything is touched (none is expected: protocol.md 2 says
reports flow on change only); every LED colour, blink rate and bar/display
rendering; the undocumented display bytes and LED bits 9, 11, 14, 15; the
BackLight control `0x7110`; anything about DMX.

## 4. Device-specific differences preserved

| Aspect | M-Touch (`f808`) | M-Play (`f80c`) | Where |
| --- | --- | --- | --- |
| Bank unit count (byte 1) | `0x0A`, analog bank 35 bytes, key bank 25 bytes | `0x0C`, analog bank 41 bytes, key bank would be 29 bytes (never occurs) | `decode` takes N from the packet; `Model::bank_units` |
| Key banks | upper/lower capacitive strip buttons `0x4201/0x4202` stride `0x10` | none (every button is pressure-sensing) | `CONTROLS_*`, test `mplay_key_bank_shape_decodes` |
| Single-fader reports (7 bytes) | base-channel faders `0x61n2` | none | `CONTROLS_MPLAY` has no `0x61xx` |
| Strips | 10 x (PFA K, PFB K, fader F, PFD V) at `0x42n0` | 12 x (fader F `0x9203`, top V `0x9205`, bottom V `0x92C5`) | tables, table tests |
| Extra keys | MF 1..10 V, Edit/Clear/Record/Update/Load/HighLight/Last/Next K, mode key X, 4 mode indicators L | right block 24 x V at `0x9805`/`0x98C5`; two `+`/`-` pairs | tables |
| Shared keys | Select, Rel, Beat, Snap, Pause, Next at `0x5502..0x5513`, same ids and colours | same | both tables |
| Blue-only bars | base-channel `0x61n2` | none, all 12 bars RGB | `led_test` writes only the blue word there |
| Page displays | `0x4401` | `0x9F01` left, `0x9E01` right | `Model::displays` |
| LED colours | K blue except Beat/Pause red, Next green; V red+green; L red | same rule | `tools::key_colour` |

## 5. Regression table

All in `service/src/mtouch/mod.rs` (`cargo test mtouch::`), 17 tests; each
test's comment names its evidence.

| Test | Evidence |
| --- | --- |
| `single_key_press_and_release` | protocol.md 5.1; listen-session-b.log lines 3, 4 (`0102114101` = `+` press, release), 1229 (`0102116101` = BASE CHANNEL 1); mplay log line 3 (`0102139f01` = FADER BANK PageDown) |
| `key_bank_pfa1_pressed` | protocol.md 5.3; listen-session-b.log lines 175 (PFA 1 pressed), 176 (released), 177 (PFB 1) |
| `analog_bank_mf1_pressure` | protocol.md 5.4; listen-session-b.log line 37 (MF 1 pressure `0xb0`, touch 1) |
| `analog_bank_fader_touch_then_lift` | protocol.md 5.4 lift rule; constructed for base `0x4203` after the shape of line 477 |
| `mplay_bank_fader_pfd1` | mplay log line 19 = decoded line 1 (FADER PFD 1 value 24 touch 1); log 37 / decoded 19 (`0x92C5`); log 70 / decoded 52 (`0x9203` lift at 255) |
| `mplay_key_bank_shape_decodes` | mplay-notes.md "Reports": 29-byte key bank with N = 12 |
| `mtouch_capture_lines_decode_as_logged` | 19 analog/fader lines (37, 49, 53, 55, 61, 63, 103, 179, 185, 207, 217, 225, 477, 699, 943, 1085, 1098, 1228, 1277) and 24 key/key-bank lines (3-35 odd, 4, 1229, 1230, 175-178) of listen-session-b.log, expected values as that log's decoder printed them |
| `mplay_capture_lines_decode_as_logged` | 12 bank lines (19, 23, 30, 37, 40, 41, 70, 71, 86, 128, 129, 166) checked against mplay-listen-decoded.txt lines 1, 5, 12, 19, 22, 23, 52, 53, 68, 110, 111, 148; 7 key lines (3, 4, 5, 7, 17, 248, 250) |
| `two_faders_in_one_report` | protocol.md 5.4 multi-touch; listen-session-b.log line 1320 (strips 2 and 3, strip 1 resting at `0x7e`); mplay log 227 / decoded 209 and log 247 / decoded 229 (both lifted) |
| `concatenated_reports_split` | protocol.md 5 "two short reports can share one packet" (cap1 frames 1716, 1730, 1756): 5+5, 5+7, 5+35 |
| `idle_and_unknown_packets` | protocol.md 2 and 5: zero-length idle poll; unknown remainder; a bank whose N does not fit |
| `led_key_values` | protocol.md 4.1 common values; Onyx `0x0011` / `0x0042` slow blinks (cap1 frames 2408, 2524); Onyx `0x0103` blue with harmless low bits |
| `fader_bar_levels_and_blink` | protocol.md 4.2; cap2 frames 236..359 (masks `0x001`, `0x003`, `0x007` .. `0x3FF` in red); bit 23 blink; blue-only base-channel bars; 10 LEDs maximum; word order |
| `page_display_digits` | protocol.md 4.3; Onyx `00 17 06 00 00 00` for page 1 (cap1 frame 3777), digit bytes only; 0, 10, 123, 999, clamp, blank |
| `outputs_carry_the_documented_requests` | protocol.md 4: `0x80` no data, `0x61` 9 bytes, `0x54` 6 bytes, `wIndex` = id; none in `0xA0..0xAF` |
| `mtouch_table_matches_the_documented_layout` | protocol.md 3.1-3.4 and `tools/mtouch-controls.tsv` (81 entries, 10 strips x 4, 4 base-channel pairs, the 3.3 button list, MF 1..10, kind counts 38 K / 20 V / 14 F / 4 L / 1 D / 3 X / 1 B) |
| `mplay_table_matches_the_documented_layout` | mplay-notes.md control map (12 strips x 3, 24 right-block buttons, 2 displays, 4 page keys, 6 shared keys; no `0x61xx`, no `0x42xx`) |

## 6. Live qualification procedure

Run on each device, on each host OS to be claimed, with the service built
from this repository (`cargo build --release` in `service/`). Onyx and any
other program holding the device must be closed. On Windows the device must
have WinUSB bound (Obsidian's own driver package binds it; otherwise Zadig).
Record the run in `docs/probes/kb-16-hardware-<device>-<os>.md` (`mtouch` or
`mplay`; `macos`, `windows`, `linux`) with the revision and hashes of the
service, the OS version, the device's `bcdDevice` from `mtpnxk list`, the
commands as typed, the printed output (attach or quote), what the operator
saw, what was not tested, and a verdict per item below.

1. **Presence.** `mtpnxk list` shows `11be:f808` or `11be:f80c`. Record the
   product string and whether a serial is reported (protocol.md says none).
2. **Input.** `mtpnxk mtouch-listen --seconds 240` (add `--pid f808` or
   `--pid f80c` when both are attached). Every line carries the decoded
   event and the raw packet; keep the whole output. During the run:
   - Press and release every key of the model's table once, in table order.
     Expected: one `KEY ... (press)` and one `(release)` line each, with the
     documented id and name; no `RAW` lines. Record any key that reports a
     different id, no report, or an `Unknown`.
   - Faders: for every F control, move it from bottom to top and back
     slowly, then lift. Record the **direction** (bottom should be 0, top
     255), the **resolution** (values should step through 0..255; note the
     smallest step seen and whether both 0 and 255 are reached), that
     `touch=1` appears while touched, and that the final line is the
     **lift** report (`touch=0 (lift)` with the resting value). Base-channel
     faders on the M-Touch must arrive as 7-byte `FADER` reports, strip
     faders as analog banks.
   - Pressure keys: for every V control, one **soft** press (expected: a ramp
     of values up and down over ~200 ms) and one **hard** press (expected: a
     jump to `0xFF`), each ending in `touch=0 (release)` with value 0.
   - Two faders at once (any two strips): both must appear in the same
     report lines; the untouched strips must not appear.
   - Idle: with nothing touched for 10 s, the summary's `idle polls` rate
     should be about 125/s (one zero-length packet every 8 ms). Record the
     figure and the min/max report interval printed.
3. **Reconnect.** With `mtouch-listen --seconds 0` running, unplug the
   device: the command must end with a `disconnected` line, not hang. Replug
   it, start `mtouch-listen` again and touch nothing for 5 s: there must be
   **no report** before the first touch (a startup report, if any arrives,
   must be recorded verbatim and must not be treated as state by future
   work). Then press one key and confirm reports resume.
4. **Output.** `mtpnxk mtouch-led-test` (same `--pid` rule; `--hold-ms`
   lengthens each step). It prints one line per write, 300 ms apart. Confirm
   by eye, per printed line, and record deviations:
   - every K key lights in its documented colour (blue; Beat and Pause red;
     Next green), then fast-blinks, then goes off; every V key lights red,
     then green, then fast-blinks red, then goes off; every L indicator
     lights red;
   - every bar fills 1..10 LEDs from the bottom in red (blue on the M-Touch
     base-channel bars), then shows bottom 5 green / top 5 red (all blue on
     the base-channel bars), then blinks, then goes off;
   - each page display shows `1`, `42`, `999`, then blank. Note whether the
     unused left/middle digits are blank for `1` and `42`.
5. **Cleanup.** The test ends with everything off and blank; if the process
   is interrupted, rerun it or power-cycle the device (it keeps its last LED
   state).

Items 2 to 4 give the device/OS row in [deployments.md](deployments.md) its
status; the macOS rows have their records (above). Qualifying one
device does not qualify the other, and qualifying one host OS does not qualify
another.

## 7. Choices made while porting

- The M-Touch log's bank lines are printed as `BANK10 hdr=... raw=...` by
  the decoder of the day (it predates the bank decoding in `decode_report`);
  their expected ids and values were derived from the `hdr`/`flags`/`values`/
  `touch` fields of the same line, which the test comments cite by line
  number. The M-Play log prints banks as `RAW len=41 <hex>` and the expected
  decode comes from the separate decoded file.
- An analog-bank unit whose id is not in the model's table is reported as a
  `Fader` event named `?` rather than dropped, so a surprise on hardware is
  visible in `mtouch-listen`.
- `LedKey`'s `fast`, `slow` and `force_off` apply to the colours selected by
  the on bits, as mtouch-cli's `led_value` does; a blink flag with no colour
  encodes as `0x0000`.
- `fader_bar` masks are truncated to bits 0..9 (bits 10..22 "do nothing" per
  protocol.md 4.2); `bar_level` clamps at 10 LEDs instead of failing.
- `page_display` clamps values above 999 instead of failing.
- The reader thread forwards idle zero-length packets so `mtouch-listen` can
  count them; the NX-K reader still drops them.

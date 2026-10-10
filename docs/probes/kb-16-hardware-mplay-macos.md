# KB-16 hardware record: Martin M-Play on macOS, 2026-10-10

Live qualification of the `mtouch` module ([docs/mtouch-protocol-reuse.md](../mtouch-protocol-reuse.md)
section 6) with the M-Play attached to this service, right after the
[M-Touch record](kb-16-hardware-mtouch-macos.md). Logs: [kb-16-logs/](kb-16-logs/)
(`mplay-listen-1.log`, `mplay-led-test-1.log`, `mplay-unplug.log`, `mplay-replug.log`, `mplay-replug-2.log`).

| Item | Value |
| --- | --- |
| Host | macOS 26.5.1 (Apple silicon, `bdsmbpm401`), nusb 0.2.7, no driver installed |
| Device | `11be:f80c`, product string "M-Play", no serial; bus-powered |
| Service | branch `feat/kb16-mtouch-protocol` on top of `eda36d1`, release build sha256 `0863850c…` (same binary as the M-Touch run; the model is chosen by PID) |
| Commands | `mtpnxk mtouch-listen --seconds 150`, `--seconds 0` (unplug), `--seconds 45`/`30` (replug); `mtpnxk mtouch-led-test --hold-ms 400` |
| Protocol source | MTouchPlay `e48eb2c` `docs/mplay-notes.md` on top of `docs/protocol.md` |

Operating note: the LED test must not run while a listener holds the device: the first attempt, started
while the 150 s listen was still open, was refused with IOKit `0xe00002c5` (exclusive access) and did
nothing; the rerun after the listener exited wrote everything. One process per device.

## Input (listen session 1, 150 s, 228 events, 0 unknown packets)

| Check | Observed |
| --- | --- |
| Left keys | Beat 0x5504, Select 0x5502, Snap 0x5511, Rel 0x5503 (pressed twice), Pause 0x5512, Go 0x5513 as 5-byte single keys, same ids as the M-Touch |
| Page keys | left `-`/`+` 0x9F13/0x9F12, right `-`/`+` 0x9E13/0x9E12 as single keys |
| Bank size | every bank report is 41 bytes with unit count `0x0C` (`C2 0C …`); the decoder took N from the byte and placed units 0..11 correctly (strip 12 = unit 11 at `+0xB0`, flagged at offset 5+11) |
| Strip buttons | top buttons 0x9205 (strip 1) and 0x92B5 (strip 12), bottom buttons 0x92C5 (strip 1) and 0x9375 (strip 12), all pressure: a soft press on strip 1 top ramped 73 → 107 → 161 → 208 → 238 → 250 → 254 → 255 and back down to a `value=0 touch=0` release; the hard press jumped to 255 |
| Right block | 0x9805 (row 1 first), 0x98B5 (row 3 last = PF 12), 0x98C5 (row 4 first = PF 13), 0x9975 (row 6 last = PF 24), i.e. reading order with rows 1–3 at 0x9805 + 0x10·(n−1) and rows 4–6 at 0x98C5 + 0x10·(n−13), as documented |
| Touch faders | strip 1 0x9203 and strip 12 0x92B3: 0..255 inclusive, `touch=1` while touched, lift report `touch=0`, value rises with the finger moving up |
| Two faders at once | strips 5 and 6 (0x9243, 0x9253) in the same report, both flagged (`…0202…`) with both touch flags set (`…0101…`) |
| Resolution and cadence | 8-bit values; shortest interval between non-empty packets 7.6 ms; idle zero-length packets at 123.8 /s (18563 in 150 s); longest quiet gap 10.3 s with nothing missed afterwards |
| Not present | no 7-byte single-fader reports and no `C1` key banks, as documented for this model |

## Output (LED test, 375 writes, all accepted)

Operator's confirmation: "looked good". The six left keys lit blue (Beat and Pause red, Go green), fast-blinked,
went dark; the 24 strip buttons and the 24 right-block buttons showed red, green, fast red, off; each of the
12 bars filled 1 to 10 from the bottom in red, showed green below and red above, blinked, cleared; both page
displays (0x9F01 left, 0x9E01 right) showed 1, 42, 999, blank.

## Reconnect

| Step | Observed |
| --- | --- |
| Unplug while listening | IOKit `0xe00002ed` on the pending read, "M-Play disconnected" at 15.8 s, clean exit |
| Replug | the retry loop reopened the device after 4 one-second retries; nothing arrived until the operator's Select 6.4 s later (no startup or state report). The device then dropped once more at 9.7 s (cable reseated) and was reopened the same way |
| After the reconnect | Select press and release were waiting in one 10-byte packet at open (`0102025501 0102025500`, split into two events) and fader 1 streamed 0 → 255 → 0 with the lift report |

## Not covered here

- Windows (WinUSB) and Linux hosts; a second M-Play; the DMX bulk endpoint (never written).
- The backlight control 0x7110.
- Anything about grandMA3: this record is protocol reuse only.

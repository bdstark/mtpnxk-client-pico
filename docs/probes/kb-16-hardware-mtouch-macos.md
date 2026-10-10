# KB-16 hardware record: Martin M-Touch on macOS, 2026-10-10

Live qualification of the `mtouch` module ([docs/mtouch-protocol-reuse.md](../mtouch-protocol-reuse.md)
section 6) with the device attached to this service. Logs: [kb-16-logs/](kb-16-logs/)
(`mtouch-listen-1.log`, `mtouch-listen-2.log`, `mtouch-led-test-1.log`, `mtouch-unplug.log`,
`mtouch-replug.log`, `mtouch-replug-2.log`). The operator worked the surface by hand and confirmed the
LED test by eye; every input line in the logs carries the raw packet.

| Item | Value |
| --- | --- |
| Host | macOS 26.5.1 (Apple silicon, `bdsmbpm401`), nusb 0.2.7, no driver installed |
| Device | `11be:f808`, product string "M-Touch", no serial (as documented); bus-powered |
| Service | branch `feat/kb16-mtouch-protocol` on top of `eda36d1`, release build sha256 `0863850c…` |
| Commands | `mtpnxk mtouch-listen --seconds 120`, `--seconds 60`, `--seconds 0` (unplug), `--seconds 20`/`30` (replug); `mtpnxk mtouch-led-test --hold-ms 400` |
| Protocol source | MTouchPlay `e48eb2c` `docs/protocol.md` |

Enumeration note: right after attaching, `mtpnxk list` and the first `mtouch-listen` found no device for
some seconds (nusb's list was empty while `ioreg` already showed the device); both saw it after that.
A `run`-style open loop with a retry covers this; a one-shot open right after attach may need a retry.

## Input (listen sessions 1 and 2, 180 s, 402 events, 0 unknown packets)

| Check | Observed |
| --- | --- |
| Named keys (section 3.3) | every one reported as a 5-byte `01 02 lo hi state` single key, press 1 / release 0: `+` 0x4111, `-` 0x4113, Edit 0x5101, Clear 0x5103, Record 0x5401, Update 0x5402, Load 0x5411, Select 0x5502, Rel 0x5503, Beat 0x5504, Snap 0x5511, Pause 0x5512, Go 0x5513, HighLight 0x6001, mode key 0x6107, Last 0x6401, Next 0x6402 |
| Strip capacitive buttons | PFA 1 0x4201 and PFB 1 0x4202 as 25-byte key banks `C1 0A 01 42 10 …` / `C1 0A 02 42 10 …` with unit 0 flagged (`02`) and state 1 then 0 |
| Strip pressure button | PFD 1 0x4205 as analog bank `C2 0A 05 42 10 …`: a press reports 255 at once, then decays (254, 244, 204, 38) to a final `value=0 touch=0` release; two presses, both jumped to 255 (no soft ramp was produced by hand on this key) |
| MF keys | MF 1 0x5801 and MF 6 0x5806 as analog bank `C2 0A 01 58 01 …`: a soft press ramps up (86 → 164 → 208 → 228 → 249 → 254 → 255) over ~100 ms and down to a `touch=0` release; a hard press jumps to 255. Unit 5 (MF 6) is flagged at byte offset 5+5 as documented |
| Touch faders | strip 1 0x4203 and strip 10 0x4293 as analog bank `C2 0A 03 42 10 …`: value 0..255 inclusive at the ends, `touch=1` while touched, the last report after lifting has `touch=0` with the resting value (the lift report); direction: value rises when the finger moves up |
| Base-channel fader | 0x6112 as 7-byte `02 02 12 61 00 value touch` single fader, 0..255, lift report `touch=0`; its capacitive button 0x6111 as a single key |
| Two faders at once | strips 2 and 3 (0x4213, 0x4223) in the same analog-bank reports, both units flagged changed and both touch flags set (`…0202…0101…`); the decoder emits one event per flagged unit |
| Resolution and cadence | 8-bit values; shortest interval between non-empty packets 7.0 ms (session 1) / 7.4 ms (session 2); hand-paced fader moves averaged 61 ms between reports |
| Idle | zero-length packets at 123.1 /s (session 1: 14767 in 120 s), i.e. the documented 8 ms poll; longest gap between non-empty packets 12.1 s with nothing missed afterwards |
| Queued reports at open | session 2 opened while base faders 2 and 3 had just been moved: the first packet (t = 0.014 s) was 49 bytes holding seven concatenated 7-byte single-fader reports (`0202326100090102022261000301…`), all split and decoded. Reports made before the host polls are delivered at the first poll: a consumer must not seed levels from them |

## Output (LED test, 393 writes, all accepted)

Operator's confirmation: "everything was as described". Every K key lit blue (Beat and Pause red, Go green),
fast-blinked and went dark; the V keys (strip bottoms, MF 1–10) showed red, green, fast red, off; the four
MF mode LEDs red; each of the ten strip bars filled 1 to 10 from the bottom in red, showed green below
and red above, blinked, cleared; the four base-channel bars the blue-only variant; the page display
showed 1, 42, 999, blank. Writes used `bRequest 0x80` (`wValue 0x0100` blue, `0x0500` fast blue, `0x0003`
yellow, …), `0x61` with 9 data bytes, `0x54` with `00 00 <right> <middle> <left> 00`.

## Reconnect

| Step | Observed |
| --- | --- |
| Unplug while listening | the pending read failed with IOKit `0xe00002ed`, the tool reported "M-Touch disconnected" at 6.7 s and exited; nothing else was reported |
| Replug | the device re-enumerated with the same ids; a listen opened on it received **no report** in 20 s with the surface untouched (no startup or state dump), then, in a further 30 s session, Clear (0x5103) and fader 1 (touch, moves, lift) reported normally |

## Not covered here

- Windows (WinUSB) and Linux hosts; a second M-Touch; the DMX bulk endpoint (never written).
- The backlight control 0x7110 and the cosmetic unknowns of protocol.md section 8.
- Anything about grandMA3: this record is protocol reuse only.

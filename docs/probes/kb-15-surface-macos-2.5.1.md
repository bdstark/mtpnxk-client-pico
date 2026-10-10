# KB-15 surface record: Quickey dispatch through the NX-K surface, onPC 2.5.1.0, macOS, 2026-10-09

Live run of `mtpnxk_surface` 0.2.0 with the vendored `gma3_mcp_hardkeys` 0.10.0 (the
KB-12 bank, KB-13 owned-Quickey backend, KB-14 mode operation and KB-15 mixed
backend from bdstark/GrandMA3MCP branch `feat/kb15-mixed-backend`, revision
`3960334`). Every observation below was read through the MCP bridge's `lua` op
(`_G.__mtpnxk_surface`, `CmdObj()`, `SelectionCount()`, `ObjectList()`); the
key events came from the service's `sim` and `bench` commands.

## Environment

| Item | Value |
| --- | --- |
| Host | macOS 26.5.1, `bdsmbpm401`, one display, US layout |
| Console | onPC 2.5.1.0, show `mcp-test-disposable`, pool `Default`, user Admin, profile `Default` with **keyboard shortcuts off** during the run |
| Plugin | `mtpnxk_surface.lua` 0.2.0 (sha256 `52964121…`), hardkeys 0.10.0 `a57ebd29…`, feedback 0.2.0 `349bb2ed…`, slot 2; updated by show macro 117 (`stop`, `Delete Plugin 2`, `Import … At Plugin 2`, `ReloadAllPlugins`, bridge restart) |
| Start argument | `key=… bank=900/1.178-189 input=mixed force` (`force`: the bridge 0.12.0 ran with its keyboard input enabled; it pressed nothing) |
| Service | `service/` release build; a `run` process with the NX-K attached stayed paired throughout (id `nxk-host`); the scripted sessions were a second process (`--id nxk-kb15`, `sim`/`bench`) |
| Slots | executors 1.191–1.193 hold show objects, so the bank went on 1.178–1.189; Quickeys 900–909 were free |

A `sim` script's first event fires before the welcome arrives and is dropped by the
service as unpaired; every step below delays its first event by 300 ms.

## 1. Update and start

| # | Step | Observed |
| --- | --- | --- |
| 1 | `Go+ Macro 117`, then the start | the first attempt was refused: the reload had killed the old loop without its shutdown (`stop requested` logged, no `stopped`), ticks frozen, state still `running`, socket still bound. Fixed in 0.2.0 (stale-run takeover) and reinstalled; the second attempt logged `taking over its state`, closed the stale session and continued |
| 2 | bank provisioning | `bank provision: 10 Quickey(s) created, 0 reused`; `state=ready codes=9 (qualified 9, discovered 0) problems=0 … show='mcp-test-disposable|Default'`. Readback: Quickeys 900–908 `MCP MA1 … MCP CLEAR` with `Code` = the name, 909 `MCP RESERVED` with an empty code, executors 178–189 showing `MCP RESERVED` |
| 3 | routing report | `input enabled on the mixed backend (routing default quickkey, 31 override(s))`; `1 5 Thru Enter Record Clear` via `quickkey (quickey)`, 27 keys via `shortcut (keyboard, shortcut-table)` (`+ - . /` on their keypad rows), `keys: 33 supported, 11 unsupported` (Thru newly supported; Bank, Link, the rotaries, Swap Prog, Delay, Fade, Load, Macro as before) |
| 4 | pairing | the hardware service re-paired on its own (`session … opened for nxk-host`); the scripted sessions paired with `input Some("mixed")` and 33 keys ok |

## 2. Dispatch

| # | Script | Observed (console) | Verdict |
| --- | --- | --- | --- |
| 5 | `1:tap,Thru:tap,5:tap` | 6 events acked, 3 presses/3 releases, `cmdtext` = `1 Thru 5`, `lastcommand` = `OK: Unpress Page 1.178` (the first free reserved executor is re-read and reused for every tap) | Quickey executor taps, Thru usable |
| 6 | `Enter:tap` | `cmdtext` empty, `SelectionCount()` = 5 | PLEASE as a Quickey executes the line |
| 7 | `2:tap,3:tap` | `cmdtext` = `23`; with shortcuts off these went through the KB-14 temporary enable (restored 60 ms after the last event) | keyboard part for the explicit overrides |
| 8 | `Clear:tap` | `cmdtext` empty | CLEAR Quickey |
| 9 | `Record:down, 2:tap, Record:up` | the `2` press refused `[unqualified-mix] PC key 2 refused: Quickey STORE (hold h15 …) is held; … not a qualified chord (KB-15), nothing is dispatched`; `cmdtext` = `Store ` only | mix refused, Quickey side held |
| 10 | `2:down, Record:tap, 2:up` | the STORE press refused `[unqualified-mix] … a temporary shortcut-mode change is active (mode m5: shortcuts on for shortcut hold NUM2 …)`; `cmdtext` = `2` | mix refused, keyboard side held (the mode operation is the reason on this profile) |
| 11 | `1:down`, session abandoned | during: 1 live hold, `lastcommand` = `OK: Press Page 1.178`, `cmdtext` = `1`; 3.5 s later: 0 live, `OK: Unpress Page 1.178`, `lease expired, its holds were released` | lease release through the recorded executor |
| 12 | `5:down` abandoned, then a new hello from the same id | `replaced-by-hello: released 1 key(s)` | restart reconciliation on the Quickey part |

No unresolved record at any point (`unresolved = 0`), no `ERROR` line beyond the
expected teardown notice (section 3).

## 3. Lifecycle

| # | Step | Observed |
| --- | --- | --- |
| 13 | `bank verify` | `state=ready … problems=0` |
| 14 | `stop`, start with the same argument | `bank record mtpnxk_surface@q900.e1.178-189 kept for the next start (10 code(s)); nothing on the console was changed`, then `bank adopt: … state=ready`, `bank: … is already live (state ready); the bank= argument is not applied`; a `5:tap` afterwards pressed through 1.178 again |
| 15 | `bank teardown` while running, nothing held | `10 Quickey(s) removed, 12 executor(s) cleared, 0 skipped; state removed`; readback 0 Quickeys, 0 reserved executors; the plugin logs that Quickey routes are refused until a bank exists |
| 16 | `5:tap,2:tap` after the teardown | `5` refused `[unavailable] … no Quickey bank is provisioned on this instance (KB-12 …)`, nothing typed for it; `2` still typed through the keyboard part (`cmdtext` = `2`) | no fallback |
| 17 | `stop`, start with `bank=` again | `bank provision: 10 Quickey(s) created, 0 reused`, input enabled on the mixed backend; the hardware service re-paired | left in this state |

## 4. Bench (`mtpnxk bench --taps 100 --rate N`, Clear and 5 alternating, both Quickeys now)

| Rate | Result |
| --- | --- |
| 20 taps/s | 197 events, 188 acked, 9 refused, 54 superseded (the KB-08 flood behaviour). The refusals: `Quickey CLEAR is held and does not advertise chord (… chord = false); pressing NUM5 next to it is refused before dispatch` — at this rate the next tap arrives before the previous Quickey's release is processed, and the module refuses a chord of two Quickeys whose codes have no chord evidence |
| 10 taps/s | 198/198 acked, 0 refused, 0 superseded; ack round trip median 14.4 ms, p95 33.0 ms; press-to-effect (plugin `bench` mode) n=89 median 102 ms, p95 107 ms |
| 5 taps/s | 200/200 acked; ack median 14.5 ms; press-to-effect n=90 median 203 ms, p95 221 ms |

The press-to-effect figure tracks the tap length the bench uses at each rate
(half the interval), not the console: in step 11 the digit was on the command
line while the Quickey was still held (before the Unpress). The bench mode's
watcher evidently samples the change at the release for executor presses; this
is a measurement defect to resolve before the figure is quoted (KB-07 D1 family).
The keyboard-path figure of the KB-08 record (median 52 ms at 20 taps/s) is not
comparable.

## Not established here

- LED behaviour seen by an operator (the NX-K was attached to the other service process; no operator at the keypad).
- Show save and reload with the bank (not issued; the bank objects exist in the unsaved show until the operator saves).
- Executor reassignment during a hold, a partial bank, a changed profile during a mode operation (harness-only in the MCP repository).
- Any code beyond the nine, chords between Quickeys other than the two with evidence, Windows and Linux.

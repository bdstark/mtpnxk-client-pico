# KB-20 live check: the surface plugin serves M-Touch strip gestures on a console encoder slot (macOS, onPC 2.5.1.0)

Evidence for [KEYBOARD.md](../../KEYBOARD.md) "KB-20". **Status: not yet run.** The console (onPC with the MCP bridge
0.16.0 and `mtpnxk_surface` 0.6.0) was not up when the surface half was built on 2026-10-10; the procedure below is the
one to execute, and this file is replaced by what the console answered.

## Procedure

1. Import `mtpnxk_surface` 0.6.0 (Macro 123), start it with Macro 121 (`Plugin "mtpnxk_surface" "key=... input=fake
   control=console"`); the MCP bridge 0.16.0 runs alongside for `Fixture 401` and the programmer read-backs (its own
   control instance stays idle).
2. `Fixture 401`, `Attribute "Dimmer" At 40` through the bridge; the Dimmer bank selected.
3. The real service driven by its simulated M-Touch strips (the same code path as the USB decoder's events onward):

   ```
   mtpnxk --verbose sim --script "strip1:t100@1500,strip1:m110@40,strip1:m121@40,strip1:lift@300,strip1:t200@500,strip1:m179@40,strip1:lift@300,skey1:down@300,strip1:t50@50,strip1:m71@40,strip1:lift@300,skey1:up" --seconds 6
   ```

   Expected: touch (hold, the bridge `[busy]`), 21 counts up = `Attribute "Dimmer" At + 9` (4 + 5 with the fraction
   carried) → 49; lift; retouch elsewhere and 21 counts down = `At - 9` → 40 (no jump on the retouch); the strip key
   held: 21 counts = `At + 0.9` (fine) → 40.9.
4. Absolute mode: `mtpnxk --strips absolute --verbose sim --script "strip1:t0@1500,strip1:m60@40,strip1:m110@40,strip1:m140@40,strip1:lift@300" --seconds 4`
   with the programmer at 40.9 (context `abs` 40.9): the first moves wait for pickup, 110/255 = 0.43 crosses 0.409 →
   `Attribute "Dimmer" At 43.1373`, then `At 54.902`; the programmer reads 54.9 after the lift.
5. `ClearAll`, `ClearSelection`, Macro 122 (plugin stop).

## What happened

Not yet run.

## Not covered here

The physical M-Touch through `run` (KB-24), a mixed selection with takeover (the MCP repository's probe covers it
through the bridge), two surfaces on one slot, Windows/Linux hosts.

# KB-19 live check: the surface plugin adjusts a console encoder slot (macOS, onPC 2.5.1.0)

Evidence for [KEYBOARD.md](../../KEYBOARD.md) "KB-19". Run on 2026-10-10 with `mtpnxk_surface` 0.5.0 (plugin slot 2,
imported with Macro 123, started with Macro 121: `Plugin "mtpnxk_surface" "key=... input=fake control=console"`), the
vendored set 0.10.0 / 0.4.0 / 0.2.0 from GrandMA3MCP `107f548`, the release service at this commit, show
`mcp-test-disposable`, the MCP bridge 0.15.0 running alongside (used only to select the fixture and read the
programmer back through its `cmd` and `lua` ops; its own control instance stayed idle). Input stayed on the fake
backend (no key injection); only the control path was exercised. Re-run unchanged after the PR #23 review re-vendor
(`107f548`: attribute-editing context required, complete physical ranges only, calibration in the digest): same
script, same value 3.300005, same refusal of the push; and once more after review round 2 (`107f548`, a failed channel
discovery is incomplete coverage): same result. Round 3 (`107f548`, an unmappable channel is incomplete discovery) changed only
the feedback module's scan bookkeeping; the vendored copy matches upstream and the surface harness passes (195); the MCP
repository's live probe re-ran 39/39 on it.

## Procedure

1. `Fixture 401` through the bridge; the programmer value of `Dimmer` read back as `empty`.
2. The real service driven by its simulated NX-K (the same code path as the USB decoder's events onward):

   ```
   mtpnxk --verbose sim --script "rot1:+5@1500,rot1:-2@500,Bank:down@300,rot1:+3@200,Bank:up@200,btn1:tap@300" --seconds 5
   ```

3. The programmer value read back, then `ClearAll`, `ClearSelection`, Macro 122 (plugin stop).

## What happened

- The service paired (`control console` in the welcome) and received the plugin's context: generation 3, bank 1
  `Dimmer` page 1, 5 slots.
- Five detents, minus two, then three with Bank held reached the plugin as three `ctl` motion packets and were
  applied by the vendored console backend; the programmer value of fixture 401's Dimmer read **3.300005** afterwards:
  `5 - 2 + 3 x 0.1` from the output value 0, one detent = one Coarse click at the Percent readout, Bank = a tenth.
- The push (`btn1:tap`) was refused by the plugin with the console backend's reason, printed by the service:
  `[unsupported] button on slot1|Attribute 1 'Dimmer'|Absolute|Coarse is not served by the console backend: an
  encoder press is not served by the console backend: calculator/open/select behaviour is not qualified (nothing is
  pressed)`. Nothing was pressed on the console.
- The two `Bank` key events were acknowledged `unsupported` ("not a console key"), as before KB-18: Bank only
  modifies the rotaries.
- Service summary: `control: events=5 sent=5 coalesced=0 unbound=0 aged=0 stale=0 unsupported=0 refused=1
  lost_reported=1 superseded=0 overflow=0 queued=0 max_detent=5`; link `lost=0 rejected=0`.
- `lost_reported=1`: the release of the refused push carried the next sequence number, and the plugin reports the gap
  left by a refused event as one lost packet (the module commits a sequence number only for admitted events; this is
  KB-18 behaviour for every admission refusal, not a transport loss: the link's own `lost=0` says every packet
  arrived). Worth tightening upstream so a refused event does not read as loss.

## Not covered here

The physical NX-K rotaries (the `sim` path enters the service after the USB decoder; the device qualification with
the operator at the surface is KB-24's), `--rotary-slots 2` against a page with a fifth slot (no such page on the test
show's default pool; the mapping is unit-tested), two surfaces on one slot, Windows/Linux hosts, M-Touch strips (KB-20).

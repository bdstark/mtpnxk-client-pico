# Vendored console modules

`gma3_mcp_hardkeys.lua`, `gma3_mcp_feedback.lua` and `gma3_mcp_control.lua` are copied **unchanged**
from [bdstark/GrandMA3MCP](https://github.com/bdstark/GrandMA3MCP) (MIT, see
[LICENSE.gma3-mcp](LICENSE.gma3-mcp)). They are the console-semantics layer:
key routes, ownership and leases, read-only state readers, continuous-control admission. The surface plugin
(`mtpnxk_surface.lua`) only consumes them.

| Component | Version | Module API | Copied from |
| --- | --- | --- | --- |
| `gma3_mcp_hardkeys` | 0.10.0 | 1 | `01e1561` (branch `feat/kb20-strip-adjust`; unchanged since `3960334`: KB-12 Quickey bank, KB-13 owned-Quickey backend, KB-14 scoped shortcut-mode changes and text routes, KB-15 mixed backend); sha256 `a57ebd29af3b2c7e9e06ef3dcd8b7059c775db83b0dc61760f49e75973cc7dc3` |
| `gma3_mcp_feedback` | 0.4.0 | 1 | `01e1561` (unchanged since `107f548`; KB-19 incl. the PR #23 review: every attribute slot carries the physical range of the channel function that names the attribute, `physicalFrom/To/Range/Function/...`, the smallest over the selection, only when complete and verified (a failed channel discovery, including a nil or unreadable attribute mapping, is incomplete coverage, reviews 2 and 3), and in the binding digest; KB-17: control-context readers, `contextSnapshot()`/`watchContext()` with the binding generation); sha256 `bbed1e5432f7f8705829cda6651a62babdde4c05593c141e891b86fd8be14dd6` |
| `gma3_mcp_control` | 0.3.0 | 1 | `01e1561` (branch `feat/kb20-strip-adjust`; KB-20: strip touches on attribute slots are holds served by the console backend, positions are `Attribute "<name>" At <value>` over the verified travel (Percent 0..100, Physical From..To, never a mixed physical range), the `mixed-values` refusal with `takeover`, `position()` exported, `valueState`/`absolute` on the resolved slot; KB-19: the console adjustment backend `consoleBackend(consoleDeps(_G))`, calibrated per readout/resolution/physical range, fine a tenth, slots resolve only while the encoder bar is in attribute editing; KB-18: admission, ordering, coalescing, bounds, recovery); sha256 `56ffe3e6aa4c492a6c9a6aa01a66e114b41930f1e9178d6ea2c3f8078887a9b8` |

Previous pins: hardkeys 0.10.0 / feedback 0.4.0 / control 0.2.0 at `107f548` (KB-19); hardkeys 0.10.0 / feedback 0.3.0 / control 0.1.0 at `9f08f87` (KB-18); hardkeys 0.10.0 / feedback 0.3.0 at `c8dbb3a` (KB-17); hardkeys 0.10.0 / feedback 0.2.0 at `3960334` (KB-15); hardkeys 0.5.0 `d73a8e10…` (PR #12, `6e0d9c1`), used by the KB-07/KB-08 records.

Upstream's `plugin/modules.lock.json` carries the same hashes. Check them before
packaging (`shasum -a 256 tools/ma3/gma3_mcp_*.lua`): the version strings alone do
not identify a revision.

Rules (from that repository's `docs/modules.md`, "Vendoring into another plugin"):

1. Never edit these files here. A defect found through the surface is fixed in
   GrandMA3MCP with a regression test, then re-vendored, so console semantics
   never fork.
2. Keep them as `ComponentLua` entries after the entry component in
   `mtpnxk_surface.xml`; the console runs every component chunk at import and
   show load with the same signal table, and the entry component looks the
   modules up there in `Main` (never through `require` or globals).
3. Re-vendor with:

   ```bash
   cp ../GrandMA3/plugin/gma3_mcp_hardkeys.lua ../GrandMA3/plugin/gma3_mcp_feedback.lua ../GrandMA3/plugin/gma3_mcp_control.lua tools/ma3/
   ```

   then update the table above (version, commit, sha256) and run
   `lua tools/ma3/test/surface_plugin_test.lua` and `sh tools/ma3/test/e2e.sh`.

# Vendored console modules

`gma3_mcp_hardkeys.lua`, `gma3_mcp_feedback.lua` and `gma3_mcp_control.lua` are copied **unchanged**
from [bdstark/GrandMA3MCP](https://github.com/bdstark/GrandMA3MCP) (MIT, see
[LICENSE.gma3-mcp](LICENSE.gma3-mcp)). They are the console-semantics layer:
key routes, ownership and leases, read-only state readers, continuous-control admission. The surface plugin
(`mtpnxk_surface.lua`) only consumes them.

| Component | Version | Module API | Copied from |
| --- | --- | --- | --- |
| `gma3_mcp_hardkeys` | 0.10.0 | 1 | `874e7bd` (branch `feat/kb19-adjustment-backend`; unchanged since `3960334`: KB-12 Quickey bank, KB-13 owned-Quickey backend, KB-14 scoped shortcut-mode changes and text routes, KB-15 mixed backend); sha256 `a57ebd29af3b2c7e9e06ef3dcd8b7059c775db83b0dc61760f49e75973cc7dc3` |
| `gma3_mcp_feedback` | 0.4.0 | 1 | `874e7bd` (KB-19: every attribute slot carries the physical range of the channel function that names the attribute, `physicalFrom/To/Range/Function/...`, the smallest over the selection; KB-17: control-context readers, `contextSnapshot()`/`watchContext()` with the binding generation); sha256 `2de74c985c77c1c37e3b15666dfc3cb02d4f4279143c42e55e8d753fa8ef03c7` |
| `gma3_mcp_control` | 0.2.0 | 1 | `874e7bd` (KB-19: the console adjustment backend `consoleBackend(consoleDeps(_G))`: relative motion on attribute slots as `Attribute "<name>" At +/- <detents x step>`, calibrated per readout/resolution/physical range, fine a tenth; `supports()` refuses presses, touches, positions and executors at admission; KB-18: admission, ordering, coalescing, bounds, recovery); sha256 `43bda4a4cb026dc0f5e9c80608ee39d5083c88318219d0e1e4b686ab7bd9e4d1` |

Previous pins: hardkeys 0.10.0 / feedback 0.3.0 / control 0.1.0 at `9f08f87` (KB-18); hardkeys 0.10.0 / feedback 0.3.0 at `c8dbb3a` (KB-17); hardkeys 0.10.0 / feedback 0.2.0 at `3960334` (KB-15); hardkeys 0.5.0 `d73a8e10…` (PR #12, `6e0d9c1`), used by the KB-07/KB-08 records.

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

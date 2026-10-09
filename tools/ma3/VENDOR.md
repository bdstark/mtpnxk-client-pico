# Vendored console modules

`gma3_mcp_hardkeys.lua` and `gma3_mcp_feedback.lua` are copied **unchanged**
from [bdstark/GrandMA3MCP](https://github.com/bdstark/GrandMA3MCP) (MIT, see
[LICENSE.gma3-mcp](LICENSE.gma3-mcp)). They are the console-semantics layer:
key routes, ownership and leases, read-only state readers. The surface plugin
(`mtpnxk_surface.lua`) only consumes them.

| Component | Version | Module API | Copied from |
| --- | --- | --- | --- |
| `gma3_mcp_hardkeys` | 0.5.0 | 1 | `2d226dd` (PR #12, merged into `main` as `6e0d9c1`: generic VirtualKeyCode resolution and `prefer` for same-target ties); sha256 `d73a8e10a53260275b9bf45b9182cb5601b48843646ec1812376ff1095f6266e` |
| `gma3_mcp_feedback` | 0.2.0 | 1 | `c18559a` (unchanged through `6e0d9c1`); sha256 `349bb2ed1cc88bdbaf197aa20e957811df44306133c04652663da01a585d2cf9` |

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
   cp ../GrandMA3/plugin/gma3_mcp_hardkeys.lua ../GrandMA3/plugin/gma3_mcp_feedback.lua tools/ma3/
   ```

   then update the table above (version, commit, sha256) and run
   `lua tools/ma3/test/surface_plugin_test.lua` and `sh tools/ma3/test/e2e.sh`.

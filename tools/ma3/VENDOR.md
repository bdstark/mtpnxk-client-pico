# Vendored console modules

`gma3_mcp_hardkeys.lua` and `gma3_mcp_feedback.lua` are copied **unchanged**
from [bdstark/GrandMA3MCP](https://github.com/bdstark/GrandMA3MCP) (MIT, see
[LICENSE.gma3-mcp](LICENSE.gma3-mcp)). They are the console-semantics layer:
key routes, ownership and leases, read-only state readers. The surface plugin
(`mtpnxk_surface.lua`) only consumes them.

| Component | Version | Module API | Copied from |
| --- | --- | --- | --- |
| `gma3_mcp_hardkeys` | 0.10.0 | 1 | `c8dbb3a` (branch `feat/kb17-context-snapshot`; unchanged since `3960334`: KB-12 Quickey bank, KB-13 owned-Quickey backend, KB-14 scoped shortcut-mode changes and text routes, KB-15 mixed backend); sha256 `a57ebd29af3b2c7e9e06ef3dcd8b7059c775db83b0dc61760f49e75973cc7dc3` |
| `gma3_mcp_feedback` | 0.3.0 | 1 | `d089a87` (KB-17: control-context readers, `contextSnapshot()`/`watchContext()` with the binding generation incl. the whole selection identity and every configured function); sha256 `0e9895a278537422a7dc5f99857fbca736074bfbb60f3839244b581fe6f0d0f4` |

Previous pins: hardkeys 0.10.0 / feedback 0.2.0 at `3960334` (KB-15); hardkeys 0.5.0 `d73a8e10…` (PR #12, `6e0d9c1`), used by the KB-07/KB-08 records.

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

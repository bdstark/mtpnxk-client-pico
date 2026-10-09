# mtpnxk

Bridges Obsidian/Elation **NX-K** keypads (later the Martin M-Touch and
M-Play, which share the USB protocol) to **grandMA3 onPC**. Two parts:

| Part | Where | Role |
| --- | --- | --- |
| Surface plugin (Lua) | [tools/ma3](tools/ma3) | Runs inside onPC. Receives key events over UDP, presses console keys through the vendored `gma3_mcp_hardkeys` module, reads console state through `gma3_mcp_feedback` and sends it back for LEDs. |
| Surface service (Rust) | [service](service) | Cross-platform process that owns the USB device, speaks the surface protocol to the plugin, and renders console state onto the keypad's LEDs. |

Protocol and design: [docs/surface-protocol.md](docs/surface-protocol.md).
Feature tracking: [KEYBOARD.md](KEYBOARD.md) (KB-07 here; KB-01 to KB-06 in
[bdstark/GrandMA3MCP](https://github.com/bdstark/GrandMA3MCP)).

The original direction, a Pico W firmware hosting the keypad with HID and OSC
towards the PC, is archived under [legacy/pico](legacy/pico/ARCHIVED.md).

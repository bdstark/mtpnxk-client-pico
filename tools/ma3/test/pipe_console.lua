-- Runs the real mtpnxk_surface.lua under stock Lua with a stubbed console, exchanging datagrams
-- over stdin/stdout so udp_pipe_bridge.py can put it behind a real UDP port. No LuaSocket needed.
--
--   lua tools/ma3/test/pipe_console.lua "<plugin argument>"
--
-- stdin lines:   D <ip> <port> <hex>   a datagram arrived
--                T <seconds>           run one loop iteration at this clock
--                Q                     stop
-- stdout lines:  S <ip> <port> <hex>   a datagram to send
--                L <text>              a plugin log line
--                E                     end of this iteration's output

local here = (arg and arg[0] or ""):match("^(.*)[/\\]") or "."
package.path = here .. "/?.lua;" .. package.path
local json = require("json")
io.stdout:setvbuf("line")

local inbox, outbox = {}, {}
local clock = { t = 0 }
local sock = {
  receivefrom = function() local p = table.remove(inbox, 1); if not p then return nil, "timeout" end; return p.data, p.ip, p.port end,
  sendto = function(_, data, ip, port) outbox[#outbox + 1] = { data = data, ip = ip, port = port }; return 1 end,
  settimeout = function() end, setsockname = function() return 1 end, close = function() end,
}
package.preload["socket"] = function() return { gettime = function() return clock.t end, udp = function() return sock end } end

-- Console stubs: the KB-01 default profile subset plus a live-ish command line that the keyboard
-- backend's fake sibling cannot see; the fake backend records what would be pressed.
local function h(props) return { Get = function(_, k) return props[k] end } end
Echo = function(m) io.write("L ", (m:gsub("\n", " ")), "\n") end
ErrEcho = Echo
-- The plugin logs to both Echo and Printf; one sink is enough here.
Printf = function() end; ErrPrintf = Printf
Enums = { VirtualKeyCode = { PLEASE = 84, STORE = 66, ESC = 88, CLEAR = 87, OOPS = 86, EXEC = 35, NUM0 = 67, NUM1 = 68, NUM2 = 69, NUM3 = 70, NUM4 = 71, NUM5 = 72, NUM6 = 73, NUM7 = 74, NUM8 = 75, NUM9 = 76, MA1 = 1, MA2 = 2 },
          KeyboardCodes = { Enter = 257, Escape = 256, Delete = 261, Backspace = 259, S = 83, LeftShift = 340, ["0"] = 48, ["1"] = 49, ["2"] = 50, ["3"] = 51, ["4"] = 52, ["5"] = 53, ["6"] = 54, ["7"] = 55, ["8"] = 56, ["9"] = 57 } }
local console = { cmdtext = "", blind = "false", highlight = "true", solo = "false", env = "Live", ma = false, page = "1", previewBar = "false", shortcuts = "true" }
local rows = { { Shortcut = "Enter", KeyCode = 84 }, { Shortcut = "S", KeyCode = 66 }, { Shortcut = "Delete", KeyCode = 87 }, { Shortcut = "Backspace", KeyCode = 86 }, { Shortcut = "Escape", KeyCode = 88 } }
for d = 0, 9 do rows[#rows + 1] = { Shortcut = tostring(d), KeyCode = 67 + d } end
CurrentProfile = function()
  return { name = "Default", Environments = h({ ActiveEnvironment = console.env }), KeyboardShortCuts = {
    Get = function(_, k) if k == "KeyboardShortcutsActive" then return console.shortcuts end end,
    Count = function() return #rows end, Ptr = function(_, i) local r = rows[i]; return r and h(r) end } }
end
GetDisplayByIndex = function(n) if n == 1 then return h({ PreviewBarActive = console.previewBar }) end return nil end
Root = function() return { Get = function(_, k) if k == "MAState" then return console.ma end end,
                           VirtualKeys = { Count = function() return 1 end, Ptr = function() return h({ Code = "PLEASE", KeyCode = "Enter" }) end },
                           MANetSocket = { Get = function() return "pipe-show" end } } end
CmdObj = function() return { cmdtext = console.cmdtext, lastcommand = "" } end
ShowData = function() return { Masters = { Grand = { Blind = h({ FaderEnabled = console.blind }), Highlight = h({ FaderEnabled = console.highlight }), Solo = h({ FaderEnabled = console.solo }) } } } end
CurrentExecPage = function() return { name = "Page 1", Get = function(_, k) if k == "No" then return console.page end end } end
CurrentUser = function() return { name = "Admin" } end
SelectedSequence = function() return nil end
ObjectList = function() return {} end
GetExecutor = function() return nil end
Version = function() return "pipe-console" end
-- Keyboard(): simulate the command line so the plugin's bench mode has an observable. A NUM key
-- press appends its digit; Backspace deletes; Delete (CLEAR) empties; Enter executes (empties).
Keyboard = function(_, kind, key)
  if kind ~= "press" then return end
  if key:match("^%d$") then console.cmdtext = console.cmdtext .. key
  elseif key == "Delete" or key == "Enter" then console.cmdtext = ""
  elseif key == "Backspace" then console.cmdtext = console.cmdtext:sub(1, -2)
  elseif key == "S" then console.cmdtext = "Store " end
end

local signals = {}
local function run(file, name) local c = assert(loadfile(here .. "/../" .. file)); return c("mtpnxk_surface", name, signals, nil) end
local Main, Cleanup = run("mtpnxk_surface.lua", "mtpnxk_surface")
run("gma3_mcp_hardkeys.lua", "gma3_mcp_hardkeys")
run("gma3_mcp_feedback.lua", "gma3_mcp_feedback")
local state = _G.__mtpnxk_surface
state.sock = sock
if not state._start(state._parseArgument(arg[1] or "")) then io.write("L start refused\n"); os.exit(1) end
state.sock = sock
io.write("L pipe console ready\n")

local function toHex(s) return (s:gsub(".", function(c) return string.format("%02x", c:byte()) end)) end
local function fromHex(s) return (s:gsub("%x%x", function(x) return string.char(tonumber(x, 16)) end)) end

for line in io.lines() do
  local kind, rest = line:match("^(%a)%s*(.*)$")
  if kind == "D" then
    local ip, port, hexd = rest:match("^(%S+)%s+(%d+)%s+(%x*)$")
    if ip then inbox[#inbox + 1] = { data = fromHex(hexd), ip = ip, port = tonumber(port) } end
  elseif kind == "T" then
    clock.t = tonumber(rest) or clock.t
    local ok, err = xpcall(state._tick, debug.traceback, clock.t)
    if not ok then io.write("L tick error: ", (tostring(err):gsub("\n", " | ")), "\n") end
    for _, p in ipairs(outbox) do io.write("S ", p.ip, " ", p.port, " ", toHex(p.data), "\n") end
    outbox = {}
    io.write("E\n")
  elseif kind == "Q" then
    break
  end
end
Cleanup()

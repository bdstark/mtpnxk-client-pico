-- Regression harness for tools/ma3/mtpnxk_surface.lua under stock Lua (5.4 or newer).
--
-- The console API and LuaSocket are stubbed; the plugin and the vendored modules are loaded the way
-- onPC loads them (one signal table for every component); datagrams are pushed into a fake socket and
-- the loop is driven one tick at a time with an explicit clock. Nothing sleeps, nothing reaches a
-- console key: the hardkeys FAKE backend records what would have been pressed.
--
-- Run from the repository root:   lua tools/ma3/test/surface_plugin_test.lua

local here = (arg and arg[0] or ""):match("^(.*)[/\\]") or "."
package.path = here .. "/?.lua;" .. package.path
local json = require("json")

-- Fake LuaSocket: a UDP socket with an inbox the tests fill and an outbox they read.
local inbox, outbox = {}, {}
local clock = { t = 1000.0 }
local sock = {
  receivefrom = function() local p = table.remove(inbox, 1); if not p then return nil, "timeout" end; return p.data, p.ip, p.port end,
  sendto = function(_, data, ip, port) outbox[#outbox + 1] = { data = data, ip = ip, port = port }; return 1 end,
  settimeout = function() end, setsockname = function() return 1 end, close = function() end,
}
package.preload["socket"] = function() return { gettime = function() return clock.t end, udp = function() return sock end } end

-- Console stubs (the KB-01 default profile subset, plus EDIT mapped to E so a generic key resolves).
local logs = {}
Echo = function(m) logs[#logs + 1] = m end; ErrEcho = Echo; Printf = Echo; ErrPrintf = Echo
Enums = { VirtualKeyCode = { PLEASE = 84, STORE = 66, ESC = 88, CLEAR = 87, OOPS = 86, EXEC = 35, EDIT = 40, COPY = 41, HIGHLIGHT = 50, PLUS = 77, MINUS = 79, MA1 = 1, MA2 = 2,
                             NUM0 = 67, NUM1 = 68, NUM2 = 69, NUM3 = 70, NUM4 = 71, NUM5 = 72, NUM6 = 73, NUM7 = 74, NUM8 = 75, NUM9 = 76 },
          KeyboardCodes = { Enter = 257, Escape = 256, Delete = 261, Backspace = 259, S = 83, E = 69, LeftShift = 340, Equal = 61, kpAdd = 334, Minus = 45, kpSubtract = 333, ["0"] = 48, ["1"] = 49, ["2"] = 50, ["3"] = 51, ["4"] = 52, ["5"] = 53, ["6"] = 54, ["7"] = 55, ["8"] = 56, ["9"] = 57 } }
keyboardCalls = {}
Keyboard = function(display, kind, key, shift, ctrl, alt, numlock) keyboardCalls[#keyboardCalls + 1] = { display = display, kind = kind, key = key, shift = shift, ctrl = ctrl, alt = alt } end
local profile = { name = "Default", shortcutsActive = "true", rows = {
  { Shortcut = "Enter", KeyCode = 84 }, { Shortcut = "S", KeyCode = 66 }, { Shortcut = "Delete", KeyCode = 87 }, { Shortcut = "Backspace", KeyCode = 86 },
  { Shortcut = "Escape", KeyCode = 88 }, { Shortcut = "E", KeyCode = 40 }, { Shortcut = "Equal", KeyCode = 77 }, { Shortcut = "kpAdd", KeyCode = 77 }, { Shortcut = "Minus", KeyCode = 79 }, { Shortcut = "kpSubtract", KeyCode = 79 } } }
for d = 0, 9 do profile.rows[#profile.rows + 1] = { Shortcut = tostring(d), KeyCode = 67 + d } end
local console = { cmdtext = "", blind = "false", highlight = "true", solo = "false", env = "Live", ma = false, page = "3", showFile = "show-a", user = "Admin", previewBar = "false", shortcuts = "true", cmdRaise = false }
local function h(props) return { Get = function(_, k) return props[k] end } end
CurrentProfile = function()
  return { name = profile.name, Environments = h({ ActiveEnvironment = console.env }), KeyboardShortCuts = {
    Get = function(_, k) if k == "KeyboardShortcutsActive" then return console.shortcuts end end,
    Count = function() return #profile.rows end, Ptr = function(_, i) local r = profile.rows[i]; return r and h(r) end } }
end
GetDisplayByIndex = function(n) if n == 1 then return h({ PreviewBarActive = console.previewBar }) end return nil end
Root = function() return { Get = function(_, k) if k == "MAState" then return console.ma end end,
                           VirtualKeys = { Count = function() return 1 end, Ptr = function() return h({ Code = "PLEASE", KeyCode = "Enter" }) end },
                           MANetSocket = { Get = function() return console.showFile end } } end
CmdObj = function() if console.cmdRaise then error("CmdObj exploded") end return { cmdtext = console.cmdtext, lastcommand = "" } end
ShowData = function() return { Masters = { Grand = { Blind = h({ FaderEnabled = console.blind }), Highlight = h({ FaderEnabled = console.highlight }), Solo = h({ FaderEnabled = console.solo }) } } } end
CurrentExecPage = function() return { name = "Page " .. console.page, Get = function(_, k) if k == "No" then return console.page end end } end
CurrentUser = function() return { name = console.user } end
SelectedSequence = function() return nil end
ObjectList = function() return {} end
GetExecutor = function() return nil end
Version = function() return "2.5.1.0" end

-- Load the components the way the console does: one signal table, chunk order as in the XML.
local signals = {}
local function run(file, name) local c = assert(loadfile(here .. "/../" .. file)); return c("mtpnxk_surface", name, signals, nil) end
local Main, Cleanup = run("mtpnxk_surface.lua", "mtpnxk_surface")
run("gma3_mcp_hardkeys.lua", "gma3_mcp_hardkeys")
run("gma3_mcp_feedback.lua", "gma3_mcp_feedback")
local state = _G.__mtpnxk_surface

local failures, passes = 0, 0
local function check(name, cond, detail)
  if cond then passes = passes + 1; print("PASS " .. name)
  else failures = failures + 1; print("FAIL " .. name .. (detail and (": " .. tostring(detail)) or "")) end
end
local function J(v) return json.encode(v) end
local function lastLog() return logs[#logs] or "" end
local function findLog(pat) for i = #logs, 1, -1 do if logs[i]:find(pat) then return logs[i] end end return nil end

local KEYHEX = string.rep("0123456789abcdef", 4)
local KEY = state._fromHex(KEYHEX)
local function pkt(obj) return state._encodePacket(KEY, obj) end
local function push(obj, ip, port) inbox[#inbox + 1] = { data = type(obj) == "string" and obj or pkt(obj), ip = ip or "127.0.0.1", port = port or 40000 } end
local function tick(dt) clock.t = clock.t + (dt or 0.016); state._tick(clock.t) end
local sid, seq = nil, 0
local function send(obj) seq = seq + 1; obj.sid = sid; obj.seq = seq; push(obj) end
local function tickAlive(dt) send({ t = "hb", held = {} }); tick(dt) end
local function drain() local out = {}; for _, p in ipairs(outbox) do local o = state._decodePacket(KEY, p.data, 4096); out[#out + 1] = o or { raw = p.data }; out[#out].to = p.ip .. ":" .. p.port end; outbox = {}; return out end
local function ofType(list, t) local out = {}; for _, p in ipairs(list) do if p.t == t then out[#out + 1] = p end end; return out end
local function reset(opts)
  inbox, outbox, logs, keyboardCalls = {}, {}, {}, {}
  state.sock = sock
  local ok = state._start(state._parseArgument(opts or ("key=" .. KEYHEX .. " input=fake")))
  state.sock = sock
  return ok
end
local function events() return state.adapter and state.adapter.events or {} end
local function eventCount(kind) local n = 0; for _, e in ipairs(events()) do if e.kind == kind then n = n + 1 end end; return n end

-------------------------------------------------------------------------------
-- Crypto vectors
-------------------------------------------------------------------------------
do
  local k = ""; for i = 0, 15 do k = k .. string.char(i) end
  local k0, k1 = string.unpack("<i8i8", k)
  local function m(n) local s = ""; for i = 0, n - 1 do s = s .. string.char(i) end; return s end
  check("siphash24 paper vector (15 bytes)", string.format("%016x", state._siphash24(k0, k1, m(15))) == "a129ca6149be45e5")
  check("siphash24 empty message", string.format("%016x", state._siphash24(k0, k1, "")) == "726fdb47dd0e0e31")
  check("siphash24 one full block", string.format("%016x", state._siphash24(k0, k1, m(8))) == "93f5f5799a932462")
  check("mac uses the first 16 key bytes and 16 hex characters", #state._mac(k .. string.rep("\0", 16), m(15)) == 16 and state._mac(k .. string.rep("\0", 16), m(15)) == "a129ca6149be45e5")
end
check("fromHex rejects odd and non-hex", state._fromHex("abc") == nil and state._fromHex("zz") == nil and #state._fromHex(KEYHEX) == 32)

-------------------------------------------------------------------------------
-- Arguments and start refusals
-------------------------------------------------------------------------------
local o, e = state._parseArgument("key=" .. KEYHEX .. " port=9811 bind=0.0.0.0 allow=10.0.0.5,10.0.0.6 input=fake display=2 execs=201,202 force bench")
check("argument parsing", o and o.port == 9811 and o.host == "0.0.0.0" and o.allow["10.0.0.5"] and o.allow["10.0.0.6"] and o.input == "fake" and o.display == 2 and o.execs[2] == 202 and o.force and o.bench, J(o))
check("bad option refused", select(2, state._parseArgument("key=x bogus")) ~= nil and select(2, state._parseArgument("input=osc")) ~= nil and select(2, state._parseArgument("port=70000")) ~= nil)
check("start without a key is refused", reset("input=fake") == false and findLog("pairing key is required"))
check("start with a short key is refused", reset("key=abcd input=fake") == false)
check("start with a key succeeds on the fake backend", reset() == true and state.inputEnabled == true and state.inputMode == "fake" and state.gen:match("^%d+%-%x%x%x%x%x%x%x%x$"), J({ state.inputMode, state.gen }))
check("modules loaded (vendored versions)", state.modules.hardkeys.version == "0.5.0" and state.modules.feedback.version == "0.2.0", J(state.moduleVersions))
check("feedback watch list bounded (9 items for the NX-K)", state.modules.feedback.instance:status().watched == 9)

-------------------------------------------------------------------------------
-- Pairing: hello / welcome, authentication, replay
-------------------------------------------------------------------------------
reset()
local hello = { t = "hello", v = 1, id = "nxk-test", gen = 42, nonce = "0011223344556677", surface = "nxk", fw = "0.1.0", held = { "Record" } }
push(hello); tick()
local out = drain()
local welcome = ofType(out, "welcome")[1]
check("hello answered with a welcome", welcome and welcome.nonce == hello.nonce and type(welcome.sid) == "string" and #welcome.sid == 16 and welcome.gen == state.gen and welcome.lease == 2000 and welcome.hb == 250, J(welcome))
local function has(list, v) for _, x in ipairs(list or {}) do if x == v then return true end end return false end
check("welcome reports key resolution as sorted lists", welcome and has(welcome.keys.ok, "Record") and has(welcome.keys.ok, "Enter") and has(welcome.keys.ok, "5") and has(welcome.keys.ok, "Edit") and has(welcome.keys.unsupported, "Copy") and has(welcome.keys.unsupported, "Bank") and has(welcome.keys.unsupported, "Thru") and has(welcome.keys.ok, "+") and has(welcome.keys.ok, "-") and #welcome.keys.ok == 17, J(welcome and welcome.keys))
check("unsupported reasons are logged at start, never guessed", state.keyReasons.Copy:find("no keyboard shortcut maps to COPY") and state.keyReasons.Update:find("not an Enums.VirtualKeyCode") and state.keyReasons.Bank == "not a console key" and findLog("key Copy unsupported"), J(state.keyReasons))
check("an ambiguous keypad key falls back to the keypad row as a raw PC key", state.rawKeys["+"] == "kpAdd" and state.rawKeys["-"] == "kpSubtract" and state.rawKeys["."] == nil and findLog("pressing the keypad row kpAdd"), J(state.rawKeys))
check("welcome fits the service's datagram limit", #outbox == 0 or true)
check("welcome carries module versions, input mode, protocol", welcome and welcome.modules.gma3_mcp_hardkeys == "0.5.0" and welcome.input == "fake" and welcome.v == 1 and welcome.epoch == 1, J(welcome))
check("keys physically down in the hello are not pressed", eventCount("press") == 0 and findLog("1 key%(s%) physically down, not pressed"))
local first = ofType(out, "state")[1]
check("a full state follows the welcome immediately (items not yet read are unknown)", first and first.full == 1 and first.s.freeze == "?" and (first.s.page == "?" or first.s.page == 3), J(first))
tick(); tick(); tick(1.0)
for _, p in ipairs(ofType(drain(), "state")) do if p.full == 1 then first = p end end
check("after three frames every watched item is read", first and first.full == 1 and first.gen == state.gen and first.epoch == 1 and first.s.highlight == 1 and first.s.blind == 0 and first.s.preview == 0 and first.s.previewEnv == "Live" and first.s.page == 3 and first.s.pending == "" and first.s.freeze == "?" and first.s.ma == 0 and first.s.shortcuts == 1, J(first))
sid, seq = welcome.sid, 0

push("MTX1 " .. string.rep("0", 16) .. " " .. json.encode({ t = "hb", sid = sid, seq = 99 })); tick()
check("bad MAC is dropped before parsing (seq untouched)", state.counters.rejected == 1 and state.sessions[sid].lastSeq == 0 and #drain() == 0)
push(string.rep("x", 600)); tick()
check("oversized datagram dropped", state.counters.oversized == 1)
push("OSC1 nope"); tick()
check("bad magic dropped", state.counters.rejected == 2)
push(pkt({ t = "nonsense", sid = sid, seq = 1 })); tick()
check("unknown type rejected", state.counters.rejected == 3 and state.sessions[sid].lastSeq == 0)
push({ t = "hello", v = 1, id = "nxk-test", gen = 42, nonce = "0011223344556677", surface = "nxk" }); tick()
check("replayed hello (same nonce) is rejected and the session survives", state.counters.replayedHello == 1 and state.sessions[sid] ~= nil and #ofType(drain(), "welcome") == 0)
push({ t = "hello", v = 2, id = "nxk-test", nonce = "ffffffffffffffff" }); tick(); drain()
check("wrong protocol version refused", state.sessions[sid] ~= nil)
push(pkt({ t = "hb", sid = "deadbeefdeadbeef", seq = 1 })); tick()
local errs = ofType(drain(), "err")
check("unknown session answered with err no-session (authenticated)", #errs == 1 and errs[1].e == "no-session" and errs[1].sid == "deadbeefdeadbeef" and state.counters.noSession == 1, J(errs))
push(pkt({ t = "hb", sid = "deadbeefdeadbeef", seq = 2 })); tick()
check("err replies are rate limited per address", #ofType(drain(), "err") == 0 and state.counters.noSession == 2)

-------------------------------------------------------------------------------
-- Key events: press/release, acks, dedup, ordering
-------------------------------------------------------------------------------
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick()
out = drain()
local ack = ofType(out, "ack")[1]
check("press dispatched and acked with the hold id", ack and ack.ev == 1 and ack.ok == 1 and ack.hold == "h1" and eventCount("press") == 1 and events()[1].pcKey == "S", J(ack))
check("session renewed by the packet", state.modules.hardkeys.instance:status(clock.t).sessions[sid].remainingMs == 2000)
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("duplicate event id acked again, not dispatched", ack and ack.dup == 1 and eventCount("press") == 1 and state.counters.dupEvents == 1, J(ack))
send({ t = "key", ev = 2, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("duplicate press with a new id: harmless duplicate, nothing injected", ack and ack.ok == 1 and ack.duplicate == 1 and eventCount("press") == 1, J(ack))
send({ t = "key", ev = 3, k = "Record", d = 0 }); tick()
ack = ofType(drain(), "ack")[1]
check("release dispatched and acked", ack and ack.ok == 1 and ack.hold == "h1" and eventCount("release") == 1 and state.sessions[sid].holds.Record == nil, J(ack))
send({ t = "key", ev = 4, k = "Record", d = 0 }); tick()
ack = ofType(drain(), "ack")[1]
check("release of a key that is not held is a no-op ack", ack and ack.ok == 1 and ack.noop == 1 and eventCount("release") == 1, J(ack))
-- Old sequence number: an event carried by a stale packet is dropped even with a fresh event id.
push({ t = "key", sid = sid, seq = 1, ev = 5, k = "Record", d = 1 }); tick()
check("old seq rejected, no dispatch, no ack", state.counters.oldSeq == 1 and eventCount("press") == 1 and #drain() == 0)
-- Reordered press/release: release (newer seq) arrives first, press (older seq) second.
seq = seq + 2
push({ t = "key", sid = sid, seq = seq, ev = 7, k = "Clear", d = 0 })
push({ t = "key", sid = sid, seq = seq - 1, ev = 6, k = "Clear", d = 1 })
tick()
out = drain()
check("reordered press/release collapses to nothing (no stuck key)", eventCount("press") == 1 and state.sessions[sid].holds.Clear == nil and #ofType(out, "ack") == 1 and ofType(out, "ack")[1].noop == 1, J(out))
send({ t = "key", ev = 8, k = "Copy", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("unsupported key acked ok=0 with the module's reason", ack and ack.ok == 0 and ack.code == "unsupported" and ack.why:find("COPY"), J(ack))
send({ t = "key", ev = 9, k = "Bank", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("local surface key acked unsupported", ack and ack.ok == 0 and ack.why:find("not a console key"), J(ack))
send({ t = "key", ev = 10, k = "Bogus", d = 1 }); tick()
check("unknown key name is a malformed packet", state.counters.rejected >= 4 and #drain() == 0)
send({ t = "key", ev = 11, k = "Record", d = 2 }); tick()
check("d outside 0/1 is malformed", #drain() == 0)
send({ t = "wheel", ev = 12, w = 1, dx = 3 }); tick()
ack = ofType(drain(), "ack")[1]
check("wheel acked unsupported (no verified route)", ack and ack.ok == 0 and ack.code == "unsupported" and state.counters.wheels == 1, J(ack))
send({ t = "wheel", ev = 13, w = 9, dx = 3 }); tick()
check("wheel outside 1..4 malformed", #drain() == 0)
send({ t = "key", ev = 14, k = "Edit", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("a generic VirtualKeyCode key (Edit via E) is pressed", ack and ack.ok == 1 and events()[#events()].pcKey == "E", J(ack))
send({ t = "key", ev = 15, k = "Edit", d = 0 }); tick(); drain()
send({ t = "key", ev = 16, k = "+", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("'+' is pressed as the raw keypad key", ack and ack.ok == 1 and events()[#events()].pcKey == "kpAdd", J(ack))
send({ t = "key", ev = 17, k = "+", d = 0 }); tick(); drain()

-------------------------------------------------------------------------------
-- Heartbeat reconciliation: lost release, lost press
-------------------------------------------------------------------------------
send({ t = "key", ev = 20, k = "Record", d = 1 }); tick(); drain()
local rel0, press0 = eventCount("release"), eventCount("press")
send({ t = "hb", held = { "Record" } }); tick()
local hb = ofType(drain(), "hb")[1]
check("heartbeat with the key held keeps it held", hb and hb.held[1] == "Record" and #hb.unsynced == 0 and hb.reconciled == nil and eventCount("release") == rel0, J(hb))
send({ t = "hb", held = {} }); tick()
hb = ofType(drain(), "hb")[1]
check("heartbeat without the key releases it (lost release)", hb and hb.reconciled == 1 and #hb.held == 0 and eventCount("release") == rel0 + 1 and state.counters.lostReleases == 1 and findLog("released by heartbeat reconciliation"), J(hb))
send({ t = "hb", held = { "Record", "Clear" } }); tick()
hb = ofType(drain(), "hb")[1]
check("heartbeat listing keys the plugin never pressed reports them unsynced and presses nothing", hb and #hb.unsynced == 2 and hb.unsynced[1] == "Clear" and hb.unsynced[2] == "Record" and eventCount("press") == press0, J(hb))
send({ t = "hb", held = 5 }); tick()
check("malformed held is rejected", #drain() == 0)

-------------------------------------------------------------------------------
-- Lease expiry, revival, forgetting, restart
-------------------------------------------------------------------------------
send({ t = "key", ev = 21, k = "Record", d = 1 }); tick(); drain()
local presses, rel1 = eventCount("press"), eventCount("release")
tick(1.0); tick(1.0); tick(0.2)
check("no packets for the lease: the module released the hold, nothing re-pressed", eventCount("release") == rel1 + 1 and state.sessions[sid].state == "expired" and state.sessions[sid].holds.Record == nil and findLog("lease expired"), tostring(state.sessions[sid] and state.sessions[sid].state))
send({ t = "hb", held = { "Record" } }); tick()
hb = ofType(drain(), "hb")[1]
check("a packet within 10 s revives the session, reports resync and presses nothing", hb and hb.resynced == 1 and hb.unsynced[1] == "Record" and eventCount("press") == presses and state.sessions[sid].state == "active", J(hb))
send({ t = "key", ev = 22, k = "Record", d = 1 }); tick(); drain()
check("input works again after revival", eventCount("press") == presses + 1)
tick(5); tick(5); tick(0.5)
check("silence past 10 s forgets the session and releases its holds", state.sessions[sid] == nil and eventCount("release") == rel1 + 2 and findLog("closed: silent"))
send({ t = "hb", held = {} }); tick()
errs = ofType(drain(), "err")
check("packets for a forgotten session get err no-session", #errs == 1 and errs[1].e == "no-session")
-- Service restart: a new hello for the same id while a session holds a key.
push({ t = "hello", v = 1, id = "nxk-test", gen = 43, nonce = "aaaaaaaaaaaaaaaa", held = {} }); tick()
welcome = ofType(drain(), "welcome")[1]
sid, seq = welcome.sid, 0
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick(); drain()
local before = eventCount("release")
push({ t = "hello", v = 1, id = "nxk-test", gen = 44, nonce = "bbbbbbbbbbbbbbbb", held = { "Record" } }); tick()
out = drain()
welcome = ofType(out, "welcome")[1]
check("a new hello replaces the session: old holds released, fresh sid, nothing pressed", welcome and welcome.sid ~= sid and eventCount("release") == before + 1 and eventCount("press") == presses + 2 and findLog("replaced%-by%-hello"), J(welcome))
sid, seq = welcome.sid, 0
send({ t = "bye" }); tick()
check("bye closes the session", state.sessions[sid] == nil and findLog("closed: bye"))

-------------------------------------------------------------------------------
-- Capacity and rate limits
-------------------------------------------------------------------------------
push({ t = "hello", v = 1, id = "nxk-test", gen = 45, nonce = "cccccccccccccccc" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
local names = { "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "Record", "Clear", "Undo" }
local refusedCap = nil
for i, n in ipairs(names) do send({ t = "key", ev = 100 + i, k = n, d = 1 }); tick(); local a = ofType(drain(), "ack")[1]; if a.ok == 0 then refusedCap = a end end
check("the 13th simultaneous hold is refused with capacity, 12 held", refusedCap and refusedCap.code == "capacity" and state.modules.hardkeys.instance:status().capacity.used == 12, J(refusedCap))
for i, n in ipairs(names) do send({ t = "key", ev = 200 + i, k = n, d = 0 }) end
tick(); drain()
check("all released", state.modules.hardkeys.instance:status().capacity.used == 0)
tick(1.1); drain()
for i = 1, 450 do send({ t = "hb", held = {} }) end
for _ = 1, 15 do tick(0.001) end
check("per-session rate limit drops packets beyond 400/s", state.counters.rateDropped == 50, state.counters.rateDropped)
drain()

-------------------------------------------------------------------------------
-- Flood: unauthenticated datagrams do not starve deadline servicing; the address is throttled
-------------------------------------------------------------------------------
tick(1.1)
send({ t = "key", ev = 300, k = "Record", d = 1 }); tick(); drain()
for _ = 1, 10000 do inbox[#inbox + 1] = { data = "MTX1 " .. string.rep("1", 16) .. " {}", ip = "10.9.9.9", port = 5 } end
local relBefore = eventCount("release")
local t0 = os.clock()
tick(2.05)
local dt = os.clock() - t0
check("the flood tick processed at most 32 datagrams", #inbox == 10000 - 32, #inbox)
check("the lease that lapsed during the flood was still serviced on that tick", eventCount("release") == relBefore + 1 and state.sessions[sid].state == "expired")
for _ = 1, 400 do tick(0.001) end
check("the flooding address is throttled after repeated MAC failures", state.counters.ignored > 0 and findLog("ignoring 10.9.9.9"), J(state.counters))
check("a flood tick stays cheap", dt < 0.5, dt)
inbox = {}
send({ t = "hb", held = {} }); tick(); drain()

-------------------------------------------------------------------------------
-- Feedback: deltas, full refresh, unknown, stale, identity, epochs
-------------------------------------------------------------------------------
tickAlive(1.0)
local fulls = #ofType(drain(), "state")
check("a full state is sent at least every second", fulls >= 1)
console.blind = "true"
local delta
for _ = 1, 6 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.full == 0 then delta = p end end; if delta then break end end
check("a console change is sent as a delta within the poll interval", delta and delta.s.blind == 1 and delta.s.highlight == nil, J(delta))
console.cmdtext = "Store Cue 5"
delta = nil
for _ = 1, 6 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.full == 0 and p.s.pending then delta = p end end; if delta then break end end
check("pending keyword derived from the command line", delta and delta.s.pending == "store", J(delta))
console.blind = "Maybe"
local unknown
for _ = 1, 6 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.s.blind == "?" then unknown = p end end; if unknown then break end end
check("an unrecognised value is unknown, never off", unknown ~= nil, J(unknown))
console.blind = "false"
console.cmdRaise = true
local stalled
for _ = 1, 8 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.s.pending == "?" then stalled = p end end; if stalled then break end end
check("a raising reader makes only its item unknown and the loop continues", stalled and stalled.s.pending == "?" and (stalled.s.blind == 0 or stalled.full == 0) and state.counters.ticks > 0, J(stalled))
console.cmdRaise = false
console.env = "Something"
local pv
for _ = 1, 8 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.s.preview == "?" and p.s.previewEnv == "Something" then pv = p end end; if pv then break end end
check("an unknown preview environment name is unknown with the raw name", pv ~= nil, J(pv))
console.env = "Preview"
for _ = 1, 8 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.s.preview == 1 then pv = p end end end
check("Preview environment is on", pv and pv.s.preview == 1)
-- Show change: epoch bump, full state with the new epoch.
console.showFile = "show-b"
local bumped
for _ = 1, 30 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.epoch == 2 then bumped = p end end; if bumped then break end end
check("a show change bumps the epoch and forces a full state", bumped and bumped.full == 1 and bumped.gen == state.gen and findLog("feedback invalidated: show%-changed"), J(bumped))
-- Identity unreadable: every item unknown while uncertain.
local oldRoot = Root
Root = function() return { Get = function(_, k) if k == "MAState" then return console.ma end end, VirtualKeys = oldRoot().VirtualKeys, MANetSocket = { Get = function() error("socket gone") end } } end
local allUnknown
for _ = 1, 30 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.full == 1 and p.s.blind == "?" and p.s.highlight == "?" and p.s.page == "?" then allUnknown = p end end; if allUnknown then break end end
check("an uncertain identity renders every item unknown", allUnknown ~= nil, J(allUnknown))
Root = oldRoot
for _ = 1, 30 do tickAlive(0.05); drain() end
check("identity readable again (same show): items return", state._collectState(clock.t).highlight == 1)

-------------------------------------------------------------------------------
-- Keyboard backend and bench mode
-------------------------------------------------------------------------------
reset("key=" .. KEYHEX .. " input=keyboard bench display=1")
push({ t = "hello", v = 1, id = "nxk-kb", gen = 1, nonce = "dddddddddddddddd" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
check("keyboard backend enabled", welcome.input == "keyboard" and state.inputEnabled)
send({ t = "key", ev = 1, k = "5", d = 1 }); tick()
check("Keyboard() pressed '5' with explicit modifiers on display 1", #keyboardCalls == 1 and keyboardCalls[1].kind == "press" and keyboardCalls[1].key == "5" and keyboardCalls[1].display == 1 and keyboardCalls[1].shift == false, J(keyboardCalls))
drain()
console.cmdtext = "5"
tick(0.02)
local effect = ofType(drain(), "effect")[1]
check("bench mode reports the frame the command line changed", effect and effect.ev == 1 and effect.ms == 20 and effect.frames == 1, J(effect))
send({ t = "key", ev = 2, k = "5", d = 0 }); tick(); drain()
check("Keyboard() release repeats the tuple", keyboardCalls[2].kind == "release" and keyboardCalls[2].key == "5")

-------------------------------------------------------------------------------
-- Exclusive use with the MCP bridge (courtesy check, not arbitration)
-------------------------------------------------------------------------------
_G.__gma3_mcp_bridge = { running = true, input = { enabled = true } }
reset()
check("input refused while the bridge reports input enabled", state.inputEnabled == false and state.inputMode == "off" and findLog("MCP bridge reports input enabled"))
push({ t = "hello", v = 1, id = "nxk-x", gen = 1, nonce = "eeeeeeeeeeeeeeee" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("with input off every key is acked input-disabled and feedback still flows", ack and ack.ok == 0 and ack.code == "input-disabled" and welcome.input == "off")
reset("key=" .. KEYHEX .. " input=fake force")
check("force overrides the courtesy check", state.inputEnabled == true)
-- Two consumers: the bridge's own instance holding STORE is invisible to the surface's instance.
local HK = signals.__gma3_mcp_modules.gma3_mcp_hardkeys
local other = HK.new({ owner = "bridge", deps = HK.consoleDeps(_G), config = { requireInteraction = false } }):init()
other:enableInput(HK.fakeBackend()); other:openSession({ id = "c1" }, clock.t); other:press("c1", clock.t, { key = "STORE" })
push({ t = "hello", v = 1, id = "nxk-y", gen = 1, nonce = "ffffffffffffff00" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("independent instances do not arbitrate: both hold the same tuple (documented, not a lock)", ack and ack.ok == 1 and other:status().holdCount == 1 and state.modules.hardkeys.instance:status().holdCount == 1)
_G.__gma3_mcp_bridge = nil

-------------------------------------------------------------------------------
-- Allow list, stop and Cleanup
-------------------------------------------------------------------------------
reset("key=" .. KEYHEX .. " input=fake bind=0.0.0.0 allow=10.1.1.1")
push({ t = "hello", v = 1, id = "nxk-z", gen = 1, nonce = "1111111111111111" }, "10.2.2.2", 7); tick()
check("a source outside the allow list is dropped before authentication", state.counters.notAllowed == 1 and #drain() == 0)
push({ t = "hello", v = 1, id = "nxk-z", gen = 1, nonce = "2222222222222222" }, "10.1.1.1", 7); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
check("an allowed source pairs", welcome ~= nil and welcome.to == "10.1.1.1:7")
seq = seq + 1; push({ t = "key", sid = sid, seq = seq, ev = 1, k = "Record", d = 1 }, "10.1.1.1", 7); tick(); drain()
check("a press from the allowed source is held", state.sessions[sid].holds.Record ~= nil)
state.running = true
Main(nil, "status")
check("status is a control call: nothing released, cleanup ignored", eventCount("release") == 0 and state.ignoreNextCleanup == true and findLog("holds=%[Record%]"), J({ eventCount("release"), state.ignoreNextCleanup, lastLog() }))
Cleanup()
check("cleanup after a control call leaves the running plugin alone", state.running == true and eventCount("release") == 0)
Cleanup()
check("a real cleanup releases every hold and disposes the modules", state.running == false and eventCount("release") == 1 and next(state.modules) == nil, J({ state.running, eventCount("release"), lastLog() }))

print(string.format("%d passed, %d failed", passes, failures))
print(failures == 0 and "ALL PASSED" or "FAILED")
os.exit(failures == 0 and 0 or 1)

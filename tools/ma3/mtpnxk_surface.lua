-- mtpnxk_surface.lua
--
-- Surface plugin for grandMA3 onPC: receives key events from an mtpnxk surface service (the Rust
-- process that owns an NX-K keypad) over UDP, presses console keys through the vendored
-- gma3_mcp_hardkeys module, reads console state through the vendored gma3_mcp_feedback module and
-- sends it back for the surface's LEDs. Protocol, pairing and limits: docs/surface-protocol.md.
--
-- Command line:
--   Plugin "mtpnxk_surface" "key=<64 hex>"                 start on 127.0.0.1:9810 (input on the keyboard backend)
--   Plugin "mtpnxk_surface" "key=... port=9811 bind=0.0.0.0 allow=192.168.75.10"
--   Plugin "mtpnxk_surface" "key=... input=off"            feedback only, no console key is ever pressed
--   Plugin "mtpnxk_surface" "key=... input=fake"           lifecycle testing: events recorded, no key pressed
--   Plugin "mtpnxk_surface" "key=... force"                start even though the MCP bridge has input enabled
--   Plugin "mtpnxk_surface" "key=... bench"                report press-to-effect timing for NUM taps
--   Plugin "mtpnxk_surface" "stop" | "status" | "recover"   (recover: re-attempt unresolved key releases)
--
-- Rules this plugin keeps (KEYBOARD.md KB-07, docs/surface-protocol.md):
--   * every datagram is authenticated (SipHash-2-4 under the pairing key) before it is parsed; a sender
--     id is a label, the key is the authorization;
--   * sessions have fresh random ids, strictly increasing sequence numbers, event-id deduplication,
--     a 2 s lease renewed by any valid packet and a 10 s forget time;
--   * heartbeats release lost releases and never press anything; nothing is replayed after a restart;
--   * deadline servicing runs before packet processing, packets per iteration are bounded, feedback
--     reads are spread across frames; the loop yields once per console frame;
--   * every state item is tri-state (0, 1, "?") and carries the plugin generation and feedback epoch;
--   * the MCP bridge and this plugin do not arbitrate the console keyboard: the start refuses to
--     enable input while the bridge reports input enabled (force overrides; it is a courtesy check).

local pluginName    = select(1, ...)
local componentName = select(2, ...)
local signalTable   = select(3, ...)
local my_handle     = select(4, ...)

local socket = require("socket")
local json   = require("json")

local VERSION   = "0.1.0"
local PROTOCOL  = 1
local MAGIC     = "MTX1"
local DEFAULTS = {
  host = "127.0.0.1", port = 9810,
  leaseMs = 2000,        -- hardkeys session lease, renewed by every valid packet
  forgetMs = 10000,      -- a session without packets for this long is closed and forgotten
  hbMs = 250,            -- heartbeat interval the service is told to use
  fullStateMs = 1000,    -- full state to every session at least this often
  maxDatagram = 512,
  maxPacketsPerTick = 32,
  maxPacketsPerSecond = 400,  -- per session
  evWindow = 64,         -- event ids remembered per session for deduplication
  nonceMs = 60000,       -- hello nonces remembered this long
  authFailLimit = 50, authFailWindowMs = 10000, authIgnoreMs = 10000,
  errReplyMs = 500,      -- at most one err reply per source address per this interval
  maxHolds = 12, maxHoldMs = 60000,
  display = 1,
  benchMs = 500,         -- how long bench mode waits for a NUM tap to show on the command line
  readsPerService = 4,
}

-- State survives repeated Plugin calls (file-level locals are re-created on ReloadAllPlugins only).
_G.__mtpnxk_surface = _G.__mtpnxk_surface or { running = false, inputEnabled = false }
local state = _G.__mtpnxk_surface
state.startCount = state.startCount or 0

-------------------------------------------------------------------------------
-- Logging (System Monitor and Command Line History; never raises)
-------------------------------------------------------------------------------
local function fmt(f, ...) local ok, s = pcall(string.format, f, ...); return ok and s or tostring(f) end
local function log(f, ...)
  local line = "mtpnxk: " .. fmt(f, ...)
  if type(Echo) == "function" then pcall(Echo, line) end
  if type(Printf) == "function" then pcall(Printf, line) end
  if state.log then state.log[#state.log + 1] = line; if #state.log > 200 then table.remove(state.log, 1) end end
end
local function logerr(f, ...)
  local line = "mtpnxk ERROR: " .. fmt(f, ...)
  if type(ErrEcho) == "function" then pcall(ErrEcho, line) elseif type(Echo) == "function" then pcall(Echo, line) end
  if type(ErrPrintf) == "function" then pcall(ErrPrintf, line) elseif type(Printf) == "function" then pcall(Printf, line) end
  if state.log then state.log[#state.log + 1] = line; if #state.log > 200 then table.remove(state.log, 1) end end
end

-------------------------------------------------------------------------------
-- SipHash-2-4 (pure Lua 5.3+, 64-bit integer arithmetic wraps as the algorithm needs). The MAC of a
-- datagram is SipHash-2-4 of the JSON bytes under the first 16 bytes of the pairing key, as 16 hex
-- characters. HMAC-SHA256 was measured at 20 ms per call in onPC's Lua (2026-10-09); SipHash costs
-- about 1 ms there. Checked against the SipHash paper's reference vectors in the harness.
-------------------------------------------------------------------------------
local function rotl(x, b) return (x << b) | (x >> (64 - b)) end

local function siphash24(k0, k1, msg)
  local v0 = k0 ~ 0x736f6d6570736575
  local v1 = k1 ~ 0x646f72616e646f6d
  local v2 = k0 ~ 0x6c7967656e657261
  local v3 = k1 ~ 0x7465646279746573
  local function round()
    v0 = v0 + v1; v1 = rotl(v1, 13); v1 = v1 ~ v0; v0 = rotl(v0, 32)
    v2 = v2 + v3; v3 = rotl(v3, 16); v3 = v3 ~ v2
    v0 = v0 + v3; v3 = rotl(v3, 21); v3 = v3 ~ v0
    v2 = v2 + v1; v1 = rotl(v1, 17); v1 = v1 ~ v2; v2 = rotl(v2, 32)
  end
  local len = #msg
  local nblocks = len // 8
  for i = 0, nblocks - 1 do
    local m = string.unpack("<i8", msg, i * 8 + 1)
    v3 = v3 ~ m; round(); round(); v0 = v0 ~ m
  end
  local last = (len & 0xff) << 56
  for i = 1, len - nblocks * 8 do last = last | (msg:byte(nblocks * 8 + i) << ((i - 1) * 8)) end
  v3 = v3 ~ last; round(); round(); v0 = v0 ~ last
  v2 = v2 ~ 0xff
  round(); round(); round(); round()
  return v0 ~ v1 ~ v2 ~ v3
end

local function toHex(s) return (s:gsub(".", function(c) return string.format("%02x", c:byte()) end)) end
local function fromHex(s)
  if type(s) ~= "string" or #s % 2 ~= 0 or s:find("[^%x]") then return nil end
  return (s:gsub("%x%x", function(h) return string.char(tonumber(h, 16)) end))
end

-- key: the 32-byte pairing key (its first 16 bytes are the SipHash key). Returns 16 hex characters.
local function mac(key, text)
  local k0, k1 = string.unpack("<i8i8", key)
  return string.format("%016x", siphash24(k0, k1, text))
end

-------------------------------------------------------------------------------
-- Randomness (session ids, generation). Seeded once per start from the clock and os.time.
-------------------------------------------------------------------------------
local function seedRandom()
  local t = 0
  pcall(function() t = socket.gettime() end)
  local frac = math.floor((t - math.floor(t)) * 1e6)
  math.randomseed((os.time() * 1000003 + frac * 7919 + math.floor(os.clock() * 1e6)) & 0x7fffffff)
  for _ = 1, 8 do math.random() end
end
local function randomHex(bytes)
  local out = {}
  for _ = 1, bytes do out[#out + 1] = string.format("%02x", math.random(0, 255)) end
  return table.concat(out)
end

-------------------------------------------------------------------------------
-- Surface keys. The service sends the NX-K control names; each maps to a logical key the hardkeys
-- module resolves at start. Names marked unverified are the VirtualKeyCode names we expect; a name the
-- console's enum does not know is reported unsupported at start, never guessed.
-------------------------------------------------------------------------------
local NXK_KEYS = {
  ["0"] = "NUM0", ["1"] = "NUM1", ["2"] = "NUM2", ["3"] = "NUM3", ["4"] = "NUM4",
  ["5"] = "NUM5", ["6"] = "NUM6", ["7"] = "NUM7", ["8"] = "NUM8", ["9"] = "NUM9",
  Enter = "PLEASE", Record = "STORE", Clear = "CLEAR", Undo = "OOPS",
  -- unverified VirtualKeyCode names (resolved through the shortcut table if the profile maps them)
  Update = "UPDATE", Edit = "EDIT", Copy = "COPY", Move = "MOVE", Delete = "DELETE", Load = "LOAD",
  Cue = "CUE", Group = "GROUP", Macro = "MACRO", Fade = "FADE", Delay = "DELAY",
  HighLight = "HIGHLIGHT", Preview = "PREVIEW", Next = "NEXT", Last = "PREV", Menu = "MENU",
  Thru = "THRU", Full = "FULL", ["@"] = "AT", ["+"] = "PLUS", ["-"] = "MINUS", ["."] = "DOT", ["/"] = "SLASH",
  -- Fade, Delay, Snap Shot and Back have no MA3 hardkey (onPC 2.5.1 enum, read live 2026-10-09);
  -- they are reported unsupported rather than mapped to something else.
}
-- The default profile maps two shortcuts with equal modifier count to PLUS, MINUS, DOT and SLASH (main
-- row and keypad). Both rows target the same MA key; the module resolves such a tie only when the consumer
-- names the row (spec.prefer, hardkeys 0.5.0), and the chosen row keeps every safeguard: shortcut
-- enablement, the collision check and the route rechecks before every release.
local NXK_PREFER = { ["+"] = "kpAdd", ["-"] = "kpSubtract", ["."] = "kpDecimal", ["/"] = "kpDivide" }
-- Keys the service may send that are not console keys (acknowledged unsupported, never an error).
local NXK_LOCAL = { Bank = true, ["Swap Prog"] = true, Link = true, Rotary1 = true, Rotary2 = true, Rotary3 = true, Rotary4 = true }

-- Preview environment names (previewMode reader) that mean on/off; anything else is unknown.
local PREVIEW_ENV = { Preview = 1, Live = 0, Normal = 0 }

-------------------------------------------------------------------------------
-- Module loading (KB-02 contract: the console runs every component chunk with the same signal table)
-------------------------------------------------------------------------------
local MODULE_REGISTRY_KEY = "__gma3_mcp_modules"
local MODULE_API_VERSION  = 1

local function loadModule(component)
  if type(signalTable) ~= "table" then return nil, "no signal table (the plugin was not loaded by the console)" end
  local reg = rawget(signalTable, MODULE_REGISTRY_KEY)
  if type(reg) ~= "table" then return nil, "no module registered: the module components did not run (import the plugin with every component)" end
  local mod = reg[component]
  if mod == nil then return nil, "module '" .. component .. "' is not registered (check mtpnxk_surface.xml)" end
  if type(mod) ~= "table" or type(mod.new) ~= "function" or mod.API_VERSION == nil then return nil, "registered '" .. component .. "' is not a module table" end
  if mod.API_VERSION ~= MODULE_API_VERSION then return nil, string.format("module API version %s, this plugin expects %d", tostring(mod.API_VERSION), MODULE_API_VERSION) end
  return mod
end

-------------------------------------------------------------------------------
-- Argument parsing
-------------------------------------------------------------------------------
local function parseArgument(argument)
  local opts = { input = "keyboard" }
  for tok in tostring(argument or ""):gmatch("%S+") do
    local l = tok:lower()
    local k, v = tok:match("^(%a+)=(.*)$")
    if l == "stop" or l == "status" or l == "recover" then opts.command = l
    elseif l == "force" then opts.force = true
    elseif l == "bench" then opts.bench = true
    elseif k then
      k = k:lower()
      if k == "key" then opts.key = v
      elseif k == "port" then opts.port = tonumber(v); if not opts.port or opts.port < 1 or opts.port > 65535 then return nil, "port must be 1..65535" end
      elseif k == "bind" then opts.host = v
      elseif k == "allow" then opts.allow = {}; for ip in v:gmatch("[^,]+") do opts.allow[ip] = true end
      elseif k == "input" then
        local m = v:lower()
        if m ~= "keyboard" and m ~= "fake" and m ~= "off" then return nil, "input must be keyboard, fake or off" end
        opts.input = m
      elseif k == "display" then opts.display = tonumber(v); if not opts.display then return nil, "display must be a number" end
      elseif k == "execs" then opts.execs = {}; for n in v:gmatch("[^,]+") do opts.execs[#opts.execs + 1] = tonumber(n) end
      else return nil, "unknown option '" .. tok .. "'" end
    else return nil, "unknown token '" .. tok .. "'" end
  end
  return opts
end

-------------------------------------------------------------------------------
-- Encoding and sending
-------------------------------------------------------------------------------
local function encodePacket(key, obj)
  local text = json.encode(obj)
  return MAGIC .. " " .. mac(key, text) .. " " .. text
end
state._encodePacket = encodePacket

-- Returns the parsed object or nil, reason. The MAC is verified on the exact bytes before parsing.
local function decodePacket(key, data, maxLen)
  if type(data) ~= "string" then return nil, "not a string" end
  if #data > (maxLen or DEFAULTS.maxDatagram) then return nil, "oversized" end
  if data:sub(1, 5) ~= MAGIC .. " " then return nil, "bad-magic" end
  local given, text = data:match("^MTX1 (%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x) (.*)$")
  if not given then return nil, "bad-frame" end
  if mac(key, text) ~= given:lower() then return nil, "bad-mac" end
  local ok, obj = pcall(json.decode, text)
  if not ok or type(obj) ~= "table" then return nil, "bad-json" end
  if type(obj.t) ~= "string" then return nil, "no-type" end
  return obj
end
state._decodePacket = decodePacket

local function sendTo(ip, port, obj)
  local data = encodePacket(state.key, obj)
  state.counters.sent = state.counters.sent + 1
  local ok, err = pcall(function() return state.sock:sendto(data, ip, port) end)
  if not ok or err == nil then state.counters.sendErrors = state.counters.sendErrors + 1 end
end

local function sendToSession(s, obj)
  s.outSeq = s.outSeq + 1
  obj.sid, obj.seq = s.sid, s.outSeq
  sendTo(s.ip, s.port, obj)
end

-------------------------------------------------------------------------------
-- Sessions
-------------------------------------------------------------------------------
local function hk() local r = state.modules.hardkeys; return r and r.instance end
local function fb() local r = state.modules.feedback; return r and r.instance end

local function closeSession(s, now, reason)
  local inst = hk()
  if inst and s.sessionOpen then
    local ok, r = pcall(inst.closeSession, inst, s.sid, now, reason)
    if ok and type(r) == "table" then
      for _, a in ipairs(r.unresolved or {}) do logerr("session %s (%s) %s: release of %s UNRESOLVED: %s", s.sid, s.id, reason, tostring(a.logical or a.tupleKey), tostring(a.error)) end
      if #(r.released or {}) > 0 then log("session %s (%s) %s: released %d key(s)", s.sid, s.id, reason, #r.released) end
    elseif not ok then logerr("closing session %s failed: %s", s.sid, tostring(r)) end
  end
  s.sessionOpen = false
  s.state = "closed"
  state.sessions[s.sid] = nil
  if state.byId[s.id] == s then state.byId[s.id] = nil end
  log("session %s (%s from %s:%d) closed: %s", s.sid, s.id, s.ip, s.port, reason)
end

-- Resolution of every surface key, reported in the welcome as two sorted name lists (the datagram stays
-- small) and logged with the module's reason for each unsupported key.
local function describeKeys(logReasons)
  local inst = hk()
  local ok, unsupported, reasons = {}, {}, {}
  for name, logical in pairs(NXK_KEYS) do
    local supported, reason = false, "hardkeys module not loaded"
    if inst then
      local okC, r = pcall(inst.describeKey, inst, logical, { prefer = NXK_PREFER[name] })
      if okC and type(r) == "table" and r.supported then supported = true else reason = tostring(okC and (r and r.reason) or r) end
      if supported and logReasons and r.prefer then log("key %s: %s resolved through the preferred row %s (%s)", name, logical, r.prefer, tostring(r.shortcut)) end
    end
    if supported then ok[#ok + 1] = name else unsupported[#unsupported + 1] = name; reasons[name] = reason end
  end
  for name in pairs(NXK_LOCAL) do unsupported[#unsupported + 1] = name; reasons[name] = "not a console key" end
  table.sort(ok); table.sort(unsupported)
  if logReasons then
    for _, name in ipairs(unsupported) do log("key %s unsupported: %s", name, reasons[name]) end
    log("keys: %d supported, %d unsupported", #ok, #unsupported)
  end
  state.keyReasons = reasons
  return { ok = ok, unsupported = unsupported }
end

local function consoleInfo()
  local info = {}
  pcall(function() info.version = tostring(Version and Version() or nil) end)
  pcall(function() local b = BuildDetails and BuildDetails(); if b then info.build = tostring(b.ProductVersion or b.Version or b.MajorVersion) end end)
  return info
end

local function openSession(obj, ip, port, now)
  if type(obj.id) ~= "string" or obj.id == "" or #obj.id > 64 then return nil, "bad-id" end
  if type(obj.nonce) ~= "string" or #obj.nonce < 8 or #obj.nonce > 32 then return nil, "bad-nonce" end
  if obj.v ~= PROTOCOL then return nil, "bad-version" end
  if state.nonces[obj.nonce] then state.counters.replayedHello = state.counters.replayedHello + 1; return nil, "replayed-hello" end
  state.nonces[obj.nonce] = now
  local held = {}
  if type(obj.held) == "table" then
    if #obj.held > 16 then return nil, "bad-held" end
    for _, k in ipairs(obj.held) do if type(k) ~= "string" then return nil, "bad-held" end; held[#held + 1] = k end
  end
  -- A new hello for a known surface id replaces its session: a restarted service inherits nothing.
  local old = state.byId[obj.id]
  if old then closeSession(old, now, "replaced-by-hello") end
  local sid = randomHex(8)
  while state.sessions[sid] do sid = randomHex(8) end
  local s = { sid = sid, id = obj.id, gen = obj.gen, ip = ip, port = port, openedAt = now, lastSeen = now, lastSeq = 0, outSeq = 0,
              holds = {}, evSeen = {}, evOrder = {}, evAck = {}, lastEvByKey = {}, rate = { since = now, count = 0 }, state = "active",
              unsyncedHeld = held, lastFullAt = nil, lastState = nil, sessionOpen = false, surface = obj.surface, fw = obj.fw }
  local inst = hk()
  if inst and state.inputEnabled then
    local r, err = inst:openSession({ id = sid, leaseMs = DEFAULTS.leaseMs, label = obj.id, binding = ip .. ":" .. tostring(port) }, now)
    if not r then return nil, "session-open-failed: " .. tostring(err and err.message) end
    s.sessionOpen = true
  end
  state.sessions[sid] = s
  state.byId[obj.id] = s
  log("session %s opened for %s (%s fw %s, surface gen %s) from %s:%d; %d key(s) physically down, not pressed", sid, obj.id, tostring(obj.surface), tostring(obj.fw), tostring(obj.gen), ip, port, #held)
  local welcome = { t = "welcome", v = PROTOCOL, nonce = obj.nonce, gen = state.gen, lease = DEFAULTS.leaseMs, hb = DEFAULTS.hbMs,
                    keys = describeKeys(false), modules = state.moduleVersions, console = consoleInfo(), input = state.inputMode,
                    plugin = VERSION, epoch = fb() and fb():epoch() or nil }
  sendToSession(s, welcome)
  s.lastFullAt = nil  -- the next tick sends a full state
  return s
end

-- Per-session packet admission: sequence, rate. Renews the lease on success.
local function admitPacket(s, obj, now)
  if type(obj.seq) ~= "number" or obj.seq ~= math.floor(obj.seq) or obj.seq <= s.lastSeq then
    state.counters.oldSeq = state.counters.oldSeq + 1
    return false, "old-seq"
  end
  if now - s.rate.since >= 1 then s.rate.since, s.rate.count = now, 0 end
  s.rate.count = s.rate.count + 1
  if s.rate.count > DEFAULTS.maxPacketsPerSecond then state.counters.rateDropped = state.counters.rateDropped + 1; return false, "rate" end
  s.lastSeq = obj.seq
  local revived = false
  if s.state == "expired" then s.state = "active"; revived = true end
  s.lastSeen = now
  local inst = hk()
  if inst and s.sessionOpen then
    local r, err = inst:renewSession(s.sid, now, DEFAULTS.leaseMs)
    if not r then
      -- The module forgot the session (closed with nothing unresolved): reopen it. Nothing is re-pressed.
      local r2, err2 = inst:openSession({ id = s.sid, leaseMs = DEFAULTS.leaseMs, label = s.id, binding = s.ip .. ":" .. tostring(s.port) }, now)
      if not r2 then logerr("session %s: lease renewal and reopen failed: %s / %s", s.sid, tostring(err and err.message), tostring(err2 and err2.message)) end
    end
  end
  return true, revived
end

-- Event-id deduplication: a window of the last evWindow ids per session. A seen event is answered with
-- its ORIGINAL acknowledgment (a refusal stays a refusal) and is never dispatched again.
local function seenEvent(s, ev)
  if s.evSeen[ev] then return true end
  s.evSeen[ev] = true
  s.evOrder[#s.evOrder + 1] = ev
  if #s.evOrder > DEFAULTS.evWindow then
    local old = table.remove(s.evOrder, 1)
    s.evSeen[old] = nil
    s.evAck[old] = nil
  end
  return false
end

local function replayAck(s, ev)
  state.counters.dupEvents = state.counters.dupEvents + 1
  local a = s.evAck[ev]
  if a then
    local copy = {}
    for k, v in pairs(a) do copy[k] = v end
    copy.dup = 1
    sendToSession(s, copy)
  else
    -- Seen but its outcome already left the window: say so rather than invent a success.
    sendToSession(s, { t = "ack", ev = ev, ok = 0, dup = 1, code = "outcome-expired", why = "event already processed; its outcome is no longer remembered" })
  end
end

-- Event ids are monotonic per service start. A key event older than the newest one already processed
-- for the same surface key is superseded: a retransmitted press arriving after its release, or a stale
-- release arriving after a newer press, must not act. Nothing is dispatched for it.
local function superseded(s, name, ev)
  local last = s.lastEvByKey[name]
  if last and ev < last then return true end
  s.lastEvByKey[name] = ev
  return false
end

-- Drops hold bookkeeping for holds the module released on its own (deadlines, lease expiry).
local function pruneReleased(s, releasedIds)
  for name, hid in pairs(s.holds) do
    if releasedIds[hid] then s.holds[name] = nil end
  end
end

local function ackEvent(s, ev, ok, extra)
  local a = { t = "ack", ev = ev, ok = ok and 1 or 0 }
  if extra then for k, v in pairs(extra) do a[k] = v end end
  local keep = {}
  for k, v in pairs(a) do keep[k] = v end
  s.evAck[ev] = keep
  sendToSession(s, a)
end

-------------------------------------------------------------------------------
-- Packet handlers
-------------------------------------------------------------------------------
local function handleKey(s, obj, now)
  local ev, name, down = obj.ev, obj.k, obj.d
  if type(ev) ~= "number" or ev ~= math.floor(ev) or ev < 0 then return "bad-ev" end
  if type(name) ~= "string" or (NXK_KEYS[name] == nil and not NXK_LOCAL[name]) then return "bad-key" end
  if down ~= 0 and down ~= 1 then return "bad-d" end
  if seenEvent(s, ev) then replayAck(s, ev); return nil end
  if superseded(s, name, ev) then
    state.counters.superseded = state.counters.superseded + 1
    ackEvent(s, ev, false, { code = "superseded", why = "a newer event for this key was already processed; nothing dispatched" })
    return nil
  end
  local logical = NXK_KEYS[name]
  if not logical then ackEvent(s, ev, false, { code = "unsupported", why = "not a console key" }); return nil end
  local inst = hk()
  if not inst or not state.inputEnabled then ackEvent(s, ev, false, { code = "input-disabled", why = "input is " .. tostring(state.inputMode) }); return nil end
  if down == 1 then
    local r, err = inst:press(s.sid, now, { key = logical, display = state.display, prefer = NXK_PREFER[name] })
    if not r then
      state.counters.refused = state.counters.refused + 1
      ackEvent(s, ev, false, { code = tostring(err and err.code or "error"), why = tostring(err and err.message or err) })
      return nil
    end
    s.holds[name] = r.id
    state.counters.presses = state.counters.presses + 1
    if r.duplicate then ackEvent(s, ev, true, { hold = r.id, duplicate = 1 }) else ackEvent(s, ev, true, { hold = r.id }) end
    if state.bench and logical:match("^NUM%d$") then
      local before = nil
      pcall(function() before = state.commandText() end)
      state.benchWatch[#state.benchWatch + 1] = { s = s, ev = ev, at = now, before = before, tickAt = state.counters.ticks }
    end
  else
    local hid = s.holds[name]
    if not hid then ackEvent(s, ev, true, { noop = 1 }); return nil end
    local r, err = inst:release(s.sid, now, { hold = hid })
    s.holds[name] = nil
    if not r then
      if err and (err.code == "no-hold") then ackEvent(s, ev, true, { noop = 1 }); return nil end
      state.counters.refused = state.counters.refused + 1
      ackEvent(s, ev, false, { code = tostring(err and err.code or "error"), why = tostring(err and err.message or err), hold = hid })
      return nil
    end
    state.counters.releases = state.counters.releases + 1
    if r.state == "unresolved" then
      logerr("session %s: release of %s (%s) unresolved: %s", s.sid, name, hid, tostring(r.unresolved and r.unresolved.reason))
      ackEvent(s, ev, false, { code = "unresolved", why = tostring(r.unresolved and r.unresolved.reason), hold = hid })
    else
      ackEvent(s, ev, true, { hold = hid, outcome = r.releaseOutcome, alreadyReleased = r.alreadyReleased and 1 or nil })
    end
  end
  return nil
end

local function handleWheel(s, obj, now)
  local ev = obj.ev
  if type(ev) ~= "number" or ev ~= math.floor(ev) or ev < 0 then return "bad-ev" end
  if type(obj.w) ~= "number" or obj.w < 1 or obj.w > 4 or obj.w ~= math.floor(obj.w) then return "bad-wheel" end
  if type(obj.dx) ~= "number" or obj.dx < -127 or obj.dx > 127 then return "bad-dx" end
  if seenEvent(s, ev) then replayAck(s, ev); return nil end
  state.counters.wheels = state.counters.wheels + 1
  -- No verified Lua route for encoder input exists yet (docs/surface-protocol.md section 7).
  ackEvent(s, ev, false, { code = "unsupported", why = "wheel input has no verified console route yet" })
  return nil
end

-- Heartbeat: the service's view of what is physically held. Lost releases are reconciled here; lost
-- presses are reported and never pressed late.
local function handleHeartbeat(s, obj, now, revived)
  local held = {}
  if obj.held ~= nil then
    if type(obj.held) ~= "table" or #obj.held > 16 then return "bad-held" end
    for _, k in ipairs(obj.held) do if type(k) ~= "string" then return "bad-held" end; held[k] = true end
  end
  local inst = hk()
  local releasedNow = 0
  for name, hid in pairs(s.holds) do
    if not held[name] then
      s.holds[name] = nil
      if inst and state.inputEnabled then
        local r = inst:release(s.sid, now, { hold = hid })
        if r and not r.alreadyReleased then releasedNow = releasedNow + 1; state.counters.lostReleases = state.counters.lostReleases + 1
          log("session %s: %s released by heartbeat reconciliation (its release event never arrived)", s.sid, name) end
      end
    end
  end
  local pluginHeld, unsynced = {}, {}
  for name in pairs(s.holds) do pluginHeld[#pluginHeld + 1] = name end
  for name in pairs(held) do if not s.holds[name] then unsynced[#unsynced + 1] = name end end
  table.sort(pluginHeld); table.sort(unsynced)
  s.unsyncedHeld = unsynced
  local reply = { t = "hb", held = pluginHeld, unsynced = unsynced, gen = state.gen }
  if revived then reply.resynced = 1 end
  if releasedNow > 0 then reply.reconciled = releasedNow end
  sendToSession(s, reply)
  return nil
end

local HANDLERS = {
  key = handleKey, wheel = handleWheel, hb = handleHeartbeat,
  bye = function(s, _, now) closeSession(s, now, "bye"); return nil end,
}

-- Auth-failure throttle per source address: bounded work under a flood of bad datagrams.
local function authFailed(ip, now)
  local f = state.authFail[ip]
  if not f or now - f.since > DEFAULTS.authFailWindowMs / 1000 then f = { since = now, count = 0 }; state.authFail[ip] = f end
  f.count = f.count + 1
  if f.count >= DEFAULTS.authFailLimit then f.ignoreUntil = now + DEFAULTS.authIgnoreMs / 1000; f.count = 0; f.since = now
    log("ignoring %s for %d ms after repeated authentication failures", ip, DEFAULTS.authIgnoreMs) end
end

local function sendErr(ip, port, e, sid, now)
  local last = state.errReplies[ip]
  if last and now - last < DEFAULTS.errReplyMs / 1000 then return end
  state.errReplies[ip] = now
  sendTo(ip, port, { t = "err", e = e, sid = sid, gen = state.gen, seq = 0 })
end

local function handleDatagram(data, ip, port, now)
  local c = state.counters
  c.received = c.received + 1
  if type(data) ~= "string" or #data > DEFAULTS.maxDatagram then c.oversized = c.oversized + 1; return "oversized" end
  if state.allow and not state.allow[ip] then c.notAllowed = c.notAllowed + 1; return "not-allowed" end
  local f = state.authFail[ip]
  if f and f.ignoreUntil and now < f.ignoreUntil then c.ignored = c.ignored + 1; return "ignored" end
  local obj, why = decodePacket(state.key, data)
  if not obj then
    c.rejected = c.rejected + 1
    if why == "bad-mac" or why == "bad-frame" or why == "bad-magic" then authFailed(ip, now) end
    return why
  end
  if obj.t == "hello" then
    local s, err = openSession(obj, ip, port, now)
    if not s then c.rejected = c.rejected + 1; return err end
    return "hello"
  end
  if type(obj.sid) ~= "string" then c.rejected = c.rejected + 1; return "no-sid" end
  local s = state.sessions[obj.sid]
  if not s then c.noSession = c.noSession + 1; sendErr(ip, port, "no-session", obj.sid, now); return "no-session" end
  if s.ip ~= ip or s.port ~= port then
    -- A valid MAC from another address with a known sid: the service moved (DHCP) or a replay from
    -- elsewhere. The sequence check below still applies; the address follows the newest packet.
    c.addressChanged = c.addressChanged + 1
  end
  local handler = HANDLERS[obj.t]
  if not handler then c.rejected = c.rejected + 1; return "unknown-type" end
  local ok, revived = admitPacket(s, obj, now)
  if not ok then return revived end
  s.ip, s.port = ip, port
  local err = handler(s, obj, now, revived)
  if err then c.rejected = c.rejected + 1; return err end
  return obj.t
end
state._handleDatagram = handleDatagram

-------------------------------------------------------------------------------
-- Feedback: watch list, tri-state collection, delta / full sending
-------------------------------------------------------------------------------
local function watchItems()
  local FB = state.modules.feedback and state.modules.feedback.module
  if not FB then return {} end
  local spec = { readers = { "blind", "highlight", "solo", "previewMode", "previewBar", "maState", "commandText", "page", "shortcutsActive" }, displays = { state.display } }
  if #state.execs > 0 then spec.executors = state.execs end
  local items = FB.itemsFor(spec)
  return items
end

local function tri(item)
  if not item or item.available ~= true or item.stale then return "?" end
  if item.value == true then return 1 elseif item.value == false then return 0 end
  return "?"
end

local function pendingOf(item)
  if not item or item.available ~= true or item.stale then return "?" end
  local word = tostring(item.value):match("^%s*(%a+)")
  if not word then return "" end
  return word:lower():sub(1, 16)
end

-- One tri-state table from the feedback snapshot. "?" for unavailable, stale or identity-uncertain.
local function collectState(now)
  local inst = fb()
  if not inst then return {} end
  local snap = inst:snapshot(now)
  local by = {}
  for _, it in ipairs(snap.items) do by[it.key] = it end
  local uncertain = snap.identityUncertain ~= nil
  local function get(k) local it = by[k]; if uncertain and it then it = { available = it.available, value = it.value, stale = true } end return it end
  local s = {
    blind = tri(get("blind")), highlight = tri(get("highlight")), solo = tri(get("solo")),
    previewBar = tri(get("previewBar[display=" .. state.display .. "]")),
    ma = tri(get("maState")), shortcuts = tri(get("shortcutsActive")),
    pending = pendingOf(get("commandText")),
    freeze = "?",
  }
  local pm = get("previewMode")
  if pm and pm.available == true and not pm.stale then
    s.previewEnv = tostring(pm.value)
    s.preview = PREVIEW_ENV[tostring(pm.value)] or "?"
  else s.preview = "?" end
  local pg = get("page")
  if pg and pg.available == true and not pg.stale and type(pg.value) == "table" and type(pg.value.no) == "number" then s.page = pg.value.no else s.page = "?" end
  for _, n in ipairs(state.execs) do
    local ex, fd = get("executor[executor=" .. n .. "]"), get("fader[executor=" .. n .. "]")
    if fd and fd.available == true and not fd.stale and type(fd.value) == "table" and type(fd.value.value) == "number" then
      s["f" .. n] = math.floor(fd.value.value + 0.5)
    else s["f" .. n] = "?" end
    -- Activity needs the assigned sequence; the executor reader gives the assignment only. Without a
    -- sequenceActive item per assigned object this stays unknown (M-Touch work).
    s["x" .. n] = (ex and ex.available == true and not ex.stale and ex.value and ex.value.empty == false) and "?" or "?"
  end
  return s
end
state._collectState = collectState

local function stateChanged(a, b)
  if a == nil or b == nil then return true end
  for k, v in pairs(a) do if b[k] ~= v then return true end end
  for k, v in pairs(b) do if a[k] ~= v then return true end end
  return false
end

local function deltaOf(prev, cur)
  local d = {}
  for k, v in pairs(cur) do if prev[k] ~= v then d[k] = v end end
  for k in pairs(prev) do if cur[k] == nil then d[k] = "?" end end
  return d
end

local function sendStates(now)
  local epoch = fb() and fb():epoch() or 0
  local cur = state.lastState
  for _, s in pairs(state.sessions) do
    if s.state == "active" then
      local full = s.lastFullAt == nil or (now - s.lastFullAt) >= DEFAULTS.fullStateMs / 1000 or s.lastEpoch ~= epoch
      if full then
        sendToSession(s, { t = "state", gen = state.gen, epoch = epoch, full = 1, s = cur })
        s.lastFullAt, s.lastState, s.lastEpoch = now, cur, epoch
        state.counters.fullStates = state.counters.fullStates + 1
      elseif stateChanged(s.lastState, cur) then
        sendToSession(s, { t = "state", gen = state.gen, epoch = epoch, full = 0, s = deltaOf(s.lastState, cur) })
        s.lastState = cur
        state.counters.deltaStates = state.counters.deltaStates + 1
      end
    end
  end
end

-------------------------------------------------------------------------------
-- Bench mode: press-to-effect timing for NUM taps (the command line is the observable)
-------------------------------------------------------------------------------
local function serviceBench(now)
  if #state.benchWatch == 0 then return end
  local keep = {}
  for _, w in ipairs(state.benchWatch) do
    -- frames since the dispatching iteration (0 = the same iteration, 1 = the next frame)
    w.frames = state.counters.ticks - w.tickAt - 1
    local text = nil
    pcall(function() text = state.commandText() end)
    if text ~= nil and text ~= w.before then
      sendToSession(w.s, { t = "effect", ev = w.ev, ms = math.floor((now - w.at) * 1000 + 0.5), frames = w.frames })
    elseif now - w.at > DEFAULTS.benchMs / 1000 then
      sendToSession(w.s, { t = "effect", ev = w.ev, ms = -1, frames = w.frames, why = "no command-line change within " .. DEFAULTS.benchMs .. " ms" })
    else keep[#keep + 1] = w end
  end
  state.benchWatch = keep
end

-------------------------------------------------------------------------------
-- One loop iteration (section 4 of the protocol doc). Exposed for the harness.
-------------------------------------------------------------------------------
-- Takes the hardkeys instance out of service without losing what it owns: input off, a release attempt
-- for every held key, and every record that stays unresolved kept in state.unresolved for adoption by
-- the next start and for the "recover" command. Used on a service() error, at stop and in Cleanup.
local function detachHardkeys(reason, now)
  local rec = state.modules.hardkeys
  local inst = rec and rec.instance
  if not inst then return end
  local okS, st = pcall(inst.status, inst)
  if okS and type(st) == "table" and st.state == "ready" then
    state.inputEnabled = false
    local okD, dis = pcall(inst.disableInput, inst, now, reason)
    if okD and type(dis) == "table" then
      for _, a in ipairs(dis.unresolved or {}) do logerr("%s: release of %s UNRESOLVED: %s", reason, tostring(a.logical or a.tupleKey), tostring(a.error)) end
    end
  end
  local ok, r = pcall(inst.dispose, inst, now)
  if ok and type(r) == "table" then
    state.unresolved = state.unresolved or {}
    for _, record in ipairs(r.records or {}) do
      record.keptAt, record.keptReason = now, reason
      state.unresolved[#state.unresolved + 1] = record
      logerr("keeping unresolved record %s(%s) of session %s: %s", tostring(record.logical or "raw"), tostring(record.tupleKey), tostring(record.session), tostring(record.unresolved and record.unresolved.reason))
    end
    if #(r.records or {}) > 0 then logerr("%d unresolved release record(s) kept; they are adopted at the next start and released by  Plugin \"mtpnxk_surface\" \"recover\"", #r.records) end
  elseif not ok then logerr("dispose failed on %s: %s", reason, tostring(r)) end
  rec.instance = nil
  state.inputEnabled = false
end

local function serviceModules(now)
  local released = {}
  local inst = hk()
  if inst then
    local ok, res = pcall(inst.service, inst, now)
    if not ok then
      logerr("hardkeys service() raised: %s; input disabled, module detached (its records are kept)", tostring(res))
      detachHardkeys("service-error", now)
    elseif type(res) == "table" then
      for _, a in ipairs(res.released or {}) do released[a.hold] = true end
      for _, a in ipairs(res.unresolved or {}) do released[a.hold] = true; logerr("deadline release of %s UNRESOLVED: %s", tostring(a.logical or a.tupleKey), tostring(a.error)) end
      for _, sid in ipairs(res.expired or {}) do
        local s = state.sessions[sid]
        if s then s.state = "expired"; log("session %s (%s): lease expired, its holds were released; nothing is re-pressed", sid, s.id) end
      end
    end
  end
  local f = fb()
  if f then
    local ok, res = pcall(f.service, f, now)
    if not ok then logerr("feedback service() raised: %s", tostring(res))
    elseif type(res) == "table" and res.invalidated then log("feedback invalidated: %s (epoch %d); the surface sees every item unknown until the next full state", tostring(res.invalidated), f:epoch()) end
  end
  return released
end

local function tick(now)
  -- 1 + 2: deadlines and feedback before any packet.
  local released = serviceModules(now)
  if next(released) then for _, s in pairs(state.sessions) do pruneReleased(s, released) end end
  -- Forget sessions that stayed silent past forgetMs.
  for sid, s in pairs(state.sessions) do
    if now - s.lastSeen > DEFAULTS.forgetMs / 1000 then closeSession(s, now, "silent") end
  end
  -- 3: bounded packet processing.
  local n = 0
  while n < DEFAULTS.maxPacketsPerTick do
    local ok, data, ip, port = pcall(function() return state.sock:receivefrom() end)
    if not ok or data == nil then break end
    n = n + 1
    handleDatagram(data, ip, port, now)
  end
  state.counters.ticks = state.counters.ticks + 1
  -- 4: state out.
  local cur = collectState(now)
  state.lastState = cur
  sendStates(now)
  serviceBench(now)
  -- Housekeeping (cheap, bounded).
  if now - (state.lastSweep or 0) > 5 then
    state.lastSweep = now
    for nonce, t in pairs(state.nonces) do if now - t > DEFAULTS.nonceMs / 1000 then state.nonces[nonce] = nil end end
    for ip, f in pairs(state.authFail) do if (not f.ignoreUntil or now > f.ignoreUntil) and now - f.since > DEFAULTS.authFailWindowMs / 1000 then state.authFail[ip] = nil end end
    for ip, t in pairs(state.errReplies) do if now - t > 5 then state.errReplies[ip] = nil end end
  end
end
state._tick = tick

-------------------------------------------------------------------------------
-- Start / stop
-------------------------------------------------------------------------------
local function now()
  local ok, t = pcall(socket.gettime)
  if ok and type(t) == "number" then return t end
  return os.clock()
end

local function resetCounters()
  state.counters = { received = 0, sent = 0, sendErrors = 0, rejected = 0, oversized = 0, notAllowed = 0, ignored = 0, noSession = 0,
                     oldSeq = 0, rateDropped = 0, dupEvents = 0, superseded = 0, refused = 0, presses = 0, releases = 0, wheels = 0, lostReleases = 0,
                     replayedHello = 0, addressChanged = 0, fullStates = 0, deltaStates = 0, ticks = 0 }
end

local function loadModules()
  state.modules = {}
  state.moduleVersions = {}
  local summary = {}
  for _, entry in ipairs({ { key = "hardkeys", component = "gma3_mcp_hardkeys" }, { key = "feedback", component = "gma3_mcp_feedback" } }) do
    local rec = { component = entry.component, loaded = false }
    local mod, err = loadModule(entry.component)
    if mod then
      rec.module, rec.version, rec.loaded = mod, mod.VERSION, true
      state.moduleVersions[entry.component] = mod.VERSION
      local okI, inst = pcall(function()
        local deps = mod.consoleDeps(_G)
        if entry.key == "hardkeys" then
          return mod.new({ owner = pluginName, deps = deps, config = { requireInteraction = false, maxHolds = DEFAULTS.maxHolds, maxHoldMs = DEFAULTS.maxHoldMs, defaultLeaseMs = DEFAULTS.leaseMs } }):init()
        end
        return mod.new({ owner = pluginName, deps = deps, config = { maxReadsPerService = DEFAULTS.readsPerService } }):init()
      end)
      if okI then rec.instance = inst else rec.loaded, rec.error = false, "instance: " .. tostring(inst) end
    else rec.error = err end
    state.modules[entry.key] = rec
    summary[#summary + 1] = entry.key .. (rec.loaded and (" " .. tostring(rec.version)) or (" FAILED (" .. tostring(rec.error) .. ")"))
  end
  log("modules: %s", table.concat(summary, ", "))
end
state._loadModules = loadModules

-- Records a previous run could not release are adopted before any input is admitted: their tuples stay
-- reserved (a new press of the same key is refused with "conflict") until "recover" releases them.
-- Records the module rejects (capacity, a tuple already owned) stay in state.unresolved for a later try.
local function adoptKept(t)
  local inst = hk()
  local kept = state.unresolved or {}
  if not inst or #kept == 0 then return end
  local ok, r = pcall(inst.adopt, inst, kept, t)
  if not ok then logerr("adopting %d kept record(s) failed: %s; they are kept", #kept, tostring(r)); return end
  local remaining = {}
  for _, rej in ipairs(r.rejected or {}) do remaining[#remaining + 1] = rej.record; logerr("kept record not adopted (%s); kept for a later start", tostring(rej.reason)) end
  state.unresolved = remaining
  for _, a in ipairs(r.adopted or {}) do log("adopted unresolved record %s(%s): its key stays reserved until \"recover\" releases it", tostring(a.logical or "raw"), tostring(a.tupleKey)) end
end

-- Operator recovery: re-attempt every unresolved release (adopted or current). Without input enabled
-- the keyboard backend is attached for cleanup only; input stays as configured.
local function recoverUnresolved()
  local rec = state.modules.hardkeys
  local inst = rec and rec.instance
  if not inst then logerr("recover: the plugin is not running (start it first; kept records are adopted at start)"); return end
  local t = now()
  adoptKept(t)
  local st = inst:status(t)
  if not st.backend.attached then
    local HK = rec.module
    local a, err = inst:attachBackend(HK.keyboardBackend(HK.consoleDeps(_G), { defaultDisplay = state.display }))
    if not a then logerr("recover: could not attach the keyboard backend for cleanup: %s", tostring(err and err.message)); return end
    log("recover: keyboard backend attached for cleanup only (input stays %s)", tostring(state.inputMode))
  end
  local r = inst:recover(nil, t)
  for _, a in ipairs(r.released or {}) do log("recover: released %s(%s) of session %s", tostring(a.logical or "raw"), tostring(a.tupleKey), tostring(a.session)) end
  for _, a in ipairs(r.unresolved or {}) do logerr("recover: %s(%s) still UNRESOLVED: %s", tostring(a.logical or "raw"), tostring(a.tupleKey), tostring(a.error)) end
  log("recover: %d released, %d still unresolved, %d record(s) not adopted", #(r.released or {}), #(r.unresolved or {}), #(state.unresolved or {}))
end
state._recover = recoverUnresolved

local function bridgeInputEnabled()
  local b = rawget(_G, "__gma3_mcp_bridge")
  return type(b) == "table" and b.running == true and type(b.input) == "table" and b.input.enabled == true
end

local function enableInput(mode, force)
  state.inputMode = mode
  state.inputEnabled = false
  if mode == "off" then log("input off: no console key will be pressed (feedback only)"); return true end
  local inst = hk()
  if not inst then logerr("input %s requested but the hardkeys module is not loaded", mode); return false end
  if bridgeInputEnabled() and not force then
    logerr("the MCP bridge reports input enabled; refusing to enable surface input (two injectors share one console keyboard). Stop the bridge's input or start with 'force'")
    state.inputMode = "off"
    return false
  end
  local HK = state.modules.hardkeys.module
  local adapter
  if mode == "fake" then adapter = HK.fakeBackend()
  else adapter = HK.keyboardBackend(HK.consoleDeps(_G), { defaultDisplay = state.display }) end
  local r, err = inst:enableInput(adapter)
  if not r then logerr("enableInput failed: %s", tostring(err and err.message)); state.inputMode = "off"; return false end
  state.inputEnabled = true
  state.adapter = adapter
  log("input enabled on the %s backend", mode)
  return true
end

local function disposeAll(reason)
  local t = now()
  for _, s in pairs(state.sessions or {}) do closeSession(s, t, reason) end
  detachHardkeys(reason, t)
  local f = fb()
  if f then pcall(f.dispose, f) end
  state.modules = {}
  state.inputEnabled = false
end

local function describe()
  local n = 0
  for _ in pairs(state.sessions or {}) do n = n + 1 end
  return fmt("running=%s bind=%s:%d input=%s sessions=%d gen=%s", tostring(state.running), tostring(state.host), tonumber(state.port) or 0, tostring(state.inputMode), n, tostring(state.gen))
end

local function serverMain()
  local sock, err = socket.udp()
  if not sock then logerr("socket.udp failed: %s", tostring(err)); state.running = false; disposeAll("socket-failed"); return end
  local ok, berr = sock:setsockname(state.host, state.port)
  if not ok then logerr("could not bind %s:%d (%s)", state.host, state.port, tostring(berr)); state.running = false; disposeAll("bind-failed"); return end
  sock:settimeout(0)
  state.sock = sock
  log("listening on %s:%d (v%s, protocol %d, gen %s); %s", state.host, state.port, VERSION, PROTOCOL, state.gen, describe())
  while state.running and not state.stopRequested do
    local okT, terr = xpcall(tick, debug.traceback, now())
    if not okT then logerr("tick failed: %s", tostring(terr)) end
    coroutine.yield()
  end
  disposeAll("shutdown")
  pcall(function() sock:close() end)
  state.sock = nil
  state.running = false
  state.stopRequested = false
  log("stopped")
end

local function start(opts)
  local key = fromHex(opts.key or "")
  if not key or #key ~= 32 then logerr("a 32-byte pairing key is required: key=<64 hex characters>"); return false end
  state.key = key
  state.host = opts.host or DEFAULTS.host
  state.port = opts.port or DEFAULTS.port
  state.allow = opts.allow
  state.display = opts.display or DEFAULTS.display
  state.execs = opts.execs or {}
  state.bench = opts.bench == true
  state.benchWatch = {}
  state.sessions, state.byId, state.nonces, state.authFail, state.errReplies = {}, {}, {}, {}, {}
  state.log = state.log or {}
  state.lastState = nil
  resetCounters()
  seedRandom()
  state.startCount = state.startCount + 1
  state.gen = string.format("%d-%s", state.startCount, randomHex(4))
  state.running = true
  state.stopRequested = false
  loadModules()
  local f = fb()
  if f then
    local items = watchItems()
    local w = f:watch(items, now())
    log("feedback: watching %d item(s)", w.watched)
  end
  state.commandText = function() return CmdObj().cmdtext end
  adoptKept(now())
  enableInput(opts.input, opts.force)
  describeKeys(true)
  return true
end
state._start = start

local function MainImpl(display_handle, argument)
  local opts, perr = parseArgument(argument)
  if not opts then logerr("refusing argument: %s", perr); return end
  if opts.command == "stop" then
    if state.running then state.stopRequested = true; log("stop requested") else log("not running") end
    return
  end
  if opts.command == "recover" then recoverUnresolved(); return end
  if opts.command == "status" then
    log("%s", describe())
    if state.unresolved and #state.unresolved > 0 then log("%d unresolved record(s) kept from a previous run (not adopted yet)", #state.unresolved) end
    local inst = hk()
    if inst then local st = inst:status(now()); if st.unresolved > 0 then log("%d unresolved hold(s) on the instance; run  Plugin \"mtpnxk_surface\" \"recover\"", st.unresolved) end end
    for sid, s in pairs(state.sessions or {}) do
      local held = {}
      for name in pairs(s.holds) do held[#held + 1] = name end
      table.sort(held)
      log("session %s: %s %s:%d state=%s lastSeen=%.1fs ago holds=[%s] unsynced=[%s]", sid, s.id, s.ip, s.port, s.state, now() - s.lastSeen, table.concat(held, ","), table.concat(s.unsyncedHeld or {}, ","))
    end
    if state.counters then
      local parts = {}
      for k, v in pairs(state.counters) do if v ~= 0 then parts[#parts + 1] = k .. "=" .. tostring(v) end end
      table.sort(parts)
      log("counters: %s", table.concat(parts, " "))
    end
    return
  end
  if state.running then log("already running (%s); use \"stop\" first", describe()); return end
  if not start(opts) then state.running = false; return end
  serverMain()
end

local function Main(display_handle, argument)
  local ok, err = xpcall(MainImpl, debug.traceback, display_handle, argument)
  if not ok then logerr("start failed: %s", tostring(err)) end
  -- onPC calls Cleanup after every invocation; a control call ("status") must leave the running loop alone.
  if state.running then state.ignoreNextCleanup = true end
end

local function Cleanup()
  if state.ignoreNextCleanup then state.ignoreNextCleanup = false; return end
  state.stopRequested = true
  if state.sock then pcall(function() state.sock:close() end) end
  state.sock = nil
  disposeAll("cleanup")
  state.running = false
end

-- Exposed for the harness (stock Lua, stubbed socket): the pure functions and the loop pieces.
state._siphash24, state._mac, state._toHex, state._fromHex = siphash24, mac, toHex, fromHex
state._parseArgument, state._NXK_KEYS, state._DEFAULTS, state._VERSION = parseArgument, NXK_KEYS, DEFAULTS, VERSION
state._describe, state._enableInput = describe, enableInput

return Main, Cleanup

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
--   Plugin "mtpnxk_surface" "stop" | "status"
--
-- Rules this plugin keeps (KEYBOARD.md KB-07, docs/surface-protocol.md):
--   * every datagram is authenticated (HMAC-SHA256 with the pairing key) before it is parsed; a sender
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
-- SHA-256 and HMAC (pure Lua 5.3+, integer ops). Verified against FIPS/RFC 4231 vectors in the harness.
-------------------------------------------------------------------------------
local K256 = {
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
}
local function rrot(x, n) return ((x >> n) | (x << (32 - n))) & 0xffffffff end

local function sha256(msg)
  local len = #msg
  msg = msg .. "\128" .. string.rep("\0", (55 - len) % 64) .. string.pack(">I8", len * 8)
  local h0, h1, h2, h3, h4, h5, h6, h7 = 0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19
  local w = {}
  for chunk = 1, #msg, 64 do
    for i = 1, 16 do w[i] = string.unpack(">I4", msg, chunk + (i - 1) * 4) end
    for i = 17, 64 do
      local s0 = rrot(w[i - 15], 7) ~ rrot(w[i - 15], 18) ~ (w[i - 15] >> 3)
      local s1 = rrot(w[i - 2], 17) ~ rrot(w[i - 2], 19) ~ (w[i - 2] >> 10)
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) & 0xffffffff
    end
    local a, b, c, d, e, f, g, h = h0, h1, h2, h3, h4, h5, h6, h7
    for i = 1, 64 do
      local S1 = rrot(e, 6) ~ rrot(e, 11) ~ rrot(e, 25)
      local ch = (e & f) ~ ((~e) & g)
      local t1 = (h + S1 + ch + K256[i] + w[i]) & 0xffffffff
      local S0 = rrot(a, 2) ~ rrot(a, 13) ~ rrot(a, 22)
      local maj = (a & b) ~ (a & c) ~ (b & c)
      local t2 = (S0 + maj) & 0xffffffff
      h, g, f, e, d, c, b, a = g, f, e, (d + t1) & 0xffffffff, c, b, a, (t1 + t2) & 0xffffffff
    end
    h0, h1, h2, h3 = (h0 + a) & 0xffffffff, (h1 + b) & 0xffffffff, (h2 + c) & 0xffffffff, (h3 + d) & 0xffffffff
    h4, h5, h6, h7 = (h4 + e) & 0xffffffff, (h5 + f) & 0xffffffff, (h6 + g) & 0xffffffff, (h7 + h) & 0xffffffff
  end
  return string.pack(">I4I4I4I4I4I4I4I4", h0, h1, h2, h3, h4, h5, h6, h7)
end

local function hmacSha256(key, msg)
  if #key > 64 then key = sha256(key) end
  key = key .. string.rep("\0", 64 - #key)
  local ipad = key:gsub(".", function(c) return string.char(c:byte() ~ 0x36) end)
  local opad = key:gsub(".", function(c) return string.char(c:byte() ~ 0x5c) end)
  return sha256(opad .. sha256(ipad .. msg))
end

local function toHex(s) return (s:gsub(".", function(c) return string.format("%02x", c:byte()) end)) end
local function fromHex(s)
  if type(s) ~= "string" or #s % 2 ~= 0 or s:find("[^%x]") then return nil end
  return (s:gsub("%x%x", function(h) return string.char(tonumber(h, 16)) end))
end

-- The MAC is the first 16 bytes of HMAC-SHA256(key, json bytes) as 32 hex characters.
local function mac(key, text) return toHex(hmacSha256(key, text):sub(1, 16)) end

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
  ["Snap Shot"] = "SNAPSHOT", Thru = "THRU", Full = "FULL", ["@"] = "AT", ["+"] = "PLUS", ["-"] = "MINUS",
  ["."] = "DOT", ["/"] = "SLASH", Back = "BACKSPACE",
}
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
    if l == "stop" or l == "status" then opts.command = l
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
  local given, text = data:match("^MTX1 (%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x) (.*)$")
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
      local okC, r = pcall(inst.describeKey, inst, logical)
      if okC and type(r) == "table" and r.supported then supported = true else reason = tostring(okC and (r and r.reason) or r) end
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
              holds = {}, evSeen = {}, evOrder = {}, rate = { since = now, count = 0 }, state = "active",
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

-- Event-id deduplication: a window of the last evWindow ids per session.
local function seenEvent(s, ev)
  if s.evSeen[ev] then return true end
  s.evSeen[ev] = true
  s.evOrder[#s.evOrder + 1] = ev
  if #s.evOrder > DEFAULTS.evWindow then
    local old = table.remove(s.evOrder, 1)
    s.evSeen[old] = nil
  end
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
  if seenEvent(s, ev) then state.counters.dupEvents = state.counters.dupEvents + 1; ackEvent(s, ev, true, { dup = 1 }); return nil end
  local logical = NXK_KEYS[name]
  if not logical then ackEvent(s, ev, false, { code = "unsupported", why = "not a console key" }); return nil end
  local inst = hk()
  if not inst or not state.inputEnabled then ackEvent(s, ev, false, { code = "input-disabled", why = "input is " .. tostring(state.inputMode) }); return nil end
  if down == 1 then
    local r, err = inst:press(s.sid, now, { key = logical, display = state.display })
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
  if seenEvent(s, ev) then ackEvent(s, ev, true, { dup = 1 }); return nil end
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
local function serviceModules(now)
  local released = {}
  local inst = hk()
  if inst then
    local ok, res = pcall(inst.service, inst, now)
    if not ok then
      logerr("hardkeys service() raised: %s; input disabled, module detached", tostring(res))
      state.inputEnabled = false
      pcall(inst.disableInput, inst, now, "service-error")
      state.modules.hardkeys.instance = nil
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
                     oldSeq = 0, rateDropped = 0, dupEvents = 0, refused = 0, presses = 0, releases = 0, wheels = 0, lostReleases = 0,
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
  local inst = hk()
  if inst then
    local ok, r = pcall(inst.dispose, inst, t)
    if ok and type(r) == "table" and #(r.records or {}) > 0 then
      for _, rec in ipairs(r.records) do logerr("unresolved record kept: %s(%s) session %s: %s", tostring(rec.logical or "raw"), tostring(rec.tupleKey), tostring(rec.session), tostring(rec.unresolved and rec.unresolved.reason)) end
      state.unresolved = r.records
    end
  end
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
  if opts.command == "status" then
    log("%s", describe())
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
state._sha256, state._hmac, state._toHex, state._fromHex = sha256, hmacSha256, toHex, fromHex
state._parseArgument, state._NXK_KEYS, state._DEFAULTS, state._VERSION = parseArgument, NXK_KEYS, DEFAULTS, VERSION
state._describe, state._enableInput = describe, enableInput

return Main, Cleanup

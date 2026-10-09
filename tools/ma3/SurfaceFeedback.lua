-- SurfaceFeedback: sends grandMA3 console state to an mtpnxk surface as OSC,
-- for its LEDs. Schema and rationale: docs/ma3-feedback.md.
--
-- Skeleton, not yet run on a console. API calls marked "unconfirmed" are
-- wrapped in pcall so a wrong guess costs a missing message, not the loop.
--
-- Command line:
--   Plugin "SurfaceFeedback"            start (a second start replaces the first)
--   Plugin "SurfaceFeedback" "stop"     stop
--   Plugin "SurfaceFeedback" "probe"    dump candidate state objects to the System Monitor
--
-- Setup: Menu > In & Out > OSC, a line with the Pico's IP and port, Mode UDP,
-- Send Command = Yes, empty Prefix; Enable Output on. CONFIG.osc_line is
-- that line's number.

local CONFIG = {
    osc_line = 1,
    poll = 0.1,      -- seconds between reads
    refresh = 2.0,   -- seconds between full-state sends (and keepalives)
    -- Executors to report on the current page. The NX-K needs none; the
    -- M-Touch would list its ten strips, e.g. {201, 202, ..., 210}.
    execs = {},
}

local VERSION = "0.1"
local RUN_VAR = "SurfaceFeedbackRun"

-- Keywords reported in /sfb/pending. Lower-case, first word of the command line.
local PENDING = {
    store = true, update = true, edit = true, copy = true, move = true,
    delete = true, load = true, cue = true, group = true, macro = true,
    fade = true, delay = true, assign = true, label = true,
}

local function send(addr, value)
    local types = type(value) == "string" and "s" or "i"
    Cmd(string.format('SendOSC %d "%s,%s,%s"', CONFIG.osc_line, addr, types, tostring(value)))
end

local function try(fn)
    local ok, value = pcall(fn)
    if ok then
        return value
    end
    return nil
end

-- Programmer modes. No documented Lua getter exists for these (2.5 manual);
-- fill in from "probe" output. Returning nil means "unknown, send nothing".
local MODES = {
    blind = function() return nil end,
    highlight = function() return nil end,
    preview = function() return nil end,
    freeze = function() return nil end,
    solo = function() return nil end,
}

local function read_pending()
    -- unconfirmed: the command-line object's text property name.
    local text = try(function() return CmdObj().cmdtext end)
    if type(text) ~= "string" then
        return nil
    end
    local word = string.lower(string.match(text, "^%s*(%a+)") or "")
    if PENDING[word] then
        return word
    end
    return "none"
end

local function collect()
    local state = {}

    for name, getter in pairs(MODES) do
        local on = try(getter)
        if on ~= nil then
            state["/sfb/mode/" .. name] = on and 1 or 0
        end
    end

    state["/sfb/pending"] = read_pending()

    -- unconfirmed: the page handle's number property.
    state["/sfb/page"] = try(function() return CurrentExecPage().no end)

    for _, n in ipairs(CONFIG.execs) do
        local exec = try(function() return GetExecutor(n) end)
        if exec ~= nil then
            local running = try(function() return exec.Object:HasActivePlayback() end)
            if running ~= nil then
                state["/sfb/exec/" .. n] = running and 1 or 0
            end
            local level = try(function() return exec:GetFader({}) end)
            if type(level) == "number" then
                state["/sfb/fader/" .. n] = math.floor(level + 0.5)
            end
        end
    end

    return state
end

local function run(token)
    local refresh_ticks = math.max(1, math.floor(CONFIG.refresh / CONFIG.poll + 0.5))
    local last = {}
    local tick = 0
    local seq = 0

    send("/sfb/hello", VERSION)
    while GetVar(GlobalVars(), RUN_VAR) == token do
        local full = tick % refresh_ticks == 0
        local state = collect()
        for addr, value in pairs(state) do
            if full or last[addr] ~= value then
                send(addr, value)
            end
        end
        if full then
            seq = seq + 1
            send("/sfb/alive", seq)
        end
        last = state
        tick = tick + 1
        coroutine.yield(CONFIG.poll)
    end
end

local function probe()
    Printf("SurfaceFeedback probe: CmdObj")
    try(function() CmdObj():Dump() end)
    Printf("SurfaceFeedback probe: Programmer")
    try(function() Programmer():Dump() end)
    Printf("SurfaceFeedback probe: CurrentExecPage")
    try(function() CurrentExecPage():Dump() end)
    Printf("SurfaceFeedback probe: collect()")
    for addr, value in pairs(collect()) do
        Printf("  %s = %s", addr, tostring(value))
    end
end

local function Main(display, argument)
    local arg = string.lower(argument or "")
    if arg == "stop" then
        SetVar(GlobalVars(), RUN_VAR, "")
        Printf("SurfaceFeedback stopped")
        return
    end
    if arg == "probe" then
        probe()
        return
    end
    -- A fresh token makes any loop already running exit on its next tick.
    local token = tostring(math.random(1, 1000000000))
    SetVar(GlobalVars(), RUN_VAR, token)
    Printf("SurfaceFeedback %s running on OSC line %d", VERSION, CONFIG.osc_line)
    run(token)
end

return Main

//! Martin / Obsidian M-Touch (PID 0xF808) and M-Play (PID 0xF80C): report decoder, control
//! tables and output encoders, ported from the MTouchPlay repository at revision e48eb2c
//! (`docs/protocol.md`, `docs/mplay-notes.md`, `tools/mtouch-cli/src/main.rs`). Everything here
//! is a port of documented evidence; neither device has been connected to this service. The
//! regression tests at the bottom name the capture line or document section each one reproduces.
//! Record of what was reused and what still needs hardware: `docs/mtouch-protocol-reuse.md`.
//!
//! The two devices share the NX-K's transport (VID 0x11be, interface 0 alternate 1, interrupt IN
//! 0x82, vendor request 0x80 for LED keys) and add two vendor requests: 0x61 for fader LED bars
//! and 0x54 for the page display.

// Until KB-20+ wires these devices into the link, parts of the encoder API (the LED builder's
// slow/force-off lanes, the named constants, `Event::id`) are reached only by the regression
// tests and the operator tools; the crate is a binary, so they would otherwise warn.
#![allow(dead_code)]

pub mod tools;
pub mod usb;

pub const VID: u16 = 0x11be;
pub const PID_MTOUCH: u16 = 0xF808;
pub const PID_MPLAY: u16 = 0xF80C;
/// Interface 0, alternate setting 1: the same as the NX-K (`nxk::INTERFACE`, `nxk::ALT_SETTING`),
/// selected by `nxk::usb::claim_vendor_interface`.
pub const ENDPOINT_IN: u8 = 0x82;
pub const PACKET_MAX: usize = 64;

/// bRequest codes (protocol.md section 4). Vendor requests 0xA0..0xAF are the EZ-USB firmware
/// load range and must never be sent (section 1).
pub const REQ_LED_KEY: u8 = 0x80;
pub const REQ_FADER_BAR: u8 = 0x61;
pub const REQ_PAGE_DISPLAY: u8 = 0x54;

/// Page displays: 0x4401 on the M-Touch (section 3.4); the M-Play has two (mplay-notes.md).
pub const DISPLAY_MTOUCH: u16 = 0x4401;
pub const DISPLAY_MPLAY_LEFT: u16 = 0x9F01;
pub const DISPLAY_MPLAY_RIGHT: u16 = 0x9E01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    MTouch,
    MPlay,
}

impl Model {
    pub fn from_pid(pid: u16) -> Option<Model> {
        match pid {
            PID_MTOUCH => Some(Model::MTouch),
            PID_MPLAY => Some(Model::MPlay),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Model::MTouch => "M-Touch",
            Model::MPlay => "M-Play",
        }
    }

    pub fn pid(self) -> u16 {
        match self {
            Model::MTouch => PID_MTOUCH,
            Model::MPlay => PID_MPLAY,
        }
    }

    /// Unit count the device writes into byte 1 of its bank reports (10 or 12). Informational:
    /// the decoder always takes N from the packet.
    pub fn bank_units(self) -> u8 {
        match self {
            Model::MTouch => 0x0A,
            Model::MPlay => 0x0C,
        }
    }

    pub fn displays(self) -> &'static [u16] {
        match self {
            Model::MTouch => &[DISPLAY_MTOUCH],
            Model::MPlay => &[DISPLAY_MPLAY_LEFT, DISPLAY_MPLAY_RIGHT],
        }
    }
}

/// One control from the Onyx console XML (`tools/mtouch-controls.tsv`). `kind`:
/// K = LED key (single colour), V = pressure-sensing red/green key, L = indicator LED,
/// F = fader with a 10-LED bar, D = 7-segment display, X = key without LED, B = backlight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Control {
    pub id: u16,
    pub kind: char,
    pub name: &'static str,
}

const fn c(id: u16, kind: char, name: &'static str) -> Control {
    Control { id, kind, name }
}

/// M-Touch (Onyx layout `WingTouchSurface`). Strip n (1..10) is based at 0x4200 + 0x10*(n-1):
/// +1 upper capacitive button (blue LED, key bank), +2 lower capacitive button (blue, key bank),
/// +3 touch fader (RGB bar, analog bank), +5 bottom pressure button (red+green, analog bank).
/// Base-channel fader n (1..4) is 0x6100 + 0x10*n: +1 capacitive button (blue), +2 fader with a
/// blue-only bar. protocol.md sections 3.1 to 3.4.
pub const CONTROLS_MTOUCH: &[Control] = &[
    c(0x4111, 'X', "PLAYBACK BANK Up (+)"),
    c(0x4113, 'X', "PLAYBACK BANK Down (-)"),
    c(0x4201, 'K', "PLAYBACK PFA 1"), c(0x4202, 'K', "PLAYBACK PFB 1"), c(0x4203, 'F', "LEVEL PLAYBACK 1"), c(0x4205, 'V', "PLAYBACK PFD 1"),
    c(0x4211, 'K', "PLAYBACK PFA 2"), c(0x4212, 'K', "PLAYBACK PFB 2"), c(0x4213, 'F', "LEVEL PLAYBACK 2"), c(0x4215, 'V', "PLAYBACK PFD 2"),
    c(0x4221, 'K', "PLAYBACK PFA 3"), c(0x4222, 'K', "PLAYBACK PFB 3"), c(0x4223, 'F', "LEVEL PLAYBACK 3"), c(0x4225, 'V', "PLAYBACK PFD 3"),
    c(0x4231, 'K', "PLAYBACK PFA 4"), c(0x4232, 'K', "PLAYBACK PFB 4"), c(0x4233, 'F', "LEVEL PLAYBACK 4"), c(0x4235, 'V', "PLAYBACK PFD 4"),
    c(0x4241, 'K', "PLAYBACK PFA 5"), c(0x4242, 'K', "PLAYBACK PFB 5"), c(0x4243, 'F', "LEVEL PLAYBACK 5"), c(0x4245, 'V', "PLAYBACK PFD 5"),
    c(0x4251, 'K', "PLAYBACK PFA 6"), c(0x4252, 'K', "PLAYBACK PFB 6"), c(0x4253, 'F', "LEVEL PLAYBACK 6"), c(0x4255, 'V', "PLAYBACK PFD 6"),
    c(0x4261, 'K', "PLAYBACK PFA 7"), c(0x4262, 'K', "PLAYBACK PFB 7"), c(0x4263, 'F', "LEVEL PLAYBACK 7"), c(0x4265, 'V', "PLAYBACK PFD 7"),
    c(0x4271, 'K', "PLAYBACK PFA 8"), c(0x4272, 'K', "PLAYBACK PFB 8"), c(0x4273, 'F', "LEVEL PLAYBACK 8"), c(0x4275, 'V', "PLAYBACK PFD 8"),
    c(0x4281, 'K', "PLAYBACK PFA 9"), c(0x4282, 'K', "PLAYBACK PFB 9"), c(0x4283, 'F', "LEVEL PLAYBACK 9"), c(0x4285, 'V', "PLAYBACK PFD 9"),
    c(0x4291, 'K', "PLAYBACK PFA 10"), c(0x4292, 'K', "PLAYBACK PFB 10"), c(0x4293, 'F', "LEVEL PLAYBACK 10"), c(0x4295, 'V', "PLAYBACK PFD 10"),
    c(0x4401, 'D', "PLAYBACK BANK Number (page display)"),
    c(0x5101, 'K', "COMMAND EDIT"), c(0x5103, 'K', "COMMAND CLEAR"),
    c(0x5401, 'K', "COMMAND RECORD"), c(0x5402, 'K', "COMMAND UPDATE"), c(0x5411, 'K', "COMMAND LOAD"),
    c(0x5502, 'K', "SELECT"), c(0x5503, 'K', "RELEASE (Rel)"), c(0x5504, 'K', "BEAT"),
    c(0x5511, 'K', "SNAP"), c(0x5512, 'K', "Pause (||/Back)"), c(0x5513, 'K', "Next (Go)"),
    c(0x5801, 'V', "MF 1"), c(0x5802, 'V', "MF 2"), c(0x5803, 'V', "MF 3"), c(0x5804, 'V', "MF 4"), c(0x5805, 'V', "MF 5"),
    c(0x5806, 'V', "MF 6"), c(0x5807, 'V', "MF 7"), c(0x5808, 'V', "MF 8"), c(0x5809, 'V', "MF 9"), c(0x580A, 'V', "MF 10"),
    c(0x5812, 'L', "MF mode LED Playback"), c(0x5813, 'L', "MF mode LED F-key"), c(0x5814, 'L', "MF mode LED Base"), c(0x5815, 'L', "MF mode LED Effect"),
    c(0x6001, 'K', "HIGHLIGHT"),
    c(0x6107, 'X', "BASE CHANNEL GROUP Mode (Play/Base/F-Key/FX)"),
    c(0x6111, 'K', "BASE CHANNEL 1"), c(0x6112, 'F', "VALUE BASE CHANNEL 1"),
    c(0x6121, 'K', "BASE CHANNEL 2"), c(0x6122, 'F', "VALUE BASE CHANNEL 2"),
    c(0x6131, 'K', "BASE CHANNEL 3"), c(0x6132, 'F', "VALUE BASE CHANNEL 3"),
    c(0x6141, 'K', "BASE CHANNEL 4"), c(0x6142, 'F', "VALUE BASE CHANNEL 4"),
    c(0x6401, 'K', "BROWSE PREVIOUS (Last)"), c(0x6402, 'K', "BROWSE NEXT"),
    c(0x7110, 'B', "BackLight"),
];

/// M-Play (Onyx layout `PlayTouchSurface`). Strip n (1..12): fader 0x9203, top button 0x9205,
/// bottom button 0x92C5, each + 0x10*(n-1); right block 0x9805 (rows 1-3) and 0x98C5 (rows 4-6),
/// + 0x10*(n-1); two page displays 0x9F01 (left) and 0x9E01 (right). mplay-notes.md.
pub const CONTROLS_MPLAY: &[Control] = &[
    c(0x5502, 'K', "SELECT"), c(0x5503, 'K', "RELEASE (Rel)"), c(0x5504, 'K', "BEAT"),
    c(0x5511, 'K', "SNAP"), c(0x5512, 'K', "Pause (||/Back)"), c(0x5513, 'K', "Next (Go)"),
    c(0x7110, 'B', "BackLight"),
    c(0x9203, 'F', "FADER LEVEL 1"), c(0x9213, 'F', "FADER LEVEL 2"), c(0x9223, 'F', "FADER LEVEL 3"), c(0x9233, 'F', "FADER LEVEL 4"),
    c(0x9243, 'F', "FADER LEVEL 5"), c(0x9253, 'F', "FADER LEVEL 6"), c(0x9263, 'F', "FADER LEVEL 7"), c(0x9273, 'F', "FADER LEVEL 8"),
    c(0x9283, 'F', "FADER LEVEL 9"), c(0x9293, 'F', "FADER LEVEL 10"), c(0x92A3, 'F', "FADER LEVEL 11"), c(0x92B3, 'F', "FADER LEVEL 12"),
    c(0x9205, 'V', "FADER PFD 1"), c(0x9215, 'V', "FADER PFD 2"), c(0x9225, 'V', "FADER PFD 3"), c(0x9235, 'V', "FADER PFD 4"),
    c(0x9245, 'V', "FADER PFD 5"), c(0x9255, 'V', "FADER PFD 6"), c(0x9265, 'V', "FADER PFD 7"), c(0x9275, 'V', "FADER PFD 8"),
    c(0x9285, 'V', "FADER PFD 9"), c(0x9295, 'V', "FADER PFD 10"), c(0x92A5, 'V', "FADER PFD 11"), c(0x92B5, 'V', "FADER PFD 12"),
    c(0x92C5, 'V', "FADER PFD 13"), c(0x92D5, 'V', "FADER PFD 14"), c(0x92E5, 'V', "FADER PFD 15"), c(0x92F5, 'V', "FADER PFD 16"),
    c(0x9305, 'V', "FADER PFD 17"), c(0x9315, 'V', "FADER PFD 18"), c(0x9325, 'V', "FADER PFD 19"), c(0x9335, 'V', "FADER PFD 20"),
    c(0x9345, 'V', "FADER PFD 21"), c(0x9355, 'V', "FADER PFD 22"), c(0x9365, 'V', "FADER PFD 23"), c(0x9375, 'V', "FADER PFD 24"),
    c(0x9805, 'V', "FADER EXT PF 1"), c(0x9815, 'V', "FADER EXT PF 2"), c(0x9825, 'V', "FADER EXT PF 3"), c(0x9835, 'V', "FADER EXT PF 4"),
    c(0x9845, 'V', "FADER EXT PF 5"), c(0x9855, 'V', "FADER EXT PF 6"), c(0x9865, 'V', "FADER EXT PF 7"), c(0x9875, 'V', "FADER EXT PF 8"),
    c(0x9885, 'V', "FADER EXT PF 9"), c(0x9895, 'V', "FADER EXT PF 10"), c(0x98A5, 'V', "FADER EXT PF 11"), c(0x98B5, 'V', "FADER EXT PF 12"),
    c(0x98C5, 'V', "FADER EXT PF 13"), c(0x98D5, 'V', "FADER EXT PF 14"), c(0x98E5, 'V', "FADER EXT PF 15"), c(0x98F5, 'V', "FADER EXT PF 16"),
    c(0x9905, 'V', "FADER EXT PF 17"), c(0x9915, 'V', "FADER EXT PF 18"), c(0x9925, 'V', "FADER EXT PF 19"), c(0x9935, 'V', "FADER EXT PF 20"),
    c(0x9945, 'V', "FADER EXT PF 21"), c(0x9955, 'V', "FADER EXT PF 22"), c(0x9965, 'V', "FADER EXT PF 23"), c(0x9975, 'V', "FADER EXT PF 24"),
    c(0x9E01, 'D', "FADER EXT BANK Number (display)"), c(0x9E12, 'X', "FADER EXT BANK PageUp"), c(0x9E13, 'X', "FADER EXT BANK PageDown"),
    c(0x9F01, 'D', "FADER BANK Number (display)"), c(0x9F12, 'X', "FADER BANK PageUp"), c(0x9F13, 'X', "FADER BANK PageDown"),
];

pub fn controls(model: Model) -> &'static [Control] {
    match model {
        Model::MTouch => CONTROLS_MTOUCH,
        Model::MPlay => CONTROLS_MPLAY,
    }
}

pub fn control(model: Model, id: u16) -> Option<&'static Control> {
    controls(model).iter().find(|c| c.id == id)
}

pub fn name_of(model: Model, id: u16) -> Option<&'static str> {
    control(model, id).map(|c| c.name)
}

pub fn kind_of(model: Model, id: u16) -> Option<char> {
    control(model, id).map(|c| c.kind)
}

// ---------------------------------------------------------------------------------------------
// Reports (device -> host, protocol.md section 5)

/// One unit of an analog bank: `changed` marks the unit(s) this report is about; the others
/// carry their current value and touch flag (a finger still resting on a second fader).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub id: u16,
    pub value: u8,
    pub touch: bool,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    /// `01 02 lo hi state` (5.1).
    Key { id: u16, down: bool },
    /// `02 02 lo hi 00 value touch` (5.2): the base-channel faders. `touch` false is the lift
    /// report with the resting value.
    Fader { id: u16, value: u8, touch: bool },
    /// `C1 N lo hi stride changed[N] state[N]` (5.3). `changed` lists the units flagged in this
    /// report with their new state; `states` is every unit's state.
    KeyBank { base: u16, stride: u16, changed: Vec<(u16, bool)>, states: Vec<bool> },
    /// `C2 N lo hi stride changed[N] value[N] touch[N]` (5.4).
    AnalogBank { base: u16, stride: u16, units: Vec<Unit> },
    /// Bytes that fit no report shape.
    Unknown { bytes: Vec<u8> },
}

impl Report {
    pub fn kind(&self) -> &'static str {
        match self {
            Report::Key { .. } => "key",
            Report::Fader { .. } => "fader",
            Report::KeyBank { .. } => "key-bank",
            Report::AnalogBank { .. } => "analog-bank",
            Report::Unknown { .. } => "unknown",
        }
    }
}

/// Length of the report starting at `d[0]`, if its header is a known shape and the length is
/// available. Bank unit counts come from byte 1 (0x0A on the M-Touch, 0x0C on the M-Play).
fn report_len(d: &[u8]) -> Option<usize> {
    match d {
        [0x01, 0x02, ..] => Some(5),
        [0x02, 0x02, ..] => Some(7),
        [0xC1, n, ..] => Some(5 + 2 * *n as usize),
        [0xC2, n, ..] => Some(5 + 3 * *n as usize),
        _ => None,
    }
}

fn parse_one(d: &[u8]) -> Report {
    let id = |lo: u8, hi: u8| u16::from_le_bytes([lo, hi]);
    match d {
        [0x01, 0x02, lo, hi, state] => Report::Key { id: id(*lo, *hi), down: *state != 0 },
        [0x02, 0x02, lo, hi, _, value, touch] => Report::Fader { id: id(*lo, *hi), value: *value, touch: *touch != 0 },
        [0xC1, n, lo, hi, stride, rest @ ..] => {
            let n = *n as usize;
            let (base, stride) = (id(*lo, *hi), *stride as u16);
            let (flags, states) = (&rest[..n], &rest[n..2 * n]);
            let changed = (0..n).filter(|&i| flags[i] != 0).map(|i| (base.wrapping_add(stride * i as u16), states[i] != 0)).collect();
            Report::KeyBank { base, stride, changed, states: states.iter().map(|s| *s != 0).collect() }
        }
        [0xC2, n, lo, hi, stride, rest @ ..] => {
            let n = *n as usize;
            let (base, stride) = (id(*lo, *hi), *stride as u16);
            let (flags, values, touch) = (&rest[..n], &rest[n..2 * n], &rest[2 * n..3 * n]);
            let units = (0..n)
                .map(|i| Unit { id: base.wrapping_add(stride * i as u16), value: values[i], touch: touch[i] != 0, changed: flags[i] != 0 })
                .collect();
            Report::AnalogBank { base, stride, units }
        }
        _ => Report::Unknown { bytes: d.to_vec() },
    }
}

/// Decodes one interrupt IN packet into the reports it carries. An empty packet (the idle poll
/// answer) yields nothing. Reports are consumed in sequence by their first byte and length, so
/// two short reports sharing one packet (protocol.md 5, cap1 frames 1716, 1730, 1756) come out
/// as two; whatever remains that fits no shape is returned as one `Unknown`.
pub fn decode(data: &[u8]) -> Vec<Report> {
    let mut out = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        match report_len(rest) {
            Some(len) if len <= rest.len() => {
                out.push(parse_one(&rest[..len]));
                rest = &rest[len..];
            }
            _ => {
                out.push(Report::Unknown { bytes: rest.to_vec() });
                break;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Events: one per control change, with the model's names

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    KeyDown { id: u16, name: &'static str },
    KeyUp { id: u16, name: &'static str },
    /// A fader position 0..255 (0xFF = top); `touch` false is the lift report.
    Fader { id: u16, name: &'static str, value: u8, touch: bool },
    /// A pressure-sensing key (kind V): pressure 0..255; `touch` false is the release.
    Pressure { id: u16, name: &'static str, value: u8, touch: bool },
}

impl Event {
    pub fn id(&self) -> u16 {
        match self {
            Event::KeyDown { id, .. } | Event::KeyUp { id, .. } | Event::Fader { id, .. } | Event::Pressure { id, .. } => *id,
        }
    }
}

/// Events for one report: one per changed unit of a bank, one for a single key or fader.
/// Analog-bank units resolve to `Pressure` when the model's table says kind V, otherwise `Fader`
/// (an id missing from the table is reported as a fader, named "?").
pub fn events(model: Model, report: &Report) -> Vec<Event> {
    let name = |id: u16| name_of(model, id).unwrap_or("?");
    match report {
        Report::Key { id, down } => vec![if *down { Event::KeyDown { id: *id, name: name(*id) } } else { Event::KeyUp { id: *id, name: name(*id) } }],
        Report::Fader { id, value, touch } => vec![Event::Fader { id: *id, name: name(*id), value: *value, touch: *touch }],
        Report::KeyBank { changed, .. } => changed
            .iter()
            .map(|(id, down)| if *down { Event::KeyDown { id: *id, name: name(*id) } } else { Event::KeyUp { id: *id, name: name(*id) } })
            .collect(),
        Report::AnalogBank { units, .. } => units
            .iter()
            .filter(|u| u.changed)
            .map(|u| {
                if kind_of(model, u.id) == Some('V') {
                    Event::Pressure { id: u.id, name: name(u.id), value: u.value, touch: u.touch }
                } else {
                    Event::Fader { id: u.id, name: name(u.id), value: u.value, touch: u.touch }
                }
            })
            .collect(),
        Report::Unknown { .. } => Vec::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// Output encoders (host -> device, protocol.md section 4)

/// LED key state for `bRequest 0x80` (section 4.1). Each colour has an on bit (0 green, 1 red,
/// 8 blue), a fast-blink bit (2, 3, 10), a slow-blink bit (4, 6, 12) and a force-off bit
/// (5, 7, 13). `fast`, `slow` and `force_off` apply to the colours selected by the on bits, as
/// mtouch-cli's `led_value` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LedKey {
    pub green: bool,
    pub red: bool,
    pub blue: bool,
    pub fast: bool,
    pub slow: bool,
    pub force_off: bool,
}

impl LedKey {
    pub const fn off() -> LedKey {
        LedKey { green: false, red: false, blue: false, fast: false, slow: false, force_off: false }
    }
    pub const fn green(self) -> LedKey {
        LedKey { green: true, ..self }
    }
    pub const fn red(self) -> LedKey {
        LedKey { red: true, ..self }
    }
    pub const fn blue(self) -> LedKey {
        LedKey { blue: true, ..self }
    }
    pub const fn yellow(self) -> LedKey {
        LedKey { green: true, red: true, ..self }
    }
    pub const fn fast(self) -> LedKey {
        LedKey { fast: true, ..self }
    }
    pub const fn slow(self) -> LedKey {
        LedKey { slow: true, ..self }
    }
    pub const fn force_off(self) -> LedKey {
        LedKey { force_off: true, ..self }
    }
    pub const fn value(self) -> u16 {
        led_key(self)
    }
}

pub const LED_OFF: u16 = 0x0000;
pub const LED_GREEN: u16 = 0x0001;
pub const LED_RED: u16 = 0x0002;
pub const LED_YELLOW: u16 = 0x0003;
pub const LED_BLUE: u16 = 0x0100;

/// The wValue of `bRequest 0x80` for a key state (section 4.1).
pub const fn led_key(s: LedKey) -> u16 {
    let mut v = 0u16;
    // (on, fast, slow, force off) bit numbers per colour lane.
    let lanes: [(bool, u16, u16, u16, u16); 3] = [(s.green, 0, 2, 4, 5), (s.red, 1, 3, 6, 7), (s.blue, 8, 10, 12, 13)];
    let mut i = 0;
    while i < 3 {
        let (on, on_bit, fast_bit, slow_bit, off_bit) = lanes[i];
        if on {
            v |= 1 << on_bit;
            if s.fast {
                v |= 1 << fast_bit;
            }
            if s.slow {
                v |= 1 << slow_bit;
            }
            if s.force_off {
                v |= 1 << off_bit;
            }
        }
        i += 1;
    }
    v
}

/// Bar colours as mtouch-cli names them; mixes are per LED (section 4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarColour {
    Off,
    Green,
    Red,
    Blue,
    Yellow,
    Cyan,
    Magenta,
    White,
}

impl BarColour {
    /// (green, red, blue) lanes.
    pub const fn lanes(self) -> (bool, bool, bool) {
        match self {
            BarColour::Off => (false, false, false),
            BarColour::Green => (true, false, false),
            BarColour::Red => (false, true, false),
            BarColour::Blue => (false, false, true),
            BarColour::Yellow => (true, true, false),
            BarColour::Cyan => (true, false, true),
            BarColour::Magenta => (false, true, true),
            BarColour::White => (true, true, true),
        }
    }
}

pub const BAR_LEDS: u8 = 10;
pub const BAR_BLINK: u32 = 1 << 23;

/// The 9-byte payload of `bRequest 0x61` (section 4.2): three little-endian u24 words in the
/// order green, red, blue. Bits 0..9 of each word light that colour's LEDs from the bottom; the
/// matching `blink` flag sets bit 23 so the lit LEDs of that colour blink. Bits above 9 of the
/// masks are dropped. Base-channel bars (0x61n2) show only the blue word.
pub fn fader_bar(green: u16, red: u16, blue: u16, blink: (bool, bool, bool)) -> [u8; 9] {
    let word = |mask: u16, blink: bool| -> u32 { (mask as u32 & 0x3FF) | if blink { BAR_BLINK } else { 0 } };
    let mut out = [0u8; 9];
    for (i, w) in [word(green, blink.0), word(red, blink.1), word(blue, blink.2)].iter().enumerate() {
        out[i * 3..i * 3 + 3].copy_from_slice(&w.to_le_bytes()[..3]);
    }
    out
}

/// `n` LEDs (0..=10) lit from the bottom in `colour`, blinking or not: mtouch-cli `bar_payload`.
pub fn bar_level(n: u8, colour: BarColour, blink: bool) -> [u8; 9] {
    let n = n.min(BAR_LEDS);
    let mask = ((1u32 << n) - 1) as u16;
    let (g, r, b) = colour.lanes();
    let lane = |on: bool| if on { mask } else { 0 };
    fader_bar(lane(g), lane(r), lane(b), (g && blink, r && blink, b && blink))
}

/// 7-segment codes for 0..9, bit 0 = a (top) .. bit 6 = g (middle) (section 4.3).
pub const SEG: [u8; 10] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];

/// The 6-byte payload of `bRequest 0x54` (section 4.3): bytes 2, 3, 4 are the right, middle and
/// left digits; leading zeros are blank; `None` blanks the display. Bytes 0, 1 and 5 had no
/// visible effect in the survey (Onyx writes 0x17 into byte 1); we write zero.
/// mtouch-cli `page_payload`.
pub fn page_display(n: Option<u16>) -> [u8; 6] {
    let mut out = [0u8; 6];
    if let Some(n) = n {
        let n = n.min(999);
        out[2] = SEG[(n % 10) as usize];
        if n >= 10 {
            out[3] = SEG[((n / 10) % 10) as usize];
        }
        if n >= 100 {
            out[4] = SEG[((n / 100) % 10) as usize];
        }
    }
    out
}

/// One vendor control OUT request (`bmRequestType 0x40`, wIndex = control id, wValue as given).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    LedKey { id: u16, value: u16 },
    FaderBar { id: u16, data: [u8; 9] },
    PageDisplay { id: u16, data: [u8; 6] },
}

impl Output {
    pub fn request(&self) -> u8 {
        match self {
            Output::LedKey { .. } => REQ_LED_KEY,
            Output::FaderBar { .. } => REQ_FADER_BAR,
            Output::PageDisplay { .. } => REQ_PAGE_DISPLAY,
        }
    }
    pub fn index(&self) -> u16 {
        match self {
            Output::LedKey { id, .. } | Output::FaderBar { id, .. } | Output::PageDisplay { id, .. } => *id,
        }
    }
    pub fn value(&self) -> u16 {
        match self {
            Output::LedKey { value, .. } => *value,
            _ => 0,
        }
    }
    pub fn data(&self) -> &[u8] {
        match self {
            Output::LedKey { .. } => &[],
            Output::FaderBar { data, .. } => data,
            Output::PageDisplay { data, .. } => data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Model::{MPlay, MTouch};

    fn hexv(s: &str) -> Vec<u8> {
        hex::decode(s).unwrap()
    }

    /// The single event of a packet that must decode to exactly one report.
    fn one(model: Model, s: &str) -> Vec<Event> {
        let reports = decode(&hexv(s));
        assert_eq!(reports.len(), 1, "{s}: {reports:?}");
        events(model, &reports[0])
    }

    // --- single key (protocol.md 5.1; listen-session-b.log lines 3, 4, 1229; mplay log line 3)

    #[test]
    fn single_key_press_and_release() {
        assert_eq!(decode(&hexv("0102114101")), vec![Report::Key { id: 0x4111, down: true }]);
        assert_eq!(one(MTouch, "0102114101"), vec![Event::KeyDown { id: 0x4111, name: "PLAYBACK BANK Up (+)" }]);
        assert_eq!(one(MTouch, "0102114100"), vec![Event::KeyUp { id: 0x4111, name: "PLAYBACK BANK Up (+)" }]);
        assert_eq!(one(MTouch, "0102116101"), vec![Event::KeyDown { id: 0x6111, name: "BASE CHANNEL 1" }]);
        assert_eq!(one(MPlay, "0102139f01"), vec![Event::KeyDown { id: 0x9F13, name: "FADER BANK PageDown" }]);
    }

    // --- key bank (protocol.md 5.3; listen-session-b.log line 175)

    #[test]
    fn key_bank_pfa1_pressed() {
        let r = decode(&hexv("c10a0142100200000000000000000001000000000000000000"));
        let mut states = vec![false; 10];
        states[0] = true;
        assert_eq!(r, vec![Report::KeyBank { base: 0x4201, stride: 0x10, changed: vec![(0x4201, true)], states }]);
        assert_eq!(events(MTouch, &r[0]), vec![Event::KeyDown { id: 0x4201, name: "PLAYBACK PFA 1" }]);
        // line 176: the release; line 177: PFB 1 (base 0x4202)
        assert_eq!(one(MTouch, "c10a0142100200000000000000000000000000000000000000"), vec![Event::KeyUp { id: 0x4201, name: "PLAYBACK PFA 1" }]);
        assert_eq!(one(MTouch, "c10a0242100200000000000000000001000000000000000000"), vec![Event::KeyDown { id: 0x4202, name: "PLAYBACK PFB 1" }]);
    }

    // --- analog bank, M-Touch (protocol.md 5.4; listen-session-b.log line 37)

    #[test]
    fn analog_bank_mf1_pressure() {
        let r = decode(&hexv("c20a01580102000000000000000000b000000000000000000001000000000000000000"));
        match &r[0] {
            Report::AnalogBank { base, stride, units } => {
                assert_eq!((*base, *stride, units.len()), (0x5801, 0x01, 10));
                assert_eq!(units[0], Unit { id: 0x5801, value: 0xb0, touch: true, changed: true });
                assert_eq!(units[9], Unit { id: 0x580A, value: 0, touch: false, changed: false });
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(events(MTouch, &r[0]), vec![Event::Pressure { id: 0x5801, name: "MF 1", value: 0xb0, touch: true }]);
    }

    /// Constructed for base 0x4203 (touch fader 1): a touch at value 0x80, then the lift report
    /// (touch 0 with the resting value), as protocol.md 5.4 describes and line 477 shows.
    #[test]
    fn analog_bank_fader_touch_then_lift() {
        let mut touch = vec![0xC2, 0x0A, 0x03, 0x42, 0x10];
        touch.extend_from_slice(&[0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        touch.extend_from_slice(&[0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        touch.extend_from_slice(&[0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(touch.len(), 35);
        let mut lift = touch.clone();
        lift[25] = 0;
        let r = decode(&touch);
        assert_eq!(events(MTouch, &r[0]), vec![Event::Fader { id: 0x4203, name: "LEVEL PLAYBACK 1", value: 0x80, touch: true }]);
        let r = decode(&lift);
        assert_eq!(events(MTouch, &r[0]), vec![Event::Fader { id: 0x4203, name: "LEVEL PLAYBACK 1", value: 0x80, touch: false }]);
    }

    // --- analog bank, M-Play, 41 bytes (mplay-notes.md; mplay-listen-session.log line 19 =
    //     mplay-listen-decoded.txt line 1)

    #[test]
    fn mplay_bank_fader_pfd1() {
        let r = decode(&hexv("c20c059210020000000000000000000000180000000000000000000000010000000000000000000000"));
        match &r[0] {
            Report::AnalogBank { base, stride, units } => assert_eq!((*base, *stride, units.len()), (0x9205, 0x10, 12)),
            other => panic!("{other:?}"),
        }
        assert_eq!(events(MPlay, &r[0]), vec![Event::Pressure { id: 0x9205, name: "FADER PFD 1", value: 24, touch: true }]);
        // log line 37 / decoded line 19: bottom button of strip 1
        assert_eq!(
            one(MPlay, "c20cc59210020000000000000000000000ff0000000000000000000000010000000000000000000000"),
            vec![Event::Pressure { id: 0x92C5, name: "FADER PFD 13", value: 255, touch: true }]
        );
        // log line 70 / decoded line 52: fader 1 lift at the top
        assert_eq!(
            one(MPlay, "c20c039210020000000000000000000000ff0000000000000000000000000000000000000000000000"),
            vec![Event::Fader { id: 0x9203, name: "FADER LEVEL 1", value: 255, touch: false }]
        );
    }

    /// No key bank occurs on the M-Play (every button is pressure-sensing), but the shape with
    /// N = 0x0C is 29 bytes and must decode (mplay-notes.md "Reports").
    #[test]
    fn mplay_key_bank_shape_decodes() {
        let mut d = vec![0xC1, 0x0C, 0x05, 0x92, 0x10];
        d.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02]);
        d.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01]);
        assert_eq!(d.len(), 29);
        let r = decode(&d);
        assert_eq!(r.len(), 1);
        assert_eq!(events(MPlay, &r[0]), vec![Event::KeyDown { id: 0x92B5, name: "FADER PFD 12" }]);
    }

    // --- fixture tables straight from the capture logs

    /// `(log line, raw hex, id, value, touch)`: one analog-bank or single-fader line each from
    /// `captures/20260923-listen-session-b.log` (M-Touch). The expected values are what the
    /// log's own decoder printed on that line.
    const MTOUCH_ANALOG_LINES: &[(u32, &str, u16, u8, bool)] = &[
        (37, "c20a01580102000000000000000000b000000000000000000001000000000000000000", 0x5801, 0xb0, true),
        (49, "c20a01580102000000000000000000cd00000000000000000001000000000000000000", 0x5801, 0xcd, true),
        (53, "c20a015801020000000000000000000000000000000000000000000000000000000000", 0x5801, 0x00, false),
        (55, "c20a0158010002000000000000000000f9000000000000000000010000000000000000", 0x5802, 0xf9, true),
        (61, "c20a015801000200000000000000000000000000000000000000000000000000000000", 0x5802, 0x00, false),
        (63, "c20a015801000002000000000000000000ff0000000000000000000100000000000000", 0x5803, 0xff, true),
        (103, "c20a015801000000000002000000000000000000d50000000000000000000100000000", 0x5806, 0xd5, true),
        (179, "c20a054210020000000000000000004c00000000000000000001000000000000000000", 0x4205, 0x4c, true),
        (185, "c20a05421002000000000000000000a100000000000000000001000000000000000000", 0x4205, 0xa1, true),
        (207, "c20a054210020000000000000000000000000000000000000000000000000000000000", 0x4205, 0x00, false),
        (217, "c20a034210020000000000000000000000000000000000000001000000000000000000", 0x4203, 0x00, true),
        (225, "c20a034210020000000000000000000d00000000000000000001000000000000000000", 0x4203, 0x0d, true),
        (477, "c20a03421002000000000000000000ff00000000000000000000000000000000000000", 0x4203, 0xff, false),
        (699, "c20a034210020000000000000000000000000000000000000000000000000000000000", 0x4203, 0x00, false),
        (943, "c20a03421000000000000000000002000000000000000000ff00000000000000000000", 0x4293, 0xff, false),
        (1085, "02021261000001", 0x6112, 0x00, true),
        (1098, "02021261002801", 0x6112, 0x28, true),
        (1228, "0202126100ff00", 0x6112, 0xff, false),
        (1277, "02021261000000", 0x6112, 0x00, false),
    ];

    /// `(log line, raw hex, id, down)` from `captures/20260923-listen-session-b.log`.
    const MTOUCH_KEY_LINES: &[(u32, &str, u16, bool)] = &[
        (3, "0102114101", 0x4111, true),
        (4, "0102114100", 0x4111, false),
        (5, "0102134101", 0x4113, true),
        (7, "0102045501", 0x5504, true),
        (9, "0102025501", 0x5502, true),
        (11, "0102115501", 0x5511, true),
        (13, "0102035501", 0x5503, true),
        (15, "0102125501", 0x5512, true),
        (17, "0102135501", 0x5513, true),
        (19, "0102015401", 0x5401, true),
        (21, "0102015101", 0x5101, true),
        (23, "0102025401", 0x5402, true),
        (25, "0102115401", 0x5411, true),
        (27, "0102035101", 0x5103, true),
        (29, "0102076101", 0x6107, true),
        (31, "0102016401", 0x6401, true),
        (33, "0102026401", 0x6402, true),
        (35, "0102016001", 0x6001, true),
        (1229, "0102116101", 0x6111, true),
        (1230, "0102116100", 0x6111, false),
        (175, "c10a0142100200000000000000000001000000000000000000", 0x4201, true),
        (176, "c10a0142100200000000000000000000000000000000000000", 0x4201, false),
        (177, "c10a0242100200000000000000000001000000000000000000", 0x4202, true),
        (178, "c10a0242100200000000000000000000000000000000000000", 0x4202, false),
    ];

    /// `(log line, raw hex, id, value, touch)` from `captures/20260923-mplay-listen-session.log`;
    /// the expected values are the matching line of `20260923-mplay-listen-decoded.txt`
    /// (decoded line = log line - 18).
    const MPLAY_ANALOG_LINES: &[(u32, &str, u16, u8, bool)] = &[
        (19, "c20c059210020000000000000000000000180000000000000000000000010000000000000000000000", 0x9205, 24, true),
        (23, "c20c0592100200000000000000000000008e0000000000000000000000010000000000000000000000", 0x9205, 142, true),
        (30, "c20c059210020000000000000000000000000000000000000000000000000000000000000000000000", 0x9205, 0, false),
        (37, "c20cc59210020000000000000000000000ff0000000000000000000000010000000000000000000000", 0x92C5, 255, true),
        (40, "c20cc59210020000000000000000000000000000000000000000000000000000000000000000000000", 0x92C5, 0, false),
        (41, "c20c039210020000000000000000000000000000000000000000000000010000000000000000000000", 0x9203, 0, true),
        (70, "c20c039210020000000000000000000000ff0000000000000000000000000000000000000000000000", 0x9203, 255, false),
        (71, "c20c039210020000000000000000000000ff0000000000000000000000010000000000000000000000", 0x9203, 255, true),
        (86, "c20c0592100000000000000000000000020000000000000000000000f0000000000000000000000001", 0x92B5, 240, true),
        (128, "c20c059810020000000000000000000000020000000000000000000000010000000000000000000000", 0x9805, 2, true),
        (129, "c20c059810020000000000000000000000000000000000000000000000000000000000000000000000", 0x9805, 0, false),
        (166, "c20cc59810020000000000000000000000ff0000000000000000000000010000000000000000000000", 0x98C5, 255, true),
    ];

    /// `(log line, raw hex, id, down)` from `captures/20260923-mplay-listen-session.log`.
    const MPLAY_KEY_LINES: &[(u32, &str, u16, bool)] = &[
        (3, "0102139f01", 0x9F13, true),
        (4, "0102139f00", 0x9F13, false),
        (5, "0102129f01", 0x9F12, true),
        (7, "0102045501", 0x5504, true),
        (17, "0102135501", 0x5513, true),
        (248, "0102139e01", 0x9E13, true),
        (250, "0102129e01", 0x9E12, true),
    ];

    fn check_analog(model: Model, lines: &[(u32, &str, u16, u8, bool)]) {
        for (line, raw, id, value, touch) in lines {
            let ev = one(model, raw);
            assert_eq!(ev.len(), 1, "line {line}: {ev:?}");
            let (got_id, got_value, got_touch) = match &ev[0] {
                Event::Fader { id, value, touch, .. } | Event::Pressure { id, value, touch, .. } => (*id, *value, *touch),
                other => panic!("line {line}: {other:?}"),
            };
            assert_eq!((got_id, got_value, got_touch), (*id, *value, *touch), "line {line}");
            assert_ne!(name_of(model, *id), None, "line {line}: id {id:04X} missing from the table");
            let expect_pressure = kind_of(model, *id) == Some('V');
            assert_eq!(matches!(ev[0], Event::Pressure { .. }), expect_pressure, "line {line}");
        }
    }

    fn check_keys(model: Model, lines: &[(u32, &str, u16, bool)]) {
        for (line, raw, id, down) in lines {
            let ev = one(model, raw);
            let expected = if *down { Event::KeyDown { id: *id, name: name_of(model, *id).unwrap() } } else { Event::KeyUp { id: *id, name: name_of(model, *id).unwrap() } };
            assert_eq!(ev, vec![expected], "line {line}");
        }
    }

    #[test]
    fn mtouch_capture_lines_decode_as_logged() {
        check_analog(MTouch, MTOUCH_ANALOG_LINES);
        check_keys(MTouch, MTOUCH_KEY_LINES);
    }

    #[test]
    fn mplay_capture_lines_decode_as_logged() {
        check_analog(MPlay, MPLAY_ANALOG_LINES);
        check_keys(MPlay, MPLAY_KEY_LINES);
    }

    /// Two faders moved at once share a report (protocol.md 5.4; listen-session-b.log line 1320,
    /// mplay log line 227 = decoded line 209). Units that are touched but unchanged keep their
    /// value with `changed` false; untouched units carry their resting value (strip 1 at 0x7e).
    #[test]
    fn two_faders_in_one_report() {
        let ev = one(MTouch, "c20a034210000202000000000000007e2a430000000000000000010100000000000000");
        assert_eq!(
            ev,
            vec![
                Event::Fader { id: 0x4213, name: "LEVEL PLAYBACK 2", value: 0x2a, touch: true },
                Event::Fader { id: 0x4223, name: "LEVEL PLAYBACK 3", value: 0x43, touch: true },
            ]
        );
        let r = decode(&hexv("c20a034210000202000000000000007e2a430000000000000000010100000000000000"));
        if let Report::AnalogBank { units, .. } = &r[0] {
            assert_eq!(units[0], Unit { id: 0x4203, value: 0x7e, touch: false, changed: false });
        }
        let ev = one(MPlay, "c20c039210000202000000000000000000008483000000000000000000000101000000000000000000");
        assert_eq!(
            ev,
            vec![
                Event::Fader { id: 0x9213, name: "FADER LEVEL 2", value: 132, touch: true },
                Event::Fader { id: 0x9223, name: "FADER LEVEL 3", value: 131, touch: true },
            ]
        );
        // log line 247 / decoded line 229: both lifted in one report
        let ev = one(MPlay, "c20c03921000020200000000000000000000fff5000000000000000000000000000000000000000000");
        assert_eq!(
            ev,
            vec![
                Event::Fader { id: 0x9213, name: "FADER LEVEL 2", value: 255, touch: false },
                Event::Fader { id: 0x9223, name: "FADER LEVEL 3", value: 245, touch: false },
            ]
        );
    }

    // --- packet framing (protocol.md 5: "two short reports can share one packet", cap1 frames
    //     1716, 1730, 1756; idle polls answer with a zero-length packet)

    #[test]
    fn concatenated_reports_split() {
        let r = decode(&hexv("01021141010102134101"));
        assert_eq!(r, vec![Report::Key { id: 0x4111, down: true }, Report::Key { id: 0x4113, down: true }]);
        let r = decode(&hexv("010211410002021261002801"));
        assert_eq!(r, vec![Report::Key { id: 0x4111, down: false }, Report::Fader { id: 0x6112, value: 0x28, touch: true }]);
        // a key report followed by a whole analog bank
        let mut d = hexv("0102114101");
        d.extend_from_slice(&hexv("c20a01580102000000000000000000b000000000000000000001000000000000000000"));
        let r = decode(&d);
        assert_eq!(r.len(), 2);
        assert!(matches!(r[1], Report::AnalogBank { base: 0x5801, .. }));
    }

    #[test]
    fn idle_and_unknown_packets() {
        assert_eq!(decode(&[]), vec![]);
        assert_eq!(decode(&[0x09, 0x09, 0x09]), vec![Report::Unknown { bytes: vec![9, 9, 9] }]);
        // a good report followed by a truncated one: the remainder is one Unknown
        let r = decode(&hexv("0102114101c20a0158"));
        assert_eq!(r.len(), 2);
        assert_eq!(r[1], Report::Unknown { bytes: hexv("c20a0158") });
        // a bank whose declared N does not fit the packet is unknown as a whole
        assert!(matches!(decode(&hexv("c20a0142100200"))[0], Report::Unknown { .. }));
        assert!(events(MTouch, &Report::Unknown { bytes: vec![1] }).is_empty());
    }

    // --- LED key encoding (protocol.md 4.1 common values; Onyx frames cap1 2408, 2524)

    #[test]
    fn led_key_values() {
        assert_eq!(LedKey::off().value(), 0x0000);
        assert_eq!(LedKey::off().green().value(), 0x0001);
        assert_eq!(LedKey::off().red().value(), 0x0002);
        assert_eq!(LedKey::off().yellow().value(), 0x0003);
        assert_eq!(LedKey::off().green().red().value(), LED_YELLOW);
        assert_eq!(LedKey::off().blue().value(), 0x0100);
        assert_eq!(LedKey::off().green().fast().value(), 0x0004 | 0x0001);
        assert_eq!(LedKey::off().green().fast().value() & !LED_GREEN, 0x0004);
        assert_eq!(LedKey::off().red().fast().value() & !LED_RED, 0x0008);
        assert_eq!(LedKey::off().blue().fast().value() & !LED_BLUE, 0x0400);
        // Onyx's slow blinks keep the on bit: 0x0011 green, 0x0042 red (cap1 frames 2408, 2524)
        assert_eq!(LedKey::off().green().slow().value(), 0x0011);
        assert_eq!(LedKey::off().red().slow().value(), 0x0042);
        assert_eq!(LedKey::off().blue().slow().value(), 0x1000 | LED_BLUE);
        assert_eq!(LedKey::off().green().force_off().value(), 0x0001 | 0x0020);
        assert_eq!(LedKey::off().red().force_off().value(), 0x0002 | 0x0080);
        assert_eq!(LedKey::off().blue().force_off().value(), 0x0100 | 0x2000);
        // Onyx writes 0x0103 for blue keys; the low bits are harmless (4.1). Ours is the plain lane.
        assert_eq!(LED_BLUE, 0x0100);
        assert_ne!(0x0103 & LED_BLUE, 0);
        // blink flags without a colour select nothing
        assert_eq!(LedKey::off().fast().slow().value(), 0);
    }

    // --- fader bar (protocol.md 4.2; cap2 frames 236..359: level masks in red for a CUELIST)

    #[test]
    fn fader_bar_levels_and_blink() {
        for n in 0..=10u8 {
            let mask = ((1u32 << n) - 1) as u16;
            let d = bar_level(n, BarColour::Red, false);
            // red is the middle word, bytes 3..5
            assert_eq!(&d[0..3], &[0, 0, 0], "n={n}");
            assert_eq!(u32::from_le_bytes([d[3], d[4], d[5], 0]), mask as u32, "n={n}");
            assert_eq!(&d[6..9], &[0, 0, 0], "n={n}");
        }
        assert_eq!(bar_level(1, BarColour::Red, false)[3], 0x01);
        assert_eq!(bar_level(2, BarColour::Red, false)[3], 0x03);
        assert_eq!(bar_level(3, BarColour::Red, false)[3], 0x07);
        assert_eq!(&bar_level(10, BarColour::Red, false)[3..6], &[0xFF, 0x03, 0x00]);
        // more than 10 LEDs do not exist
        assert_eq!(bar_level(11, BarColour::Red, false), bar_level(10, BarColour::Red, false));
        assert_eq!(fader_bar(0xFFFF, 0, 0, (false, false, false))[0..3], [0xFF, 0x03, 0x00]);
        // blink is bit 23 of that colour's word
        let d = bar_level(5, BarColour::Green, true);
        assert_eq!(&d[0..3], &[0x1F, 0x00, 0x80]);
        assert_eq!(&d[3..9], &[0; 6]);
        // word order green, red, blue; mixes light more than one word
        let d = bar_level(10, BarColour::White, false);
        assert_eq!(d, [0xFF, 0x03, 0, 0xFF, 0x03, 0, 0xFF, 0x03, 0]);
        let d = bar_level(4, BarColour::Cyan, false);
        assert_eq!(d, [0x0F, 0, 0, 0, 0, 0, 0x0F, 0, 0]);
        assert_eq!(bar_level(7, BarColour::Off, true), [0; 9]);
        // base-channel bars are blue-only (4.2): their level is the blue word, bytes 6..8
        let d = bar_level(3, BarColour::Blue, false);
        assert_eq!(d, [0, 0, 0, 0, 0, 0, 0x07, 0, 0]);
        // independent lanes: bottom five green, top five red
        let d = fader_bar(0x01F, 0x3E0, 0, (false, true, false));
        assert_eq!(d, [0x1F, 0, 0, 0xE0, 0x03, 0x80, 0, 0, 0]);
    }

    // --- page display (protocol.md 4.3; Onyx wrote 00 17 06 00 00 00 for page 1, cap1 frame 3777)

    #[test]
    fn page_display_digits() {
        let onyx_page_1 = [0x00, 0x17, 0x06, 0x00, 0x00, 0x00];
        let ours = page_display(Some(1));
        assert_eq!(ours[2..5], onyx_page_1[2..5]);
        assert_eq!(ours, [0, 0, 0x06, 0, 0, 0]);
        assert_eq!(page_display(Some(0)), [0, 0, 0x3F, 0, 0, 0]);
        assert_eq!(page_display(Some(10)), [0, 0, 0x3F, 0x06, 0, 0]);
        assert_eq!(page_display(Some(123)), [0, 0, 0x4F, 0x5B, 0x06, 0]);
        assert_eq!(page_display(Some(999)), [0, 0, 0x6F, 0x6F, 0x6F, 0]);
        assert_eq!(page_display(Some(1000)), page_display(Some(999)));
        assert_eq!(page_display(None), [0; 6]);
    }

    #[test]
    fn outputs_carry_the_documented_requests() {
        let led = Output::LedKey { id: 0x5101, value: LED_BLUE };
        assert_eq!((led.request(), led.index(), led.value(), led.data()), (0x80, 0x5101, 0x0100, &[][..]));
        let bar = Output::FaderBar { id: 0x4203, data: bar_level(10, BarColour::Red, false) };
        assert_eq!((bar.request(), bar.index(), bar.value(), bar.data().len()), (0x61, 0x4203, 0, 9));
        let page = Output::PageDisplay { id: DISPLAY_MTOUCH, data: page_display(Some(1)) };
        assert_eq!((page.request(), page.index(), page.data().len()), (0x54, 0x4401, 6));
        assert!(!(0xA0..=0xAF).contains(&led.request()) && !(0xA0..=0xAF).contains(&bar.request()) && !(0xA0..=0xAF).contains(&page.request()));
    }

    // --- control tables (protocol.md section 3, mplay-notes.md, tools/mtouch-controls.tsv)

    #[test]
    fn mtouch_table_matches_the_documented_layout() {
        let t = CONTROLS_MTOUCH;
        assert_eq!(t.len(), 81, "tools/mtouch-controls.tsv has 81 lines");
        let ids: std::collections::BTreeSet<u16> = t.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), t.len(), "duplicate ids");
        // 10 strips x 4 controls at base 0x4200 + 0x10*(n-1)
        for n in 1..=10u16 {
            let base = 0x4200 + 0x10 * (n - 1);
            assert_eq!(kind_of(MTouch, base + 1), Some('K'), "strip {n} PFA");
            assert_eq!(kind_of(MTouch, base + 2), Some('K'), "strip {n} PFB");
            assert_eq!(kind_of(MTouch, base + 3), Some('F'), "strip {n} fader");
            assert_eq!(kind_of(MTouch, base + 5), Some('V'), "strip {n} PFD");
            assert_eq!(name_of(MTouch, base + 3), Some(format!("LEVEL PLAYBACK {n}").as_str()));
        }
        // 4 base-channel pairs at 0x6100 + 0x10*n
        for n in 1..=4u16 {
            let base = 0x6100 + 0x10 * n;
            assert_eq!(kind_of(MTouch, base + 1), Some('K'), "base channel {n}");
            assert_eq!(kind_of(MTouch, base + 2), Some('F'), "base channel {n} fader");
        }
        // the button list of section 3.3
        for (id, kind) in [
            (0x4111, 'X'), (0x4113, 'X'), (0x5101, 'K'), (0x5103, 'K'), (0x5401, 'K'), (0x5402, 'K'), (0x5411, 'K'),
            (0x5502, 'K'), (0x5503, 'K'), (0x5504, 'K'), (0x5511, 'K'), (0x5512, 'K'), (0x5513, 'K'),
            (0x5812, 'L'), (0x5813, 'L'), (0x5814, 'L'), (0x5815, 'L'), (0x6001, 'K'), (0x6107, 'X'), (0x6401, 'K'), (0x6402, 'K'),
        ] {
            assert_eq!(kind_of(MTouch, id), Some(kind), "{id:04X}");
        }
        for n in 1..=10u16 {
            assert_eq!(kind_of(MTouch, 0x5800 + n), Some('V'), "MF {n}");
        }
        assert_eq!(kind_of(MTouch, DISPLAY_MTOUCH), Some('D'));
        assert_eq!(kind_of(MTouch, 0x7110), Some('B'));
        // the TSV sample: first, a middle and the last line
        assert_eq!(name_of(MTouch, 0x4111), Some("PLAYBACK BANK Up (+)"));
        assert_eq!(name_of(MTouch, 0x580A), Some("MF 10"));
        assert_eq!(name_of(MTouch, 0x7110), Some("BackLight"));
        let count = |k: char| t.iter().filter(|c| c.kind == k).count();
        assert_eq!((count('K'), count('V'), count('F'), count('L'), count('D'), count('X'), count('B')), (38, 20, 14, 4, 1, 3, 1), "20 strip K + 14 named K + 4 base-channel K; 10 PFD + 10 MF V; 10 + 4 bars");
        assert_eq!(Model::from_pid(0xF808), Some(MTouch));
        assert_eq!(MTouch.displays(), &[0x4401]);
    }

    #[test]
    fn mplay_table_matches_the_documented_layout() {
        let t = CONTROLS_MPLAY;
        let ids: std::collections::BTreeSet<u16> = t.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), t.len(), "duplicate ids");
        // 12 strips x 3: fader 0x9203, top 0x9205, bottom 0x92C5 (+0x10 per strip)
        for n in 1..=12u16 {
            let off = 0x10 * (n - 1);
            assert_eq!(kind_of(MPlay, 0x9203 + off), Some('F'), "strip {n}");
            assert_eq!(kind_of(MPlay, 0x9205 + off), Some('V'), "strip {n} top");
            assert_eq!(kind_of(MPlay, 0x92C5 + off), Some('V'), "strip {n} bottom");
            assert_eq!(name_of(MPlay, 0x92C5 + off), Some(format!("FADER PFD {}", 12 + n).as_str()));
            // 24 right-block buttons: 0x9805 rows 1-3, 0x98C5 rows 4-6
            assert_eq!(kind_of(MPlay, 0x9805 + off), Some('V'), "right {n}");
            assert_eq!(kind_of(MPlay, 0x98C5 + off), Some('V'), "right {}", 12 + n);
        }
        let count = |k: char| t.iter().filter(|c| c.kind == k).count();
        assert_eq!((count('F'), count('V'), count('D'), count('X'), count('K'), count('B')), (12, 48, 2, 4, 6, 1), "12 strips x 3 (12 F + 24 V) + 24 right-block V");
        assert_eq!(MPlay.displays(), &[0x9F01, 0x9E01]);
        assert_eq!(kind_of(MPlay, 0x9F01), Some('D'));
        assert_eq!(kind_of(MPlay, 0x9E01), Some('D'));
        for id in [0x9F12, 0x9F13, 0x9E12, 0x9E13] {
            assert_eq!(kind_of(MPlay, id), Some('X'), "{id:04X}");
        }
        // no base-channel faders and no M-Touch strips on the M-Play
        assert_eq!(kind_of(MPlay, 0x6112), None);
        assert_eq!(kind_of(MPlay, 0x4203), None);
        assert_eq!(Model::from_pid(0xF80C), Some(MPlay));
        assert_eq!(Model::from_pid(0xE102), None);
        assert_eq!((MTouch.bank_units(), MPlay.bank_units()), (0x0A, 0x0C));
    }
}

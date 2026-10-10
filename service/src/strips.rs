//! KB-20: the M-Touch's four right-hand parameter strips (the base-channel faders, `VALUE BASE CHANNEL
//! 1..4` at 0x6112/0x6122/0x6132/0x6142, 8-bit position 0..255 bottom to top, `touch` flag, lift report
//! with the resting value; `docs/mtouch-protocol-reuse.md`) act as the encoders the NX-K rotaries are:
//! strip n means encoder slot `--strip-slots + n - 1` of the bound display.
//!
//! Gesture conversion lives here, nothing else does: the output is the same `CtlKind` stream the rotaries
//! produce (`Touch`, `Rel`, `Abs`) and the link's admission, the plugin and the vendored console backend
//! treat it identically. The console backend serves a touch as a hold (busy, owned, nothing moves), a
//! relative delta as `Attribute "<name>" At +/- <detents x step>` and a position as `At <value>`.
//!
//! Relative mode (the default):
//!   * the first report with `touch = 1` ANCHORS the strip at its position and emits a touch down; the
//!     position itself moves nothing (no jump to the touched spot);
//!   * each later report moves by `(value - previous) x detents_per_travel / 255` detents, the fraction
//!     carried over, so a slow drag still adds up and one count of jitter (0.47 detents) never emits on its
//!     own (three counts in one direction are needed before a detent goes out);
//!   * the lift report emits the touch up and nothing else (its resting value is ignored); a new touch
//!     re-anchors, so repeated travel across a large range is a matter of lifting and touching again;
//!   * `fine` is the NX-K's Bank held or this strip's own capacitive key (`BASE CHANNEL n`, 0x61n1) held;
//!   * a binding generation that moves while the strip is touched marks the gesture REBOUND: its motion is
//!     dropped here until the lift (the plugin would refuse it `gesture-rebound` anyway); the next touch is
//!     a fresh gesture against the new binding;
//!   * a touch longer than `max_touch` (a missed lift) is ended locally; the next report with `touch = 1`
//!     re-anchors; a device disconnect ends every touch.
//!
//! Absolute mode (`--strips absolute`, an explicit choice): a touch down anchors as above, then positions
//! go out only after TAKEOVER: either the strip crossed (or landed within `pickup_tolerance` of) the slot's
//! last known value from the plugin's context (`abs` as 0..1 of the travel: the programmer value is a
//! percentage of the range for every readout) - the pickup - or the strip's key was held at touch-down,
//! the explicit takeover, which also marks the events `takeover` so the console backend places the value
//! on a mixed selection. A slot whose value is unknown, empty or mixed cannot be picked up: the strip
//! waits (counted) until an explicit takeover. Reconnects, page changes and touch-down therefore never
//! jump: a page change while touched is a rebound (lift and touch again, then pick up again).
//!
//! Pressure is never an input: the M-Touch's faders report none and its pressure keys are not strips.
//! The playback strips (0x42n3) are not parameters here (KB-21/22).

use crate::link::CtlKind;
use crate::protocol::CtlTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Relative,
    Absolute,
}

impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Mode, String> {
        match s.to_ascii_lowercase().as_str() {
            "off" | "none" => Ok(Mode::Off),
            "relative" | "rel" => Ok(Mode::Relative),
            "absolute" | "abs" => Ok(Mode::Absolute),
            other => Err(format!("--strips {other}: expected relative, absolute or off")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    /// The encoder slot strip 1 means (strips 1-4 are slots base..base+3), as `--rotary-slots` for the rotaries.
    pub base_slot: u8,
    /// Detents across the strip's full travel: 120 is five encoder turns, the range of an attribute at the
    /// console's own encoders (manual: 24 clicks per turn, 5 turns per range), so a full stroke is one range.
    pub detents_per_travel: i32,
    /// A touch without a lift report for longer than this is ended locally (seconds).
    pub max_touch: f64,
    /// Absolute mode: a position within this distance (0..1 of the travel) of the slot's last known value picks it up.
    pub pickup_tolerance: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config { mode: Mode::Relative, base_slot: 1, detents_per_travel: 120, max_touch: 30.0, pickup_tolerance: 0.02 }
    }
}

/// What the plugin's context last said about a slot's value, for the absolute-mode pickup.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotHint {
    /// `value`: a single known value (`abs` set); anything else cannot be picked up.
    pub single: bool,
    /// The value as 0..1 of the travel (the programmer's `absolute` / 100).
    pub value: Option<f64>,
}

/// One control event for the link.
#[derive(Debug, Clone, PartialEq)]
pub struct Out {
    pub control: &'static str,
    pub target: CtlTarget,
    pub kind: CtlKind,
}

#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub reports: u64,
    pub touches: u64,
    pub lifts: u64,
    pub detents: u64,
    pub positions: u64,
    /// Motion dropped because the binding moved while the strip stayed touched.
    pub rebound_dropped: u64,
    /// Touches ended locally after `max_touch` or on disconnect.
    pub forced_lifts: u64,
    /// Absolute mode: reports that waited for pickup or takeover.
    pub waiting_pickup: u64,
    /// Explicit takeovers (strip key held at touch-down).
    pub takeovers: u64,
    /// Reports for faders that are not parameter strips (playback strips, pressure keys).
    pub ignored: u64,
}

#[derive(Debug, Clone, Default)]
struct Strip {
    touching: bool,
    anchor: u8,
    /// Accumulated motion in counts x detents_per_travel (exact integer arithmetic: a full stroke is exactly the configured detents).
    acc: i32,
    since: f64,
    generation: Option<u64>,
    rebound: bool,
    key_held: bool,
    picked_up: bool,
    takeover: bool,
    last_v: f64,
}

pub const CONTROLS: [&str; 4] = ["Strip1", "Strip2", "Strip3", "Strip4"];

/// The M-Touch base-channel fader ids, strip 1..4.
pub fn strip_of_fader(id: u16) -> Option<u8> {
    match id {
        0x6112 => Some(1),
        0x6122 => Some(2),
        0x6132 => Some(3),
        0x6142 => Some(4),
        _ => None,
    }
}

/// The M-Touch base-channel key ids (the strip's own fine / takeover modifier), strip 1..4.
pub fn strip_of_key(id: u16) -> Option<u8> {
    match id {
        0x6111 => Some(1),
        0x6121 => Some(2),
        0x6131 => Some(3),
        0x6141 => Some(4),
        _ => None,
    }
}

pub struct Strips {
    pub cfg: Config,
    strips: [Strip; 4],
    pub stats: Stats,
    pub log: Vec<String>,
}

impl Strips {
    pub fn new(cfg: Config) -> Strips {
        Strips { cfg, strips: Default::default(), stats: Stats::default(), log: Vec::new() }
    }

    fn target(&self, n: u8) -> CtlTarget {
        CtlTarget::Slot { slot: self.cfg.base_slot.clamp(1, 5) + n.clamp(1, 4) - 1 }
    }

    fn out(&self, n: u8, kind: CtlKind) -> Out {
        Out { control: CONTROLS[(n.clamp(1, 4) - 1) as usize], target: self.target(n), kind }
    }

    #[cfg(test)]
    pub fn touching(&self, n: u8) -> bool {
        self.strips[(n.clamp(1, 4) - 1) as usize].touching
    }

    /// The strip's own key: held, it is the fine modifier (relative) or, at touch-down, the explicit takeover (absolute).
    pub fn key(&mut self, n: u8, down: bool) {
        self.strips[(n.clamp(1, 4) - 1) as usize].key_held = down;
    }

    /// One fader report of strip `n`. `fine` is the external modifier (NX-K Bank); `cg` the current binding
    /// generation (`None`: unbound, nothing but a lift goes out); `hint` the slot's last known value for pickup.
    pub fn fader(&mut self, n: u8, value: u8, touch: bool, fine: bool, cg: Option<u64>, hint: Option<SlotHint>, now: f64) -> Vec<Out> {
        self.stats.reports += 1;
        if self.cfg.mode == Mode::Off {
            return Vec::new();
        }
        let i = (n.clamp(1, 4) - 1) as usize;
        let mut outs = Vec::new();
        if !touch {
            // The lift report: end the touch, ignore the resting value.
            if self.strips[i].touching {
                self.strips[i].touching = false;
                self.strips[i].picked_up = false;
                self.stats.lifts += 1;
                outs.push(self.out(n, CtlKind::Touch { down: false }));
            }
            return outs;
        }
        let fine = fine || self.strips[i].key_held;
        if !self.strips[i].touching {
            // Touch-down: anchor, no motion, a new gesture.
            let s = &mut self.strips[i];
            s.touching = true;
            s.anchor = value;
            s.acc = 0;
            s.since = now;
            s.generation = cg;
            s.rebound = cg.is_none();
            s.picked_up = false;
            s.takeover = self.cfg.mode == Mode::Absolute && s.key_held;
            s.last_v = value as f64 / 255.0;
            self.stats.touches += 1;
            if s.takeover {
                self.stats.takeovers += 1;
            }
            outs.push(self.out(n, CtlKind::Touch { down: true }));
            return outs;
        }
        if self.strips[i].generation != cg {
            self.strips[i].rebound = true;
        }
        if self.strips[i].rebound {
            self.strips[i].anchor = value;
            self.stats.rebound_dropped += 1;
            return outs;
        }
        match self.cfg.mode {
            Mode::Off => {}
            Mode::Relative => {
                let s = &mut self.strips[i];
                let d = value as i32 - s.anchor as i32;
                s.anchor = value;
                s.acc += d * self.cfg.detents_per_travel;
                let det = s.acc / 255;  // truncates toward zero: the fraction stays in acc, signed
                if det != 0 {
                    s.acc -= det * 255;
                    self.stats.detents += det.unsigned_abs() as u64;
                    outs.push(self.out(n, CtlKind::Rel { dx: det, fine }));
                }
            }
            Mode::Absolute => {
                let v = value as f64 / 255.0;
                let s = &mut self.strips[i];
                if !s.picked_up {
                    if s.takeover {
                        s.picked_up = true;
                    } else if let Some(h) = hint.filter(|h| h.single) {
                        if let Some(target) = h.value {
                            let crossed = (s.last_v - target) * (v - target) <= 0.0;
                            if crossed || (v - target).abs() <= self.cfg.pickup_tolerance {
                                s.picked_up = true;
                            }
                        }
                    }
                }
                s.last_v = v;
                if s.picked_up {
                    self.stats.positions += 1;
                    let takeover = s.takeover;
                    outs.push(self.out(n, CtlKind::Abs { v, takeover }));
                } else {
                    self.stats.waiting_pickup += 1;
                }
            }
        }
        outs
    }

    /// Housekeeping: a moved generation marks touched strips rebound; a touch past `max_touch` is ended.
    pub fn tick(&mut self, cg: Option<u64>, now: f64) -> Vec<Out> {
        let mut outs = Vec::new();
        for n in 1..=4u8 {
            let i = (n - 1) as usize;
            if !self.strips[i].touching {
                continue;
            }
            if self.strips[i].generation != cg && !self.strips[i].rebound {
                self.strips[i].rebound = true;
                self.log.push(format!("{}: the binding changed while touched; lift and touch again", CONTROLS[i]));
            }
            if now - self.strips[i].since > self.cfg.max_touch {
                self.strips[i].touching = false;
                self.strips[i].picked_up = false;
                self.stats.forced_lifts += 1;
                self.log.push(format!("{}: touched for more than {:.0} s without a lift report; ended locally", CONTROLS[i], self.cfg.max_touch));
                outs.push(self.out(n, CtlKind::Touch { down: false }));
            }
        }
        outs
    }

    /// The device went away: every touch ends.
    pub fn disconnect(&mut self) -> Vec<Out> {
        let mut outs = Vec::new();
        for n in 1..=4u8 {
            let i = (n - 1) as usize;
            if self.strips[i].touching {
                self.strips[i] = Strip { key_held: false, ..Default::default() };
                self.stats.forced_lifts += 1;
                outs.push(self.out(n, CtlKind::Touch { down: false }));
            } else {
                self.strips[i].key_held = false;
            }
        }
        outs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel() -> Strips {
        Strips::new(Config::default())
    }
    fn kinds(o: &[Out]) -> Vec<CtlKind> {
        o.iter().map(|x| x.kind).collect()
    }

    #[test]
    fn a_touch_anchors_without_motion_and_the_drag_emits_whole_detents_with_the_fraction_carried() {
        let mut s = rel();
        let o = s.fader(1, 128, true, false, Some(3), None, 1.0);
        assert_eq!(kinds(&o), vec![CtlKind::Touch { down: true }], "touch-down is a boundary only: no jump to the touched position");
        assert_eq!(o[0].control, "Strip1");
        assert_eq!(o[0].target, CtlTarget::Slot { slot: 1 });
        // One count is 120/255 = 0.47 detents: nothing yet. Two counts: 0.94, nothing. Three: 1.41 -> one detent, 0.41 carried.
        assert!(s.fader(1, 129, true, false, Some(3), None, 1.01).is_empty());
        assert!(s.fader(1, 130, true, false, Some(3), None, 1.02).is_empty());
        assert_eq!(kinds(&s.fader(1, 131, true, false, Some(3), None, 1.03)), vec![CtlKind::Rel { dx: 1, fine: false }]);
        // A jump of 51 counts (a fifth of the travel) is 24 detents.
        assert_eq!(kinds(&s.fader(1, 182, true, false, Some(3), None, 1.04)), vec![CtlKind::Rel { dx: 24, fine: false }]);
        // Downwards is negative; the carried fraction keeps the long-run total exact: back to the anchor nets to zero.
        let back = s.fader(1, 128, true, false, Some(3), None, 1.05);
        assert_eq!(kinds(&back), vec![CtlKind::Rel { dx: -25, fine: false }]);
        assert!(s.stats.detents == 50);
        // The lift emits the touch up and ignores its resting value.
        assert_eq!(kinds(&s.fader(1, 10, false, false, Some(3), None, 1.1)), vec![CtlKind::Touch { down: false }]);
        assert!(!s.touching(1));
        // A second lift is nothing.
        assert!(s.fader(1, 10, false, false, Some(3), None, 1.2).is_empty());
    }

    #[test]
    fn a_full_stroke_is_the_configured_detents_and_a_retouch_reanchors() {
        let mut s = rel();
        s.fader(2, 0, true, false, Some(1), None, 0.0);
        let mut total = 0;
        for v in 1..=255u8 {
            for o in s.fader(2, v, true, false, Some(1), None, v as f64 * 0.01) {
                if let CtlKind::Rel { dx, .. } = o.kind { total += dx; }
            }
        }
        assert_eq!(total, 120, "bottom to top is one attribute range: five encoder turns");
        assert_eq!(s.fader(2, 255, false, false, Some(1), None, 3.0)[0].kind, CtlKind::Touch { down: false });
        // Lift, retouch at the bottom, drag up again: the same range once more (repeated travel).
        s.fader(2, 0, true, false, Some(1), None, 3.1);
        let o = s.fader(2, 255, true, false, Some(1), None, 3.2);
        assert_eq!(kinds(&o), vec![CtlKind::Rel { dx: 120, fine: false }]);
        assert_eq!(o[0].target, CtlTarget::Slot { slot: 2 });
    }

    #[test]
    fn jitter_of_one_count_never_oscillates() {
        let mut s = rel();
        s.fader(1, 100, true, false, Some(1), None, 0.0);
        let mut emitted = Vec::new();
        for (i, v) in [101u8, 100, 101, 100, 101, 100, 99, 100, 99, 100].iter().enumerate() {
            emitted.extend(kinds(&s.fader(1, *v, true, false, Some(1), None, i as f64 * 0.01)));
        }
        assert!(emitted.is_empty(), "{emitted:?}");
    }

    #[test]
    fn fine_is_bank_or_the_strips_own_key_and_the_slot_window_follows_base_slot() {
        let mut s = Strips::new(Config { base_slot: 2, ..Config::default() });
        s.fader(1, 0, true, false, Some(1), None, 0.0);
        assert_eq!(kinds(&s.fader(1, 10, true, true, Some(1), None, 0.1)), vec![CtlKind::Rel { dx: 4, fine: true }]);
        s.key(1, true);
        assert_eq!(kinds(&s.fader(1, 20, true, false, Some(1), None, 0.2)), vec![CtlKind::Rel { dx: 5, fine: true }], "4.7 + the carried 0.7");
        s.key(1, false);
        let o = s.fader(1, 30, true, false, Some(1), None, 0.3);
        assert_eq!(kinds(&o), vec![CtlKind::Rel { dx: 5, fine: false }]);
        assert_eq!(o[0].target, CtlTarget::Slot { slot: 2 });
        s.fader(4, 0, true, false, Some(1), None, 0.4);
        assert_eq!(s.fader(4, 255, true, false, Some(1), None, 0.5)[0].target, CtlTarget::Slot { slot: 5 }, "strip 4 with base 2 is the fifth pool slot");
    }

    #[test]
    fn a_binding_change_while_touched_drops_motion_until_the_lift_and_a_new_touch_is_fresh() {
        let mut s = rel();
        s.fader(1, 0, true, false, Some(1), None, 0.0);
        assert_eq!(kinds(&s.fader(1, 51, true, false, Some(1), None, 0.1)), vec![CtlKind::Rel { dx: 24, fine: false }]);
        assert!(s.tick(Some(2), 0.15).is_empty(), "a moved generation ends nothing by itself");
        assert!(s.log.iter().any(|l| l.contains("lift and touch again")));
        assert!(s.fader(1, 102, true, false, Some(2), None, 0.2).is_empty(), "motion after the change is dropped here");
        assert_eq!(s.stats.rebound_dropped, 1);
        assert_eq!(kinds(&s.fader(1, 102, false, false, Some(2), None, 0.3)), vec![CtlKind::Touch { down: false }], "the lift still goes out");
        s.fader(1, 102, true, false, Some(2), None, 0.4);
        assert_eq!(kinds(&s.fader(1, 153, true, false, Some(2), None, 0.5)), vec![CtlKind::Rel { dx: 24, fine: false }], "a new touch is a fresh gesture against the new binding");
        // The change seen first by a report (no tick in between) is a rebound too.
        s.fader(1, 153, false, false, Some(2), None, 0.6);
        s.fader(1, 0, true, false, Some(2), None, 0.7);
        assert!(s.fader(1, 51, true, false, Some(3), None, 0.8).is_empty());
        // Unbound at touch-down: the touch is a boundary the link drops (no generation), motion is dropped here.
        s.fader(1, 51, false, false, Some(3), None, 0.9);
        s.fader(1, 0, true, false, None, None, 1.0);
        assert!(s.fader(1, 51, true, false, None, None, 1.1).is_empty());
    }

    #[test]
    fn a_missed_lift_is_ended_after_max_touch_and_a_disconnect_ends_every_touch() {
        let mut s = Strips::new(Config { max_touch: 2.0, ..Config::default() });
        s.fader(1, 0, true, false, Some(1), None, 0.0);
        s.fader(3, 0, true, false, Some(1), None, 0.0);
        assert!(s.tick(Some(1), 1.9).is_empty());
        let o = s.tick(Some(1), 2.1);
        assert_eq!(o.len(), 2);
        assert!(o.iter().all(|x| x.kind == CtlKind::Touch { down: false }));
        assert_eq!(s.stats.forced_lifts, 2);
        assert!(!s.touching(1) && !s.touching(3));
        // A report with touch still set re-anchors (a new gesture), its motion counted from there.
        assert_eq!(kinds(&s.fader(1, 60, true, false, Some(1), None, 2.2)), vec![CtlKind::Touch { down: true }]);
        assert_eq!(kinds(&s.fader(1, 111, true, false, Some(1), None, 2.3)), vec![CtlKind::Rel { dx: 24, fine: false }]);
        s.key(1, true);
        let d = s.disconnect();
        assert_eq!(kinds(&d), vec![CtlKind::Touch { down: false }]);
        assert_eq!(d[0].control, "Strip1");
        assert!(s.disconnect().is_empty());
        s.fader(1, 0, true, false, Some(1), None, 3.0);
        assert_eq!(kinds(&s.fader(1, 10, true, false, Some(1), None, 3.1)), vec![CtlKind::Rel { dx: 4, fine: false }], "the key state was cleared with the device");
    }

    #[test]
    fn off_emits_nothing_and_pressure_is_not_an_input() {
        let mut s = Strips::new(Config { mode: Mode::Off, ..Config::default() });
        assert!(s.fader(1, 0, true, false, Some(1), None, 0.0).is_empty());
        assert!(s.fader(1, 100, true, false, Some(1), None, 0.1).is_empty());
        assert_eq!(s.stats.reports, 2);
        assert_eq!(strip_of_fader(0x6112), Some(1));
        assert_eq!(strip_of_fader(0x6142), Some(4));
        assert_eq!(strip_of_fader(0x4203), None, "a playback strip is not a parameter strip");
        assert_eq!(strip_of_fader(0x5801), None, "a pressure key is not a strip");
        assert_eq!(strip_of_key(0x6131), Some(3));
        assert_eq!(strip_of_key(0x6112), None);
        assert_eq!("rel".parse::<Mode>(), Ok(Mode::Relative));
        assert!("faders".parse::<Mode>().is_err());
    }

    #[test]
    fn absolute_mode_waits_for_pickup_or_takeover_and_never_jumps_on_touch_or_reconnect() {
        let mut s = Strips::new(Config { mode: Mode::Absolute, ..Config::default() });
        let known = Some(SlotHint { single: true, value: Some(0.5) });
        // Touch far from the value: nothing moves; the finger approaches from below and crosses 0.5: picked up.
        assert_eq!(kinds(&s.fader(1, 25, true, false, Some(1), known, 0.0)), vec![CtlKind::Touch { down: true }]);
        assert!(s.fader(1, 60, true, false, Some(1), known, 0.1).is_empty());
        assert!(s.fader(1, 100, true, false, Some(1), known, 0.2).is_empty());
        assert_eq!(s.stats.waiting_pickup, 2);
        let o = s.fader(1, 130, true, false, Some(1), known, 0.3);
        assert_eq!(kinds(&o), vec![CtlKind::Abs { v: 130.0 / 255.0, takeover: false }]);
        assert_eq!(kinds(&s.fader(1, 200, true, false, Some(1), known, 0.4)), vec![CtlKind::Abs { v: 200.0 / 255.0, takeover: false }], "once picked up every position goes out");
        assert_eq!(kinds(&s.fader(1, 200, false, false, Some(1), known, 0.5)), vec![CtlKind::Touch { down: false }]);
        // Landing within the tolerance picks up too.
        s.fader(1, 0, true, false, Some(1), known, 1.0);
        assert_eq!(s.fader(1, 128, true, false, Some(1), known, 1.1).len(), 1, "128/255 = 0.502 is within 0.02 of 0.5");
        s.fader(1, 128, false, false, Some(1), known, 1.2);
        // No single value (empty, mixed, unknown): no pickup; the strip waits until an explicit takeover.
        let mixed = Some(SlotHint { single: false, value: Some(0.3) });
        s.fader(1, 0, true, false, Some(1), mixed, 2.0);
        assert!(s.fader(1, 255, true, false, Some(1), mixed, 2.1).is_empty());
        assert!(s.fader(1, 0, true, false, Some(1), None, 2.2).is_empty());
        s.fader(1, 0, false, false, Some(1), mixed, 2.3);
        s.key(1, true);
        assert_eq!(kinds(&s.fader(1, 10, true, false, Some(1), mixed, 2.4)), vec![CtlKind::Touch { down: true }]);
        s.key(1, false);
        assert_eq!(kinds(&s.fader(1, 20, true, false, Some(1), mixed, 2.5)), vec![CtlKind::Abs { v: 20.0 / 255.0, takeover: true }], "the key held at touch-down is the takeover for the whole touch");
        assert_eq!(s.stats.takeovers, 1);
        s.fader(1, 20, false, false, Some(1), mixed, 2.6);
        // A page change while touched: rebound, nothing placed; the new touch must pick up again.
        s.fader(1, 128, true, false, Some(1), known, 3.0);
        assert_eq!(s.fader(1, 130, true, false, Some(1), known, 3.1).len(), 1);
        assert!(s.fader(1, 140, true, false, Some(2), known, 3.2).is_empty());
        s.fader(1, 140, false, false, Some(2), known, 3.3);
        s.fader(1, 140, true, false, Some(2), Some(SlotHint { single: true, value: Some(0.9) }), 3.4);
        assert!(s.fader(1, 150, true, false, Some(2), Some(SlotHint { single: true, value: Some(0.9) }), 3.5).is_empty(), "not picked up yet: no jump");
        assert_eq!(s.stats.positions, 5);
    }
}

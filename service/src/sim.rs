//! A simulated keypad: plays a script of key events on a schedule and prints LED writes. Lets the
//! plugin be exercised on a machine without the NX-K, and drives the bench.
//!
//! Script syntax: comma-separated steps, each `<key>:<down|up|tap>[@<ms>]`, where `@<ms>` is the
//! time since the previous step (default 100). A `tap` is down then up 60 ms later. Example:
//! `Record:down,5:tap@50,Record:up@300,Enter:tap`. KB-18 adds the rotaries: `rot<n>:<delta>` turns
//! encoder n (1..4) by a signed number of detents and `btn<n>:<down|up|tap>` pushes it. KB-20 adds the
//! M-Touch parameter strips as the decoder would report them: `strip<n>:t<v>` a report with the finger down
//! at position v (0..255; the first one after a lift is the touch-down), `strip<n>:m<v>` a further report
//! while touched (a move), `strip<n>:lift` or `strip<n>:l<v>` the lift report (resting value v, default the
//! last one), and `skey<n>:<down|up|tap>` the strip's own capacitive key (fine / takeover).

use crate::mtouch;
use crate::nxk::usb::Surface;
use crate::nxk::{self, Event};
use anyhow::{anyhow, Result};
use std::collections::VecDeque;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Key { name: &'static str, down: bool },
    Rotate { wheel: u8, delta: i32 },
    Push { wheel: u8, down: bool },
    /// KB-20: a base-channel fader report of strip n (1..4): `touch` false is the lift report.
    Strip { n: u8, value: u8, touch: bool },
    /// KB-20: the strip's own key.
    StripKey { n: u8, down: bool },
}

#[derive(Debug, Clone)]
pub struct Step {
    pub at_ms: u64,
    pub action: Action,
}

fn key_name(s: &str) -> Result<&'static str> {
    nxk::buttons().map(|(n, _)| n).find(|n| n.eq_ignore_ascii_case(s)).ok_or_else(|| anyhow!("unknown NX-K key '{s}'"))
}

/// Expands a script into absolute-time steps.
pub fn parse_script(script: &str) -> Result<Vec<Step>> {
    let mut steps = Vec::new();
    let mut last_strip_lift_default: Vec<usize> = Vec::new();
    let mut t = 0u64;
    for part in script.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (spec, delay) = match part.split_once('@') {
            Some((s, d)) => (s, d.trim().parse::<u64>().map_err(|_| anyhow!("bad delay in '{part}'"))?),
            None => (part, 100),
        };
        let (key, action) = spec.rsplit_once(':').ok_or_else(|| anyhow!("step '{part}' needs <key>:<down|up|tap>"))?;
        let key = key.trim();
        t += delay;
        let lower = key.to_ascii_lowercase();
        if let Some(n) = lower.strip_prefix("rot").and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=4).contains(n)) {
            let delta: i32 = action.trim().parse().map_err(|_| anyhow!("rot{n} needs a signed delta in '{part}'"))?;
            if delta == 0 || delta.abs() > 4096 {
                return Err(anyhow!("rot{n}: delta must be non-zero and within +-4096 in '{part}'"));
            }
            steps.push(Step { at_ms: t, action: Action::Rotate { wheel: n, delta } });
            continue;
        }
        if let Some(n) = lower.strip_prefix("strip").and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=4).contains(n)) {
            let a = action.trim().to_ascii_lowercase();
            let value = |rest: &str| rest.parse::<u8>().map_err(|_| anyhow!("strip{n}: a position 0..255 is needed in '{part}'"));
            if a == "lift" {
                steps.push(Step { at_ms: t, action: Action::Strip { n, value: 0, touch: false } });  // 0 = the last value (resolved at play time)
                last_strip_lift_default.push(steps.len() - 1);
            } else if let Some(v) = a.strip_prefix('t') {
                steps.push(Step { at_ms: t, action: Action::Strip { n, value: value(v)?, touch: true } });
            } else if let Some(v) = a.strip_prefix('m') {
                steps.push(Step { at_ms: t, action: Action::Strip { n, value: value(v)?, touch: true } });
            } else if let Some(v) = a.strip_prefix('l') {
                steps.push(Step { at_ms: t, action: Action::Strip { n, value: value(v)?, touch: false } });
            } else {
                return Err(anyhow!("strip{n}: expected t<v>, m<v>, lift or l<v> in '{part}'"));
            }
            continue;
        }
        if let Some(n) = lower.strip_prefix("skey").and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=4).contains(n)) {
            match action.trim() {
                "down" => steps.push(Step { at_ms: t, action: Action::StripKey { n, down: true } }),
                "up" => steps.push(Step { at_ms: t, action: Action::StripKey { n, down: false } }),
                "tap" => {
                    steps.push(Step { at_ms: t, action: Action::StripKey { n, down: true } });
                    steps.push(Step { at_ms: t + 60, action: Action::StripKey { n, down: false } });
                }
                other => return Err(anyhow!("unknown action '{other}' in '{part}'")),
            }
            continue;
        }
        if let Some(n) = lower.strip_prefix("btn").and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=4).contains(n)) {
            match action.trim() {
                "down" => steps.push(Step { at_ms: t, action: Action::Push { wheel: n, down: true } }),
                "up" => steps.push(Step { at_ms: t, action: Action::Push { wheel: n, down: false } }),
                "tap" => {
                    steps.push(Step { at_ms: t, action: Action::Push { wheel: n, down: true } });
                    steps.push(Step { at_ms: t + 60, action: Action::Push { wheel: n, down: false } });
                }
                other => return Err(anyhow!("unknown action '{other}' in '{part}'")),
            }
            continue;
        }
        let name = key_name(key)?;
        match action.trim() {
            "down" => steps.push(Step { at_ms: t, action: Action::Key { name, down: true } }),
            "up" => steps.push(Step { at_ms: t, action: Action::Key { name, down: false } }),
            "tap" => {
                steps.push(Step { at_ms: t, action: Action::Key { name, down: true } });
                steps.push(Step { at_ms: t + 60, action: Action::Key { name, down: false } });
            }
            other => return Err(anyhow!("unknown action '{other}' in '{part}'")),
        }
    }
    // A bare `lift` rests at the strip's last scripted position.
    for i in last_strip_lift_default {
        if let Action::Strip { n, .. } = steps[i].action {
            let last = steps[..i].iter().rev().find_map(|s| match s.action { Action::Strip { n: m, value, touch: true } if m == n => Some(value), _ => None }).unwrap_or(0);
            steps[i].action = Action::Strip { n, value: last, touch: false };
        }
    }
    steps.sort_by_key(|s| s.at_ms);
    Ok(steps)
}

/// The M-Touch ids the strips converter recognises (`VALUE BASE CHANNEL n`, `BASE CHANNEL n`).
fn strip_fader_id(n: u8) -> u16 { 0x6102 + 0x10 * n as u16 }
fn strip_key_id(n: u8) -> u16 { 0x6101 + 0x10 * n as u16 }

pub struct SimKeypad {
    queue: VecDeque<Step>,
    strip_out: Vec<mtouch::Event>,
    started: Instant,
    loop_script: Option<Vec<Step>>,
    loop_len_ms: u64,
    offset_ms: u64,
    pub led_writes: Vec<(u16, u16)>,
    pub echo_leds: bool,
    pub done: bool,
}

impl SimKeypad {
    pub fn new(steps: Vec<Step>, repeat: bool, echo_leds: bool) -> SimKeypad {
        let loop_len_ms = steps.last().map(|s| s.at_ms + 200).unwrap_or(0);
        SimKeypad {
            queue: steps.iter().cloned().collect(),
            strip_out: Vec::new(),
            started: Instant::now(),
            loop_script: if repeat { Some(steps) } else { None },
            loop_len_ms,
            offset_ms: 0,
            led_writes: Vec::new(),
            echo_leds,
            done: false,
        }
    }
}

impl Surface for SimKeypad {
    fn poll(&mut self) -> Result<Vec<Event>> {
        let now_ms = self.started.elapsed().as_millis() as u64;
        let mut out = Vec::new();
        while let Some(s) = self.queue.front() {
            if s.at_ms + self.offset_ms > now_ms {
                break;
            }
            let s = self.queue.pop_front().unwrap();
            out.push(match s.action {
                Action::Key { name, down } => {
                    let id = nxk::button_id(name).unwrap_or(0);
                    if down { Event::KeyDown { name, id } } else { Event::KeyUp { name, id } }
                }
                Action::Rotate { wheel, delta } => Event::Rotate { wheel, delta, id: 0 },
                Action::Push { wheel, down } => if down { Event::PressDown { wheel, id: 0 } } else { Event::PressUp { wheel, id: 0 } },
                Action::Strip { n, value, touch } => {
                    let id = strip_fader_id(n);
                    self.strip_out.push(mtouch::Event::Fader { id, name: mtouch::name_of(mtouch::Model::MTouch, id).unwrap_or("?"), value, touch });
                    continue;
                }
                Action::StripKey { n, down } => {
                    let id = strip_key_id(n);
                    let name = mtouch::name_of(mtouch::Model::MTouch, id).unwrap_or("?");
                    self.strip_out.push(if down { mtouch::Event::KeyDown { id, name } } else { mtouch::Event::KeyUp { id, name } });
                    continue;
                }
            });
        }
        if self.queue.is_empty() {
            match &self.loop_script {
                Some(script) if self.loop_len_ms > 0 => {
                    self.offset_ms += self.loop_len_ms;
                    self.queue = script.iter().cloned().collect();
                }
                _ => self.done = true,
            }
        }
        Ok(out)
    }

    fn led(&mut self, id: u16, value: u16) {
        if self.echo_leds {
            let name = nxk::buttons().find(|(_, i)| *i == id).map(|(n, _)| n).unwrap_or("encoder");
            println!("led {name} ({id:04x}) <- {}", match value {
                nxk::LED_OFF => "off".to_string(),
                nxk::LED_ON => "on".to_string(),
                nxk::LED_BLINK => "blink".to_string(),
                v => format!("{v:04x}"),
            });
        }
        self.led_writes.push((id, value));
    }

    fn name(&self) -> String {
        "simulated NX-K (+ M-Touch strips)".into()
    }

    fn done(&self) -> bool {
        self.done
    }

    fn poll_strips(&mut self) -> Result<Vec<mtouch::Event>> {
        Ok(std::mem::take(&mut self.strip_out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_expand_in_order() {
        let s = parse_script("Record:down,5:tap@50,Record:up@300,enter:tap").unwrap();
        let names: Vec<(u64, Action)> = s.iter().map(|x| (x.at_ms, x.action.clone())).collect();
        assert_eq!(names, vec![(100, Action::Key { name: "Record", down: true }), (150, Action::Key { name: "5", down: true }), (210, Action::Key { name: "5", down: false }), (450, Action::Key { name: "Record", down: false }), (550, Action::Key { name: "Enter", down: true }), (610, Action::Key { name: "Enter", down: false })]);
        assert!(parse_script("Bogus:tap").is_err());
        assert!(parse_script("Record:wiggle").is_err());
        // KB-18: rotaries.
        let r = parse_script("rot1:+3,rot2:-2@10,btn1:tap@20,Rot4:1").unwrap();
        let acts: Vec<(u64, Action)> = r.iter().map(|x| (x.at_ms, x.action.clone())).collect();
        assert_eq!(acts, vec![(100, Action::Rotate { wheel: 1, delta: 3 }), (110, Action::Rotate { wheel: 2, delta: -2 }), (130, Action::Push { wheel: 1, down: true }), (190, Action::Push { wheel: 1, down: false }), (230, Action::Rotate { wheel: 4, delta: 1 })]);
        assert!(parse_script("rot5:1").is_err());
        assert!(parse_script("rot1:0").is_err());
        assert!(parse_script("btn2:wiggle").is_err());
        // KB-20: strips. A bare lift rests at the last scripted position.
        let s = parse_script("strip1:t100,strip1:m120@10,skey1:down@5,strip1:m130@5,skey1:up,strip1:lift@20,strip2:t0,strip2:l5").unwrap();
        let acts: Vec<(u64, Action)> = s.iter().map(|x| (x.at_ms, x.action.clone())).collect();
        assert_eq!(acts, vec![
            (100, Action::Strip { n: 1, value: 100, touch: true }), (110, Action::Strip { n: 1, value: 120, touch: true }), (115, Action::StripKey { n: 1, down: true }),
            (120, Action::Strip { n: 1, value: 130, touch: true }), (220, Action::StripKey { n: 1, down: false }), (240, Action::Strip { n: 1, value: 130, touch: false }),
            (340, Action::Strip { n: 2, value: 0, touch: true }), (440, Action::Strip { n: 2, value: 5, touch: false })]);
        assert!(parse_script("strip5:t1").is_err());
        assert!(parse_script("strip1:t300").is_err());
        assert!(parse_script("strip1:x1").is_err());
        assert!(parse_script("skey1:wiggle").is_err());
    }

    #[test]
    fn strip_steps_come_out_of_poll_strips_as_mtouch_events() {
        let steps = parse_script("strip1:t100@0,skey3:down@0").unwrap();
        let mut k = SimKeypad::new(steps, false, false);
        assert!(k.poll().unwrap().is_empty(), "strips are not NX-K events");
        let ev = k.poll_strips().unwrap();
        assert_eq!(ev, vec![mtouch::Event::Fader { id: 0x6112, name: "VALUE BASE CHANNEL 1", value: 100, touch: true }, mtouch::Event::KeyDown { id: 0x6131, name: "BASE CHANNEL 3" }]);
        assert!(k.poll_strips().unwrap().is_empty());
    }
}

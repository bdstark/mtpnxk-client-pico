//! A simulated keypad: plays a script of key events on a schedule and prints LED writes. Lets the
//! plugin be exercised on a machine without the NX-K, and drives the bench.
//!
//! Script syntax: comma-separated steps, each `<key>:<down|up|tap>[@<ms>]`, where `@<ms>` is the
//! time since the previous step (default 100). A `tap` is down then up 60 ms later. Example:
//! `Record:down,5:tap@50,Record:up@300,Enter:tap`.

use crate::nxk::usb::Surface;
use crate::nxk::{self, Event};
use anyhow::{anyhow, Result};
use std::collections::VecDeque;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct Step {
    pub at_ms: u64,
    pub name: &'static str,
    pub down: bool,
}

fn key_name(s: &str) -> Result<&'static str> {
    nxk::buttons().map(|(n, _)| n).find(|n| n.eq_ignore_ascii_case(s)).ok_or_else(|| anyhow!("unknown NX-K key '{s}'"))
}

/// Expands a script into absolute-time steps.
pub fn parse_script(script: &str) -> Result<Vec<Step>> {
    let mut steps = Vec::new();
    let mut t = 0u64;
    for part in script.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (spec, delay) = match part.split_once('@') {
            Some((s, d)) => (s, d.trim().parse::<u64>().map_err(|_| anyhow!("bad delay in '{part}'"))?),
            None => (part, 100),
        };
        let (key, action) = spec.rsplit_once(':').ok_or_else(|| anyhow!("step '{part}' needs <key>:<down|up|tap>"))?;
        let name = key_name(key.trim())?;
        t += delay;
        match action.trim() {
            "down" => steps.push(Step { at_ms: t, name, down: true }),
            "up" => steps.push(Step { at_ms: t, name, down: false }),
            "tap" => {
                steps.push(Step { at_ms: t, name, down: true });
                steps.push(Step { at_ms: t + 60, name, down: false });
            }
            other => return Err(anyhow!("unknown action '{other}' in '{part}'")),
        }
    }
    steps.sort_by_key(|s| s.at_ms);
    Ok(steps)
}

pub struct SimKeypad {
    queue: VecDeque<Step>,
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
            let id = nxk::button_id(s.name).unwrap_or(0);
            out.push(if s.down { Event::KeyDown { name: s.name, id } } else { Event::KeyUp { name: s.name, id } });
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
        "simulated NX-K".into()
    }

    fn done(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_expand_in_order() {
        let s = parse_script("Record:down,5:tap@50,Record:up@300,enter:tap").unwrap();
        let names: Vec<(u64, &str, bool)> = s.iter().map(|x| (x.at_ms, x.name, x.down)).collect();
        assert_eq!(names, vec![(100, "Record", true), (150, "5", true), (210, "5", false), (450, "Record", false), (550, "Enter", true), (610, "Enter", false)]);
        assert!(parse_script("Bogus:tap").is_err());
        assert!(parse_script("Record:wiggle").is_err());
    }
}

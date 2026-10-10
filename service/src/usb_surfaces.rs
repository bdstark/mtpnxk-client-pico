//! KB-20: the `run` surface: the NX-K keypad plus, when one is attached, an M-Touch whose base-channel
//! strips and keys are decoded for the strips converter. The M-Touch is optional and hot-pluggable: it is
//! opened when present (retried every two seconds while absent) and dropped when its reads fail, which the
//! loop turns into a release of every touched strip. The keypad stays the loop's lifeline: when it goes,
//! `poll` fails and the caller reopens both.
//!
//! Reports queued on the device before the first poll are delivered at open (KB-16 record): they are
//! discarded, never used to seed a touch or a level.

use crate::mtouch::{self, usb::UsbSurface, Event as MtEvent};
use crate::nxk::usb::{Surface, UsbKeypad};
use crate::nxk::Event;
use anyhow::Result;
use std::time::{Duration, Instant};

pub struct Combined {
    keypad: UsbKeypad,
    want_mtouch: bool,
    mtouch: Option<UsbSurface>,
    model: Option<mtouch::Model>,
    next_open: Instant,
    /// Reports within this window after the open are the device's queued history, not input.
    discard_until: Instant,
}

impl Combined {
    pub fn new(keypad: UsbKeypad, want_mtouch: bool) -> Combined {
        Combined { keypad, want_mtouch, mtouch: None, model: None, next_open: Instant::now(), discard_until: Instant::now() }
    }

    fn try_open(&mut self) {
        if !self.want_mtouch || self.mtouch.is_some() || Instant::now() < self.next_open {
            return;
        }
        self.next_open = Instant::now() + Duration::from_secs(2);
        match UsbSurface::open_model(mtouch::Model::MTouch) {
            Ok(dev) => {
                eprintln!("strips: {} open; its four base-channel strips are parameter strips", dev.name());
                self.model = Some(dev.model());
                self.mtouch = Some(dev);
                self.discard_until = Instant::now() + Duration::from_millis(250);
            }
            Err(_) => {
                // Absent is normal (the NX-K alone); the next attempt is in two seconds.
            }
        }
    }
}

impl Surface for Combined {
    fn poll(&mut self) -> Result<Vec<Event>> {
        self.keypad.poll()
    }

    fn led(&mut self, id: u16, value: u16) {
        self.keypad.led(id, value)
    }

    fn name(&self) -> String {
        match &self.mtouch {
            Some(m) => format!("{} + {}", self.keypad.name(), m.name()),
            None => self.keypad.name(),
        }
    }

    fn poll_strips(&mut self) -> Result<Vec<MtEvent>> {
        self.try_open();
        let Some(dev) = self.mtouch.as_mut() else { return Ok(Vec::new()) };
        let packets = match dev.poll() {
            Ok(p) => p,
            Err(e) => {
                self.mtouch = None;
                self.next_open = Instant::now() + Duration::from_secs(1);
                return Err(e);
            }
        };
        let model = self.model.unwrap_or(mtouch::Model::MTouch);
        if Instant::now() < self.discard_until {
            // Whatever was queued before the first reads is history, not input (KB-16: one packet held seven
            // concatenated base-fader reports at open).
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for p in packets {
            if p.is_empty() {
                continue;
            }
            for r in mtouch::decode(&p) {
                out.extend(mtouch::events(model, &r));
            }
        }
        Ok(out)
    }
}

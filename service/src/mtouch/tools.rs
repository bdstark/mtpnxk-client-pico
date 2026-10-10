//! Operator tools for the KB-16 hardware qualification: `mtouch-listen` prints every decoded
//! event with its raw packet (the format of mtouch-cli's `listen`) and `mtouch-led-test` walks
//! every output of the model so a person can confirm it by eye. Procedure and what to record:
//! `docs/mtouch-protocol-reuse.md`.

use super::usb::UsbSurface;
use super::{bar_level, controls, decode, events, fader_bar, name_of, page_display, BarColour, Event, LedKey, Model, Output, Report};
use anyhow::Result;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Opens the requested model, or the first M-Touch / M-Play found.
pub fn open(model: Option<Model>) -> Result<UsbSurface> {
    match model {
        Some(m) => UsbSurface::open_model(m),
        None => UsbSurface::open_any(),
    }
}

fn event_line(ts: f64, ev: &Event, raw: &str) -> String {
    let t = format!("{ts:9.3}");
    match ev {
        Event::KeyDown { id, name } => format!("{t}  KEY   0x{id:04X} {name} state=1 (press)  raw={raw}"),
        Event::KeyUp { id, name } => format!("{t}  KEY   0x{id:04X} {name} state=0 (release)  raw={raw}"),
        Event::Fader { id, name, value, touch } => format!("{t}  FADER 0x{id:04X} {name} value={value} (0x{value:02x}) touch={}{}  raw={raw}", *touch as u8, if *touch { "" } else { " (lift)" }),
        Event::Pressure { id, name, value, touch } => format!("{t}  PRESS 0x{id:04X} {name} value={value} (0x{value:02x}) touch={}{}  raw={raw}", *touch as u8, if *touch { "" } else { " (release)" }),
    }
}

/// Reads for `seconds` (forever if 0), printing one line per event.
pub fn listen(model: Option<Model>, seconds: f64) -> Result<()> {
    let mut dev = open(model)?;
    let model = dev.model();
    println!("{} ({:04x}:{:04x}) open; listening on EP 0x82 for {} ... (touch faders / press keys)", dev.name(), super::VID, model.pid(), if seconds > 0.0 { format!("{seconds} s") } else { "ever".into() });
    let started = Instant::now();
    let mut by_type: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut idle = 0u64;
    let mut events_n = 0u64;
    let mut last_report: Option<Instant> = None;
    let mut interval = (f64::MAX, 0.0f64);
    loop {
        let packets = match dev.poll() {
            Ok(p) => p,
            Err(e) => {
                println!("{:9.3}  {e}", started.elapsed().as_secs_f64());
                break;
            }
        };
        let now = Instant::now();
        let ts = started.elapsed().as_secs_f64();
        for p in packets {
            if p.is_empty() {
                idle += 1;
                continue;
            }
            if let Some(prev) = last_report {
                let d = now.duration_since(prev).as_secs_f64() * 1000.0;
                interval = (interval.0.min(d), interval.1.max(d));
            }
            last_report = Some(now);
            let raw = hex::encode(&p);
            for r in decode(&p) {
                *by_type.entry(r.kind()).or_default() += 1;
                let evs = events(model, &r);
                if evs.is_empty() {
                    match &r {
                        Report::Unknown { bytes } => println!("{ts:9.3}  RAW   len={} {}", bytes.len(), hex::encode(bytes)),
                        Report::KeyBank { base, stride, .. } => println!("{ts:9.3}  KBANK base=0x{base:04X} stride=0x{stride:02x} (no change flagged)  raw={raw}"),
                        Report::AnalogBank { base, stride, units } => {
                            let held: Vec<String> = units.iter().filter(|u| u.touch).map(|u| format!("0x{:04X}={}", u.id, u.value)).collect();
                            println!("{ts:9.3}  ABANK base=0x{base:04X} stride=0x{stride:02x} (no change flagged; touched: {})  raw={raw}", held.join(" "))
                        }
                        _ => {}
                    }
                }
                for ev in evs {
                    events_n += 1;
                    println!("{}", event_line(ts, &ev, &raw));
                }
            }
        }
        if seconds > 0.0 && started.elapsed().as_secs_f64() >= seconds {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let secs = started.elapsed().as_secs_f64();
    println!("--- {:.1} s: {events_n} events; reports by type: {}; idle polls={idle} ({:.1}/s)", secs, if by_type.is_empty() { "none".to_string() } else { by_type.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ") }, idle as f64 / secs.max(0.001));
    if interval.1 > 0.0 {
        println!("--- report interval: min {:.1} ms, max {:.1} ms (between non-empty packets)", interval.0, interval.1);
    }
    Ok(())
}

/// The documented colour of a key's LED: blue for K keys except Beat and Pause (red) and Next
/// (green); V keys are red + green; L indicators are red (protocol.md 3.3).
fn key_colour(id: u16, kind: char) -> &'static str {
    match (kind, id) {
        ('K', 0x5504) | ('K', 0x5512) => "red",
        ('K', 0x5513) => "green",
        ('K', _) => "blue",
        ('L', _) => "red",
        _ => "red",
    }
}

fn led_for(colour: &str) -> LedKey {
    match colour {
        "red" => LedKey::off().red(),
        "green" => LedKey::off().green(),
        _ => LedKey::off().blue(),
    }
}

/// A deterministic walk over every output of the model, each step held `hold`, printing what is
/// sent so the operator can confirm it by eye.
pub fn led_test(model: Option<Model>, hold: Duration) -> Result<()> {
    let dev = open(model)?;
    let model = dev.model();
    println!("{}: LED test, {} ms per step. Confirm each line by eye.", dev.name(), hold.as_millis());
    let step = |what: String, out: Output| {
        println!("  {what}  -> bRequest 0x{:02x} wIndex 0x{:04X} wValue 0x{:04X} data {}", out.request(), out.index(), out.value(), if out.data().is_empty() { "-".to_string() } else { hex::encode(out.data()) });
        dev.send(out);
        std::thread::sleep(hold);
    };
    for c in controls(model).iter().filter(|c| matches!(c.kind, 'K' | 'V' | 'L')) {
        let colour = key_colour(c.id, c.kind);
        let label = format!("0x{:04X} {} [{}]", c.id, c.name, c.kind);
        if c.kind == 'V' {
            step(format!("{label}: red"), Output::LedKey { id: c.id, value: LedKey::off().red().value() });
            step(format!("{label}: green"), Output::LedKey { id: c.id, value: LedKey::off().green().value() });
            step(format!("{label}: fast blink red"), Output::LedKey { id: c.id, value: LedKey::off().red().fast().value() });
        } else {
            step(format!("{label}: {colour}"), Output::LedKey { id: c.id, value: led_for(colour).value() });
            step(format!("{label}: fast blink {colour}"), Output::LedKey { id: c.id, value: led_for(colour).fast().value() });
        }
        step(format!("{label}: off"), Output::LedKey { id: c.id, value: LedKey::off().value() });
    }
    for c in controls(model).iter().filter(|c| c.kind == 'F') {
        let label = format!("0x{:04X} {} [F]", c.id, c.name);
        let blue_only = model == Model::MTouch && (c.id & 0xFF00) == 0x6100;
        let colour = if blue_only { BarColour::Blue } else { BarColour::Red };
        for n in 1..=10u8 {
            step(format!("{label}: {n} of 10 lit from the bottom, {colour:?}{}", if blue_only { " (blue-only bar)" } else { "" }), Output::FaderBar { id: c.id, data: bar_level(n, colour, false) });
        }
        if blue_only {
            step(format!("{label}: bottom 5 blue (green/red lanes have no effect here)"), Output::FaderBar { id: c.id, data: fader_bar(0, 0, 0x01F, (false, false, false)) });
            step(format!("{label}: all 10 blue, blinking"), Output::FaderBar { id: c.id, data: bar_level(10, BarColour::Blue, true) });
        } else {
            step(format!("{label}: bottom 5 green, top 5 red"), Output::FaderBar { id: c.id, data: fader_bar(0x01F, 0x3E0, 0, (false, false, false)) });
            step(format!("{label}: all 10 red, blinking"), Output::FaderBar { id: c.id, data: bar_level(10, BarColour::Red, true) });
        }
        step(format!("{label}: off"), Output::FaderBar { id: c.id, data: bar_level(0, BarColour::Off, false) });
    }
    for id in model.displays() {
        let label = format!("0x{id:04X} {} [D]", name_of(model, *id).unwrap_or("?"));
        for n in [Some(1u16), Some(42), Some(999), None] {
            step(format!("{label}: {}", n.map(|n| n.to_string()).unwrap_or_else(|| "blank".into())), Output::PageDisplay { id: *id, data: page_display(n) });
        }
    }
    println!("done: every K/V/L key, every F bar and every display of the {} was written", model.name());
    // Give the writer thread time to drain the last request before the interface is released.
    std::thread::sleep(Duration::from_millis(100));
    Ok(())
}

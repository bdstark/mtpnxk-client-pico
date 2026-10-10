//! mtpnxk: bridges an Obsidian/Elation NX-K keypad to the mtpnxk_surface plugin inside grandMA3
//! onPC over an authenticated UDP link. Protocol and design: docs/surface-protocol.md.

mod auth;
mod leds;
mod link;
mod mtouch;
mod nxk;
mod protocol;
mod sim;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use nxk::usb::Surface;
use nxk::Event;
use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(name = "mtpnxk", version, about = "NX-K keypad to grandMA3 onPC surface plugin")]
struct Cli {
    /// Plugin address (host:port); the plugin binds 127.0.0.1:9810 by default.
    #[arg(long, default_value = "127.0.0.1:9810", global = true)]
    plugin: String,
    /// Pairing key: 64 hex characters (the plugin's key=...). Or set MTPNXK_KEY.
    #[arg(long, env = "MTPNXK_KEY", global = true, hide_env_values = true)]
    key: Option<String>,
    /// Read the pairing key from this file instead.
    #[arg(long, global = true)]
    key_file: Option<std::path::PathBuf>,
    /// Surface id reported to the plugin.
    #[arg(long, global = true)]
    id: Option<String>,
    /// Print every link event (pairing, refusals, losses).
    #[arg(long, global = true)]
    verbose: bool,
    /// KB-19: the first encoder slot the four rotaries mean (1 = slots 1-4, the console's default page
    /// layout; 2 = slots 2-5 reaches a fifth pool slot). An explicit window, never a silent discard; a slot
    /// the bound page does not have is refused by the plugin with the reason.
    #[arg(long, default_value_t = 1, global = true, value_parser = clap::value_parser!(u8).range(1..=5))]
    rotary_slots: u8,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Drive the USB keypad.
    Run,
    /// Play a key script without hardware (see src/sim.rs for the syntax); prints LED writes.
    Sim {
        #[arg(long, default_value = "Record:down,5:tap@50,Record:up@300,Enter:tap,Clear:tap@200")]
        script: String,
        #[arg(long)]
        repeat: bool,
        /// Seconds to run before exiting (0 = until the script ends, or forever with --repeat).
        #[arg(long, default_value_t = 0.0)]
        seconds: f64,
        /// Exit without saying bye (simulates a pulled cable; the plugin's lease must clean up).
        #[arg(long)]
        abandon: bool,
    },
    /// Latency measurement: taps of '5' (and a Clear every 10) at a rate; reports percentiles.
    /// Start the plugin with `bench` to get press-to-effect timings as well.
    Bench {
        #[arg(long, default_value_t = 200)]
        taps: u32,
        /// Taps per second.
        #[arg(long, default_value_t = 20.0)]
        rate: f64,
    },
    /// List USB devices.
    List,
    /// KB-16: print every report of an M-Touch or M-Play (decoded, with the raw packet) and a
    /// summary on exit. Not wired into the link; the operator's hardware qualification tool.
    MtouchListen {
        /// Seconds to listen (0 = until the device goes away or the process is killed).
        #[arg(long, default_value_t = 30.0)]
        seconds: f64,
        /// Product id to open: f808 (M-Touch) or f80c (M-Play); default: the first of either.
        #[arg(long)]
        pid: Option<String>,
    },
    /// KB-16: walk every LED key, fader bar and page display of an M-Touch or M-Play with a
    /// deterministic sequence, printing each write so an operator can confirm it by eye.
    MtouchLedTest {
        #[arg(long)]
        pid: Option<String>,
        /// Milliseconds each step is held.
        #[arg(long, default_value_t = 300)]
        hold_ms: u64,
    },
    /// Print a fresh random pairing key.
    Keygen,
}

fn load_key(cli: &Cli) -> Result<auth::Key> {
    let hex = if let Some(p) = &cli.key_file {
        std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?
    } else if let Some(k) = &cli.key {
        k.clone()
    } else {
        return Err(anyhow!("a pairing key is required: --key <64 hex>, --key-file <path> or MTPNXK_KEY"));
    };
    auth::Key::from_hex(&hex).map_err(|e| anyhow!(e))
}

fn surface_id(cli: &Cli) -> String {
    cli.id.clone().unwrap_or_else(|| {
        let host = std::env::var("HOSTNAME").ok().or_else(|| std::env::var("COMPUTERNAME").ok()).unwrap_or_else(|| "host".into());
        format!("nxk-{host}")
    })
}

struct Loop {
    link: link::Link,
    leds: leds::Renderer,
    sock: UdpSocket,
    started: Instant,
    verbose: bool,
    log_seen: usize,
    bank_held: bool,
    bye_on_exit: bool,
    /// KB-19: the first encoder slot of the rotaries (`--rotary-slots`).
    rotary_base: u8,
}

impl Loop {
    fn new(cli: &Cli) -> Result<Loop> {
        let key = load_key(cli)?;
        let sock = UdpSocket::bind("0.0.0.0:0").context("binding a UDP socket")?;
        sock.connect(&cli.plugin).with_context(|| format!("resolving the plugin address {}", cli.plugin))?;
        sock.set_nonblocking(true)?;
        let cfg = link::Config { id: surface_id(cli), ..link::Config::default() };
        let started = Instant::now();
        if cli.rotary_slots != 1 {
            eprintln!("rotaries: explicit slot window {}-{} (--rotary-slots {})", cli.rotary_slots, cli.rotary_slots + 3, cli.rotary_slots);
        }
        Ok(Loop { link: link::Link::new(cfg, key, 0.0), leds: leds::Renderer::new(), sock, started, verbose: cli.verbose, log_seen: 0, bank_held: false, bye_on_exit: true, rotary_base: cli.rotary_slots })
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// One iteration: surface events, datagrams, link timers, sends, LEDs.
    fn step(&mut self, surface: &mut dyn Surface) -> Result<()> {
        let now = self.now();
        for ev in surface.poll()? {
            match ev {
                Event::KeyDown { name, .. } => {
                    if name == "Bank" {
                        self.bank_held = true;
                    }
                    self.leds.key_event(name, true, now);
                    self.link.key_event(name, true, now);
                }
                Event::KeyUp { name, .. } => {
                    if name == "Bank" {
                        self.bank_held = false;
                    }
                    self.link.key_event(name, false, now);
                }
                // KB-18: the four rotaries are four encoder slots of the bound display (slots
                // `--rotary-slots`..+3, 1-4 by default); Bank held is the explicit fine modifier (KB-19: a
                // tenth of a click on the console); a push is a button boundary (refused by the console
                // backend: not qualified). Everything travels as `ctl` events through the link's admission.
                // No acceleration is applied anywhere in the service: the device's delta is the detent count.
                Event::Rotate { wheel, delta, .. } => self.link.control_event("nxk", ROTARIES[(wheel.clamp(1, 4) - 1) as usize], link_target(wheel, self.rotary_base), link::CtlKind::Rel { dx: delta, fine: self.bank_held }, now),
                Event::PressDown { wheel, .. } => self.link.control_event("nxk", ROTARIES[(wheel.clamp(1, 4) - 1) as usize], link_target(wheel, self.rotary_base), link::CtlKind::Btn { down: true }, now),
                Event::PressUp { wheel, .. } => self.link.control_event("nxk", ROTARIES[(wheel.clamp(1, 4) - 1) as usize], link_target(wheel, self.rotary_base), link::CtlKind::Btn { down: false }, now),
                Event::Unknown { bytes } => {
                    if self.verbose {
                        eprintln!("nxk: unknown packet {bytes:02x?}");
                    }
                }
            }
        }
        let mut buf = [0u8; 2048];
        loop {
            match self.sock.recv(&mut buf) {
                Ok(n) => self.link.receive(&buf[..n], self.now()),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::ConnectionReset || e.kind() == ErrorKind::ConnectionRefused => break,
                Err(e) => return Err(e).context("receiving"),
            }
        }
        self.link.tick(self.now());
        for d in self.link.take_outgoing() {
            if let Err(e) = self.sock.send(&d) {
                if e.kind() != ErrorKind::ConnectionRefused && e.kind() != ErrorKind::ConnectionReset {
                    return Err(e).context("sending");
                }
            }
        }
        let now = self.now();
        let view = self.link.console(now);
        let held: BTreeSet<&'static str> = self.link.held().clone();
        for (id, v) in self.leds.render(&view, &held, now) {
            surface.led(id, v);
        }
        if self.verbose {
            while self.log_seen < self.link.log.len() {
                eprintln!("link: {}", self.link.log[self.log_seen]);
                self.log_seen += 1;
            }
        }
        Ok(())
    }

    fn run(&mut self, surface: &mut dyn Surface, until: impl Fn(&Loop, &dyn Surface) -> bool) -> Result<()> {
        eprintln!("mtpnxk {}: {} -> plugin at {}", env!("CARGO_PKG_VERSION"), surface.name(), self.sock.peer_addr().map(|a| a.to_string()).unwrap_or_default());
        loop {
            self.step(surface)?;
            if until(self, surface) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if self.bye_on_exit {
            self.link.bye();
            for d in self.link.take_outgoing() {
                let _ = self.sock.send(&d);
            }
        }
        Ok(())
    }
}

const ROTARIES: [&str; 4] = ["Rotary1", "Rotary2", "Rotary3", "Rotary4"];

/// Rotary `wheel` (1-4) means encoder slot `base + wheel - 1` of the bound display (KB-19 `--rotary-slots`).
fn link_target(wheel: u8, base: u8) -> protocol::CtlTarget {
    protocol::CtlTarget::Slot { slot: base.clamp(1, 5) + wheel.clamp(1, 4) - 1 }
}

#[cfg(test)]
mod rotary_tests {
    use super::*;
    #[test]
    fn rotaries_map_to_a_slot_window() {
        assert_eq!(link_target(1, 1), protocol::CtlTarget::Slot { slot: 1 });
        assert_eq!(link_target(4, 1), protocol::CtlTarget::Slot { slot: 4 });
        assert_eq!(link_target(1, 2), protocol::CtlTarget::Slot { slot: 2 });
        assert_eq!(link_target(4, 2), protocol::CtlTarget::Slot { slot: 5 });
        assert_eq!(link_target(9, 1), protocol::CtlTarget::Slot { slot: 4 }, "an out-of-range wheel clamps");
        assert_eq!(link_target(4, 9), protocol::CtlTarget::Slot { slot: 8 }, "the base is bounded so the slot stays within the plugin's 1..8");
    }
}

fn percentiles(label: &str, samples: &[f64]) {
    if samples.is_empty() {
        println!("{label}: no samples");
        return;
    }
    let mut s = samples.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| s[((s.len() - 1) as f64 * q).round() as usize];
    println!("{label}: n={} median={:.1} p95={:.1} p99={:.1} worst={:.1} ms", s.len(), p(0.5), p(0.95), p(0.99), s[s.len() - 1]);
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Keygen => {
            let k: [u8; 32] = rand::random();
            println!("{}", hex::encode(k));
            Ok(())
        }
        Cmd::List => {
            for line in nxk::usb::list()? {
                println!("{line}");
            }
            Ok(())
        }
        Cmd::MtouchListen { seconds, pid } => mtouch::tools::listen(mtouch_model(pid.as_deref())?, *seconds),
        Cmd::MtouchLedTest { pid, hold_ms } => mtouch::tools::led_test(mtouch_model(pid.as_deref())?, Duration::from_millis(*hold_ms)),
        Cmd::Run => {
            let mut lp = Loop::new(&cli)?;
            loop {
                let mut keypad = match nxk::usb::UsbKeypad::open() {
                    Ok(k) => k,
                    Err(e) => {
                        eprintln!("{e:#}; retrying in 2 s");
                        std::thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                };
                lp.leds.invalidate();
                let r = lp.run(&mut keypad, |_, _| false);
                eprintln!("keypad loop ended: {r:?}; reopening in 1 s");
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        Cmd::Sim { script, repeat, seconds, abandon } => {
            let steps = sim::parse_script(script)?;
            let mut keypad = sim::SimKeypad::new(steps, *repeat, true);
            let mut lp = Loop::new(&cli)?;
            let deadline = if *seconds > 0.0 { Some(Duration::from_secs_f64(*seconds)) } else { None };
            lp.bye_on_exit = !abandon;
            lp.run(&mut keypad, |lp, s| {
                let timed_out = deadline.map(|d| lp.started.elapsed() > d).unwrap_or(false);
                // After the script ends, give acks a moment to arrive.
                let finished = !repeat && s.done() && lp.started.elapsed() > Duration::from_millis(500) && lp.link.pending_count() == 0;
                timed_out || finished
            })?;
            print_summary(&lp.link);
            Ok(())
        }
        Cmd::Bench { taps, rate } => {
            let gap = (1000.0 / rate).max(1.0) as u64;
            let mut script = String::new();
            for i in 0..*taps {
                if i % 10 == 9 {
                    script.push_str(&format!("Clear:tap@{gap},"));
                } else {
                    script.push_str(&format!("5:tap@{gap},"));
                }
            }
            let steps = sim::parse_script(&script)?;
            let total = Duration::from_millis(gap * (*taps as u64 + 2) + 1500);
            let mut keypad = sim::SimKeypad::new(steps, false, false);
            let mut lp = Loop::new(&cli)?;
            lp.run(&mut keypad, |lp, _| lp.started.elapsed() > total)?;
            println!("bench: {} taps at {rate}/s against {}", taps, cli.plugin);
            print_summary(&lp.link);
            percentiles("ack round trip", &lp.link.rtt_ms);
            let effects: Vec<f64> = lp.link.effect_ms.iter().filter(|m| **m >= 0).map(|m| *m as f64).collect();
            let no_effect = lp.link.effect_ms.iter().filter(|m| **m < 0).count();
            if lp.link.effect_ms.is_empty() {
                println!("press-to-effect: no samples (start the plugin with `bench` to measure it)");
            } else {
                percentiles("press-to-effect (NUM taps, command line)", &effects);
                println!("press-to-effect: {no_effect} tap(s) showed no command-line change within the plugin's window");
            }
            Ok(())
        }
    }
}

/// `--pid f808|f80c` (with or without 0x) to a model; `None` means the first found.
fn mtouch_model(pid: Option<&str>) -> Result<Option<mtouch::Model>> {
    let Some(p) = pid else { return Ok(None) };
    let n = u16::from_str_radix(p.trim_start_matches("0x"), 16).with_context(|| format!("parsing --pid {p}"))?;
    mtouch::Model::from_pid(n).map(Some).ok_or_else(|| anyhow!("--pid {p}: not an M-Touch (f808) or M-Play (f80c)"))
}

fn print_summary(link: &link::Link) {
    let st = &link.stats;
    println!(
        "link: paired={} hellos={} events={} acked={} refused={} retransmitted={} lost={} superseded={} states={} link_downs={} rejected={} dropped_unpaired={}",
        link.session().is_some(),
        st.hellos,
        st.events,
        st.acked,
        st.refused,
        st.retransmitted,
        st.lost,
        st.superseded,
        st.states,
        st.link_downs,
        st.rejected,
        st.dropped_unpaired
    );
    println!(
        "control: events={} sent={} coalesced={} unbound={} aged={} stale={} unsupported={} refused={} lost_reported={} superseded={} overflow={} queued={} max_detent={}",
        st.ctl_events, st.ctl_sent, st.ctl_coalesced, st.ctl_unbound, st.ctl_aged, st.ctl_stale, st.ctl_unsupported, st.ctl_refused, st.ctl_lost_reported, st.ctl_superseded, st.ctl_overflow, link.queued_motion(), st.ctl_max_detent
    );
    if let Some(s) = link.session() {
        println!("session: sid={} plugin gen={} input={:?} control={} keys ok={} unsupported={:?}", s.sid, s.plugin_gen, s.input, if s.control { s.control_backend.clone().unwrap_or_else(|| "on".into()) } else { "off".into() }, s.keys.ok.len(), s.keys.unsupported);
    }
    for r in &link.refusals {
        println!("refused ev {}: [{}] {}", r.ev, r.code, r.why);
    }
}

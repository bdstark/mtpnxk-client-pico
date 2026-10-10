//! The surface side of the protocol (docs/surface-protocol.md): pairing, sequence numbers, event
//! ids with retransmission, heartbeats, the link watchdog and the console-state cache with its
//! freshness rules. Pure: no sockets, no clock. The caller feeds datagrams and the time in seconds
//! and sends what `take_outgoing()` returns.

use crate::auth::{self, Key, MAX_FROM_PLUGIN};
use crate::protocol::{CtlTarget, FromPlugin, KeyReport, ToPlugin, PROTOCOL_VERSION};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct Config {
    pub id: String,
    pub surface: String,
    pub fw: String,
    /// Seconds between hellos while unpaired.
    pub hello_interval: f64,
    /// No plugin packet for this long: link down, every state item unknown.
    pub watchdog: f64,
    /// No plugin packet for this long: back to unpaired, hello again.
    pub repair_after: f64,
    /// Retransmissions of an unacknowledged key event, and their spacing.
    pub retransmits: u32,
    pub retransmit_spacing: f64,
    /// No `state` for this long: console state unknown.
    pub state_stale: f64,
    /// KB-18: queued motion older than this is dropped, never sent late.
    pub motion_max_age: f64,
    /// KB-18: at most one motion packet per control per this interval; deltas merge in between.
    pub motion_min_interval: f64,
    /// KB-18: motion packets sent per tick.
    pub ctl_per_tick: usize,
    /// KB-18: a relative gesture without a touch ends after this idle time (the next delta is a new gesture).
    pub gesture_idle: f64,
    /// KB-18: queued motion entries per control; beyond it the oldest entry of that control is dropped.
    pub motion_per_control: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            id: "nxk".into(),
            surface: "nxk".into(),
            fw: env!("CARGO_PKG_VERSION").into(),
            hello_interval: 2.0,
            watchdog: 1.5,
            repair_after: 4.0,
            retransmits: 4,
            retransmit_spacing: 0.06, // measured ack round trip on onPC: median 35 ms, p99 50 ms
            state_stale: 1.5,
            motion_max_age: 0.25,
            motion_min_interval: 0.005,
            ctl_per_tick: 16,
            gesture_idle: 0.3,
            motion_per_control: 64,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub sent: u64,
    pub received: u64,
    pub rejected: u64,
    pub old_seq: u64,
    pub hellos: u64,
    pub welcomes: u64,
    pub events: u64,
    pub acked: u64,
    pub refused: u64,
    pub retransmitted: u64,
    pub lost: u64,
    pub dropped_unpaired: u64,
    /// Presses still awaiting an ack when their release went out: no longer retransmitted.
    pub superseded: u64,
    pub states: u64,
    pub deltas_ignored: u64,
    /// KB-17 context messages received, and how many moved the binding generation.
    pub contexts: u64,
    pub context_generations: u64,
    pub link_downs: u64,
    pub no_session: u64,
    /// KB-18 continuous control: events from the surface, packets sent, deltas merged before sending,
    /// events dropped for want of a binding generation, for age, for a moved generation, or because the
    /// plugin admits no control; refusals and the loss the plugin reported back; presses superseded by
    /// their release before an ack.
    pub ctl_events: u64,
    pub ctl_sent: u64,
    pub ctl_coalesced: u64,
    pub ctl_unbound: u64,
    pub ctl_aged: u64,
    pub ctl_stale: u64,
    pub ctl_unsupported: u64,
    pub ctl_refused: u64,
    pub ctl_lost_reported: u64,
    pub ctl_superseded: u64,
    /// Positions dropped because a control's motion queue overflowed (stateful targets keep every position).
    pub ctl_overflow: u64,
    /// KB-19: the largest |delta| of one rotary event seen (a device that accelerates reports more than one
    /// detent per report; the service adds nothing to it, so this is what reaches the console per event).
    pub ctl_max_detent: i32,
}

/// Fader functions whose value path matters (the same set as the console module's STATEFUL_FUNCTIONS):
/// a queued position of such a target is never replaced by a newer one here.
const STATEFUL_FUNCTIONS: [&str; 7] = ["x", "xa", "xb", "crossfade", "crossfadea", "crossfadeb", "temp"];

/// KB-18: what a surface control did. `Abs` and `Touch` are produced by the M-Touch strips once KB-20
/// wires them into the link; the transport treats them now (tests), the NX-K produces `Rel` and `Btn`.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CtlKind {
    /// Encoder motion in detents; `fine` is the surface's explicit fine-adjustment modifier.
    Rel { dx: i32, fine: bool },
    /// A strip or fader position, 0..1 of its travel. `takeover` (KB-20) is the surface's statement that the
    /// operator deliberately chose an absolute operation: the console backend then places the value even on a
    /// slot whose selected fixtures hold different values (otherwise refused `mixed-values`).
    Abs { v: f64, takeover: bool },
    Touch { down: bool },
    Btn { down: bool },
}

/// A touch or button event awaiting its acknowledgment (retransmitted like a key).
#[derive(Debug, Clone)]
struct CtlBoundary {
    k: &'static str,
    dev: String,
    c: String,
    es: u64,
    cg: Option<u64>,
    gs: u64,
    tgt: CtlTarget,
    d: u8,
}

/// Motion waiting to be sent: deltas of one control, generation and gesture merge into it.
#[derive(Debug, Clone)]
struct Motion {
    dev: String,
    c: String,
    tgt: CtlTarget,
    cg: u64,
    gs: u64,
    fine: bool,
    dx: i32,
    v: Option<f64>,
    takeover: bool,
    first_at: f64,
    events: u32,
}

#[derive(Debug, Clone)]
struct Gesture {
    id: u64,
    touching: bool,
    last_at: f64,
}

#[derive(Debug, Clone)]
enum PendingKind {
    Key { k: &'static str, d: u8 },
    Ctl(CtlBoundary),
}

#[derive(Debug, Clone)]
struct Pending {
    ev: u64,
    kind: PendingKind,
    first_sent: f64,
    next_at: f64,
    attempts: u32,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub sid: String,
    pub out_seq: u64,
    pub in_seq: u64,
    pub lease_ms: u64,
    pub hb_ms: u64,
    pub plugin_gen: String,
    pub keys: KeyReport,
    pub modules: BTreeMap<String, String>,
    pub input: Option<String>,
    /// KB-18: the plugin admits `ctl` events (control=fake|...); otherwise they are dropped here.
    pub control: bool,
    pub control_backend: Option<String>,
    next_hb: f64,
}

#[derive(Debug, Clone)]
pub enum Phase {
    Unpaired { next_hello: f64, nonce: Option<String> },
    Paired(Session),
}

/// The console state as last received, with everything needed to judge freshness.
#[derive(Debug, Clone, Default)]
pub struct ConsoleState {
    pub generation: String,
    pub epoch: u64,
    pub items: BTreeMap<String, Value>,
    pub full_seen: bool,
    pub last_state_at: f64,
}

/// The control context as last received (KB-17): the plugin's binding generation and the message body.
/// `known` false means the plugin had not observed every part; `generation` is then absent.
#[derive(Debug, Clone, Default)]
pub struct ConsoleContext {
    pub plugin_generation: String,
    pub epoch: u64,
    pub known: bool,
    pub generation: Option<u64>,
    pub body: BTreeMap<String, Value>,
    pub received_at: f64,
}

/// What a renderer sees: the items, or nothing when they must be shown as unknown.
#[derive(Debug, Clone)]
pub struct ConsoleView<'a> {
    pub fresh: bool,
    pub paired: bool,
    pub link_up: bool,
    pub items: Option<&'a BTreeMap<String, Value>>,
}

/// One refused event, for the log.
#[derive(Debug, Clone)]
pub struct Refusal {
    pub ev: u64,
    pub code: String,
    pub why: String,
}

pub struct Link {
    cfg: Config,
    key: Key,
    pub generation: u32,
    phase: Phase,
    ev_next: u64,
    pending: Vec<Pending>,
    /// Keys physically down at the surface (sent in hello and heartbeats).
    held: BTreeSet<&'static str>,
    outgoing: Vec<Vec<u8>>,
    pub stats: Stats,
    console: ConsoleState,
    /// KB-17: the last control context, kept as data; `None` until one arrived in this pairing.
    pub context: Option<ConsoleContext>,
    last_rx: f64,
    pub link_up: bool,
    pub plugin_held: Vec<String>,
    pub unsynced: Vec<String>,
    /// Ack round trips in milliseconds, and press-to-effect samples from the plugin's bench mode.
    pub rtt_ms: Vec<f64>,
    pub effect_ms: Vec<i64>,
    pub refusals: Vec<Refusal>,
    pub log: Vec<String>,
    /// KB-18: per-device event sequence (next value), gestures per control, queued motion.
    es_next: BTreeMap<String, u64>,
    gestures: BTreeMap<(String, String), Gesture>,
    gs_next: u64,
    motion: Vec<Motion>,
    last_flush: BTreeMap<(String, String), f64>,
}

fn nonce() -> String {
    format!("{:016x}", rand::random::<u64>())
}

impl Link {
    pub fn new(cfg: Config, key: Key, now: f64) -> Link {
        Link {
            cfg,
            key,
            generation: rand::random::<u32>(),
            phase: Phase::Unpaired { next_hello: now, nonce: None },
            ev_next: 1,
            pending: Vec::new(),
            held: BTreeSet::new(),
            outgoing: Vec::new(),
            stats: Stats::default(),
            console: ConsoleState::default(),
            context: None,
            last_rx: now,
            link_up: false,
            plugin_held: Vec::new(),
            unsynced: Vec::new(),
            rtt_ms: Vec::new(),
            effect_ms: Vec::new(),
            refusals: Vec::new(),
            log: Vec::new(),
            es_next: BTreeMap::new(),
            gestures: BTreeMap::new(),
            gs_next: 1,
            motion: Vec::new(),
            last_flush: BTreeMap::new(),
        }
    }

    /// KB-18: motion waiting to be sent (merged deltas and positions).
    pub fn queued_motion(&self) -> usize {
        self.motion.len()
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn session(&self) -> Option<&Session> {
        match &self.phase {
            Phase::Paired(s) => Some(s),
            _ => None,
        }
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn held(&self) -> &BTreeSet<&'static str> {
        &self.held
    }

    pub fn console(&self, now: f64) -> ConsoleView<'_> {
        let paired = self.session().is_some();
        let fresh = paired && self.link_up && self.console.full_seen && now - self.console.last_state_at <= self.cfg.state_stale;
        ConsoleView { fresh, paired, link_up: self.link_up, items: if fresh { Some(&self.console.items) } else { None } }
    }

    /// The control context (KB-17) as something a consumer may act on: only while paired with the link up,
    /// only a context received in this pairing within `state_stale`, and only one the plugin marked known.
    /// Anything else is `None`: a stale or previous-pairing context is never presented as current.
    pub fn context_view(&self, now: f64) -> Option<&ConsoleContext> {
        let fresh = self.session().is_some() && self.link_up;
        match &self.context {
            Some(c) if fresh && c.known && now - c.received_at <= self.cfg.state_stale => Some(c),
            _ => None,
        }
    }

    pub fn take_outgoing(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.outgoing)
    }

    fn note(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > 500 {
            self.log.remove(0);
        }
    }

    fn send_json(&mut self, json: String) {
        self.outgoing.push(auth::frame(&self.key, &json));
        self.stats.sent += 1;
    }

    fn send_hello(&mut self, now: f64) {
        let n = nonce();
        let held: Vec<&str> = self.held.iter().copied().collect();
        let json = serde_json::to_string(&ToPlugin::Hello {
            v: PROTOCOL_VERSION,
            id: &self.cfg.id,
            generation: self.generation,
            nonce: &n,
            surface: &self.cfg.surface,
            fw: &self.cfg.fw,
            held,
        })
        .expect("hello serialises");
        self.phase = Phase::Unpaired { next_hello: now + self.cfg.hello_interval, nonce: Some(n) };
        self.stats.hellos += 1;
        self.send_json(json);
    }

    /// Sends a session packet; returns false (and drops it) while unpaired.
    fn send_session(&mut self, build: impl FnOnce(&str, u64) -> String) -> bool {
        let (sid, seq) = match &mut self.phase {
            Phase::Paired(s) => {
                s.out_seq += 1;
                (s.sid.clone(), s.out_seq)
            }
            _ => return false,
        };
        let json = build(&sid, seq);
        self.send_json(json);
        true
    }

    fn send_pending(&mut self, p: &Pending) -> bool {
        let ev = p.ev;
        match &p.kind {
            PendingKind::Key { k, d } => {
                let (k, d) = (*k, *d);
                self.send_session(|sid, seq| serde_json::to_string(&ToPlugin::Key { sid, seq, ev, k, d }).expect("key serialises"))
            }
            PendingKind::Ctl(b) => {
                let b = b.clone();
                self.send_session(|sid, seq| {
                    serde_json::to_string(&ToPlugin::Ctl { sid, seq, ev, k: b.k, dev: &b.dev, c: &b.c, es: b.es, cg: b.cg, gs: Some(b.gs), tgt: b.tgt, dx: None, v: None, d: Some(b.d), fine: None, tk: None }).expect("ctl serialises")
                })
            }
        }
    }

    fn next_es(&mut self, dev: &str) -> u64 {
        let e = self.es_next.entry(dev.to_string()).or_insert(1);
        let v = *e;
        *e += 1;
        v
    }

    pub fn current_generation(&self, now: f64) -> Option<u64> {
        self.context_view(now).and_then(|c| c.generation)
    }

    /// KB-20: what the last context said about an encoder slot's programmer value, for the strips' absolute-mode
    /// pickup: `single` when the plugin reported one agreed value (`val` "value" with `abs`), the value as 0..1 of
    /// the travel (the programmer's `absolute` is a percentage of the range at every readout). The context is
    /// refreshed on a generation change and at least once a second, so this is the binding's last read, not live.
    pub fn slot_hint(&self, slot: u8, now: f64) -> Option<crate::strips::SlotHint> {
        let ctx = self.context_view(now)?;
        let slots = ctx.body.get("slots")?.as_array()?;
        let s = slots.iter().find(|x| x.get("n").and_then(|n| n.as_u64()) == Some(slot as u64))?;
        let state = s.get("val").and_then(|v| v.as_str()).unwrap_or("");
        let abs = s.get("abs").and_then(|v| v.as_f64()).map(|a| (a / 100.0).clamp(0.0, 1.0));
        Some(crate::strips::SlotHint { single: state == "value" && abs.is_some(), value: abs })
    }

    /// Whether a newer position may replace a queued one for this target (review 7): an encoder slot or an
    /// executor whose configured fader function the context reports as stateless. Unknown means no: every
    /// position is kept and the plugin decides.
    fn position_supersedes(&self, tgt: &CtlTarget, now: f64) -> bool {
        match tgt {
            CtlTarget::Slot { .. } => true,
            CtlTarget::Executor { ex, el } => {
                if *el != "fader" {
                    return false;
                }
                let Some(ctx) = self.context_view(now) else { return false };
                let Some(exs) = ctx.body.get("ex").and_then(|v| v.as_array()) else { return false };
                let Some(x) = exs.iter().find(|x| x.get("n").and_then(|n| n.as_u64()) == Some(*ex as u64)) else { return false };
                let tok = x.get("tok").and_then(|t| t.as_str()).or_else(|| x.get("fd").and_then(|t| t.as_str()));
                match tok {
                    Some(t) => {
                        let t = t.trim_start_matches("Fader").trim_start_matches("fader").to_ascii_lowercase();
                        !t.is_empty() && !STATEFUL_FUNCTIONS.contains(&t.as_str())
                    }
                    None => false,
                }
            }
        }
    }

    /// KB-18: a surface control moved, was touched or pressed. Motion and positions are queued and merged
    /// until `tick` sends them (one packet per control per `motion_min_interval`, dropped after
    /// `motion_max_age` or when the binding generation moved: never sent late, never reinterpreted).
    /// Touches and buttons are boundaries: the control's queued motion is sent first, then the boundary,
    /// which is retransmitted like a key until acknowledged. A release stops the retransmission of the
    /// press it ends. Motion and downs need the current binding generation from the plugin's context;
    /// without one they are dropped and counted (`ctl_unbound`). A release is always sent.
    pub fn control_event(&mut self, dev: &str, c: &str, tgt: CtlTarget, kind: CtlKind, now: f64) {
        self.stats.ctl_events += 1;
        let Some(session) = self.session() else {
            self.stats.dropped_unpaired += 1;
            return;
        };
        if !session.control {
            self.stats.ctl_unsupported += 1;
            return;
        }
        let key = (dev.to_string(), c.to_string());
        let cg = self.current_generation(now);
        match kind {
            CtlKind::Rel { dx, fine } => {
                if dx == 0 {
                    return;
                }
                self.stats.ctl_max_detent = self.stats.ctl_max_detent.max(dx.abs());
                let Some(cg) = cg else {
                    self.stats.ctl_unbound += 1;
                    return;
                };
                let gs = {
                    let idle = self.cfg.gesture_idle;
                    let gs_next = &mut self.gs_next;
                    let g = self.gestures.entry(key.clone()).or_insert_with(|| { let id = *gs_next; *gs_next += 1; Gesture { id, touching: false, last_at: now } });
                    if !g.touching && now - g.last_at > idle {
                        g.id = *gs_next;
                        *gs_next += 1;
                    }
                    g.last_at = now;
                    g.id
                };
                if let Some(m) = self.motion.iter_mut().rev().find(|m| m.dev == dev && m.c == c) {
                    if m.v.is_none() && m.cg == cg && m.gs == gs && m.fine == fine && m.tgt == tgt {
                        m.dx += dx;
                        m.events += 1;
                        self.stats.ctl_coalesced += 1;
                        return;
                    }
                }
                self.motion.push(Motion { dev: dev.into(), c: c.into(), tgt, cg, gs, fine, dx, v: None, takeover: false, first_at: now, events: 1 });
            }
            CtlKind::Abs { v, takeover } => {
                let Some(cg) = cg else {
                    self.stats.ctl_unbound += 1;
                    return;
                };
                let gs = {
                    let gs_next = &mut self.gs_next;
                    let g = self.gestures.entry(key.clone()).or_insert_with(|| { let id = *gs_next; *gs_next += 1; Gesture { id, touching: false, last_at: now } });
                    g.last_at = now;
                    g.id
                };
                // A newer position replaces the queued one only where the context says the target's function is
                // stateless (review 7); crossfades, Temp and unknown targets keep every position in order.
                if self.position_supersedes(&tgt, now) {
                    if let Some(m) = self.motion.iter_mut().rev().find(|m| m.dev == dev && m.c == c) {
                        if m.v.is_some() && m.cg == cg && m.gs == gs && m.tgt == tgt && m.takeover == takeover {
                            m.v = Some(v.clamp(0.0, 1.0));
                            m.events += 1;
                            self.stats.ctl_coalesced += 1;
                            return;
                        }
                    }
                }
                let queued = self.motion.iter().filter(|m| m.dev == dev && m.c == c).count();
                if queued >= self.cfg.motion_per_control {
                    if let Some(i) = self.motion.iter().position(|m| m.dev == dev && m.c == c) {
                        self.motion.remove(i);
                        self.stats.ctl_overflow += 1;
                    }
                }
                self.motion.push(Motion { dev: dev.into(), c: c.into(), tgt, cg, gs, fine: false, dx: 0, v: Some(v.clamp(0.0, 1.0)), takeover, first_at: now, events: 1 });
            }
            CtlKind::Touch { down } | CtlKind::Btn { down } => {
                let k: &'static str = if matches!(kind, CtlKind::Touch { .. }) { "touch" } else { "btn" };
                if down && cg.is_none() {
                    self.stats.ctl_unbound += 1;
                    return;
                }
                // Motion of this control queued before the boundary goes out first, in order.
                self.flush_control(dev, c, now);
                let gs = {
                    let gs_next = &mut self.gs_next;
                    let g = self.gestures.entry(key.clone()).or_insert_with(|| { let id = *gs_next; *gs_next += 1; Gesture { id, touching: false, last_at: now } });
                    if down {
                        g.id = *gs_next;
                        *gs_next += 1;
                        g.touching = k == "touch";
                    } else {
                        g.touching = false;
                        // A button up or touch up ends the gesture: the next delta starts a new one.
                        g.last_at = now - self.cfg.gesture_idle - 1.0;
                    }
                    if down { g.last_at = now; }
                    g.id
                };
                if !down {
                    let before = self.pending.len();
                    self.pending.retain(|p| !matches!(&p.kind, PendingKind::Ctl(b) if b.d == 1 && b.dev == dev && b.c == c));
                    let n = (before - self.pending.len()) as u64;
                    if n > 0 {
                        self.stats.ctl_superseded += n;
                        self.note(format!("{k} down on {dev}/{c} never acknowledged before its release: no longer retransmitted"));
                    }
                }
                let es = self.next_es(dev);
                let ev = self.ev_next;
                self.ev_next += 1;
                let b = CtlBoundary { k, dev: dev.into(), c: c.into(), es, cg: if down { cg } else { None }, gs, tgt, d: if down { 1 } else { 0 } };
                let p = Pending { ev, kind: PendingKind::Ctl(b), first_sent: now, next_at: now + self.cfg.retransmit_spacing, attempts: 1 };
                self.stats.events += 1;
                self.stats.ctl_sent += 1;
                self.send_pending(&p);
                self.pending.push(p);
            }
        }
    }

    /// Sends one queued motion packet. Never retransmitted.
    fn send_motion(&mut self, m: &Motion, now: f64) {
        let es = self.next_es(&m.dev);
        let ev = self.ev_next;
        self.ev_next += 1;
        self.stats.events += 1;
        self.stats.ctl_sent += 1;
        let (k, dx, v): (&'static str, Option<i32>, Option<f64>) = if m.v.is_some() { ("abs", None, m.v) } else { ("rel", Some(m.dx.clamp(-4096, 4096)), None) };
        let fine = if m.fine { Some(1) } else { None };
        let tk = if m.takeover && m.v.is_some() { Some(1) } else { None };
        self.send_session(|sid, seq| serde_json::to_string(&ToPlugin::Ctl { sid, seq, ev, k, dev: &m.dev, c: &m.c, es, cg: Some(m.cg), gs: Some(m.gs), tgt: m.tgt, dx, v, d: None, fine, tk }).expect("ctl serialises"));
        self.last_flush.insert((m.dev.clone(), m.c.clone()), now);
    }

    /// Sends every queued motion of one control now (before a boundary of that control).
    fn flush_control(&mut self, dev: &str, c: &str, now: f64) {
        let cg = self.current_generation(now);
        let mine: Vec<Motion> = self.motion.iter().filter(|m| m.dev == dev && m.c == c).cloned().collect();
        self.motion.retain(|m| !(m.dev == dev && m.c == c));
        for m in mine {
            if now - m.first_at > self.cfg.motion_max_age {
                self.stats.ctl_aged += 1;
            } else if cg != Some(m.cg) {
                self.stats.ctl_stale += 1;
            } else {
                self.send_motion(&m, now);
            }
        }
    }

    /// Bounded, in order, at most one packet per control per `motion_min_interval`: a control whose
    /// last packet is too recent keeps merging (its later entries wait too, so order holds).
    fn flush_motion(&mut self, now: f64) {
        if self.motion.is_empty() {
            return;
        }
        let cg = self.current_generation(now);
        let mut sent = 0usize;
        let mut waiting: BTreeSet<(String, String)> = BTreeSet::new();
        let mut keep: Vec<Motion> = Vec::new();
        let queue = std::mem::take(&mut self.motion);
        for m in queue {
            let key = (m.dev.clone(), m.c.clone());
            if now - m.first_at > self.cfg.motion_max_age {
                self.stats.ctl_aged += 1;
                continue;
            }
            if cg != Some(m.cg) {
                self.stats.ctl_stale += 1;
                continue;
            }
            if waiting.contains(&key) || sent >= self.cfg.ctl_per_tick || self.last_flush.get(&key).map(|t| now - t < self.cfg.motion_min_interval).unwrap_or(false) {
                waiting.insert(key);
                keep.push(m);
                continue;
            }
            self.send_motion(&m, now);
            sent += 1;
        }
        self.motion = keep;
    }

    /// A physical key went down or up. Events are sent once and retransmitted until acknowledged.
    pub fn key_event(&mut self, name: &'static str, down: bool, now: f64) {
        if down {
            self.held.insert(name);
        } else {
            self.held.remove(name);
        }
        if self.session().is_none() {
            self.stats.dropped_unpaired += 1;
            return;
        }
        if !down {
            // A press of this key that was never acknowledged must not be delivered after its release:
            // stop retransmitting it (the plugin rejects it as superseded anyway; the tap is lost).
            let before = self.pending.len();
            self.pending.retain(|p| !matches!(p.kind, PendingKind::Key { k, d: 1 } if k == name));
            let n = (before - self.pending.len()) as u64;
            if n > 0 {
                self.stats.superseded += n;
                self.note(format!("press of {name} never acknowledged before its release: no longer retransmitted"));
            }
        }
        let ev = self.ev_next;
        self.ev_next += 1;
        let p = Pending { ev, kind: PendingKind::Key { k: name, d: if down { 1 } else { 0 } }, first_sent: now, next_at: now + self.cfg.retransmit_spacing, attempts: 1 };
        self.stats.events += 1;
        self.send_pending(&p);
        self.pending.push(p);
    }


    /// Hellos while unpaired, heartbeats, retransmissions and the watchdog.
    pub fn tick(&mut self, now: f64) {
        let paired = match &self.phase {
            Phase::Unpaired { next_hello, .. } => {
                if now >= *next_hello {
                    self.send_hello(now);
                }
                return;
            }
            Phase::Paired(s) => (s.next_hb, s.hb_ms),
        };
        let (next_hb, hb_ms) = paired;
        let silent = now - self.last_rx;
        if silent > self.cfg.repair_after {
            self.note(format!("no plugin packet for {:.1} s: pairing again", silent));
            self.pending.clear();
            self.motion.clear();
            self.gestures.clear();
            self.link_up = false;
            self.console.full_seen = false;
            self.context = None;
            self.send_hello(now);
            return;
        }
        if silent > self.cfg.watchdog && self.link_up {
            self.link_up = false;
            self.context = None;
            // Queued motion against a context that is now unknown is never sent.
            self.stats.ctl_stale += self.motion.len() as u64;
            self.motion.clear();
            self.stats.link_downs += 1;
            self.note(format!("link down: no plugin packet for {:.1} s", silent));
        }
        if now >= next_hb {
            let held: Vec<&str> = self.held.iter().copied().collect();
            self.send_session(|sid, seq| serde_json::to_string(&ToPlugin::Hb { sid, seq, held }).expect("hb serialises"));
            if let Phase::Paired(s) = &mut self.phase {
                s.next_hb = now + hb_ms as f64 / 1000.0;
            }
        }
        // Retransmissions.
        let due: Vec<Pending> = self.pending.iter().filter(|p| now >= p.next_at).cloned().collect();
        for p in due {
            if p.attempts > self.cfg.retransmits {
                self.stats.lost += 1;
                self.note(format!("event {} ({:?}) lost: no ack after {} attempts", p.ev, p.kind, p.attempts));
                self.pending.retain(|q| q.ev != p.ev);
                continue;
            }
            self.stats.retransmitted += 1;
            self.send_pending(&p);
            if let Some(q) = self.pending.iter_mut().find(|q| q.ev == p.ev) {
                q.attempts += 1;
                q.next_at = now + self.cfg.retransmit_spacing;
            }
        }
        // KB-18: queued motion, bounded per tick.
        self.flush_motion(now);
    }

    /// Closes the session politely (the plugin releases what the surface held).
    pub fn bye(&mut self) {
        self.send_session(|sid, seq| serde_json::to_string(&ToPlugin::Bye { sid, seq }).expect("bye serialises"));
    }

    /// One datagram from the socket.
    pub fn receive(&mut self, data: &[u8], now: f64) {
        self.stats.received += 1;
        let text = match auth::unframe(&self.key, data, MAX_FROM_PLUGIN) {
            Ok(t) => t,
            Err(e) => {
                self.stats.rejected += 1;
                self.note(format!("rejected datagram: {e:?}"));
                return;
            }
        };
        let pkt: FromPlugin = match serde_json::from_str(text) {
            Ok(p) => p,
            Err(e) => {
                self.stats.rejected += 1;
                self.note(format!("unparseable plugin packet: {e}"));
                return;
            }
        };
        match pkt {
            FromPlugin::Welcome { v, sid, nonce: n, generation: pgen, lease, hb, keys, modules, input, seq, epoch, control, control_backend, .. } => {
                let expected = match &self.phase {
                    Phase::Unpaired { nonce: Some(x), .. } => x.clone(),
                    _ => {
                        self.stats.rejected += 1;
                        self.note("welcome while not waiting for one: ignored".into());
                        return;
                    }
                };
                if n != expected || v != PROTOCOL_VERSION {
                    self.stats.rejected += 1;
                    self.note(format!("welcome with nonce/version mismatch ignored (v{v})"));
                    return;
                }
                self.stats.welcomes += 1;
                let control = control == Some(1);
                self.note(format!("paired: sid {sid}, plugin gen {pgen}, lease {lease} ms, hb {hb} ms, {} keys ok, {} unsupported, input {:?}, control {}", keys.ok.len(), keys.unsupported.len(), input,
                    if control { control_backend.clone().unwrap_or_else(|| "on".into()) } else { "off (ctl events are dropped here)".into() }));
                self.phase = Phase::Paired(Session { sid, out_seq: 0, in_seq: seq, lease_ms: lease, hb_ms: hb, plugin_gen: pgen.clone(), keys, modules, input, control, control_backend, next_hb: now });
                self.pending.clear();
                self.motion.clear();
                self.gestures.clear();
                self.console = ConsoleState { generation: pgen, epoch: epoch.unwrap_or(0), items: BTreeMap::new(), full_seen: false, last_state_at: now };
                // KB-17 review: a new pairing is a new plugin run; the previous context (and its generation) is gone.
                self.context = None;
                self.last_rx = now;
                self.link_up = true;
            }
            FromPlugin::Err { sid, e, .. } => {
                if e == "no-session" {
                    let ours = self.session().map(|s| s.sid.clone());
                    if ours.is_some() && sid == ours {
                        self.stats.no_session += 1;
                        self.note("plugin knows no session for our sid (plugin restarted or forgot us): pairing again".into());
                        self.pending.clear();
                        self.link_up = false;
                        self.context = None;
                        self.console.full_seen = false;
                        self.send_hello(now);
                    }
                } else {
                    self.note(format!("plugin error: {e}"));
                }
            }
            FromPlugin::Ack { sid, seq, ev, ok, code, why, lost, coalesced, cg, .. } => {
                if !self.admit(&sid, seq, now) {
                    return;
                }
                let mut was_ctl = false;
                if let Some(i) = self.pending.iter().position(|p| p.ev == ev) {
                    let p = self.pending.remove(i);
                    was_ctl = matches!(p.kind, PendingKind::Ctl(_));
                    self.rtt_ms.push((now - p.first_sent) * 1000.0);
                }
                if let Some(n) = lost {
                    // KB-18: the plugin saw a gap in our per-device sequence: packets were lost on the way.
                    self.stats.ctl_lost_reported += n;
                    self.note(format!("event {ev}: the plugin reports {n} lost control packet(s) before it (never replayed)"));
                }
                let _ = coalesced;
                if ok == 1 {
                    self.stats.acked += 1;
                } else {
                    self.stats.refused += 1;
                    let is_ctl = was_ctl || lost.is_some() || cg.is_some() || matches!(code.as_deref(), Some("stale-generation" | "binding-unknown" | "target-unavailable" | "out-of-order" | "conflict" | "control-disabled" | "gesture-rebound" | "queue-full" | "rate"));
                    if is_ctl {
                        self.stats.ctl_refused += 1;
                    }
                    if let (Some("stale-generation"), Some(g)) = (code.as_deref(), cg) {
                        self.note(format!("event {ev}: the plugin's binding generation is {g}; our context lags (events against it are refused until the next context message)"));
                    }
                    let r = Refusal { ev, code: code.unwrap_or_default(), why: why.unwrap_or_default() };
                    self.note(format!("event {} refused [{}] {}", r.ev, r.code, r.why));
                    self.refusals.push(r);
                    if self.refusals.len() > 100 {
                        self.refusals.remove(0);
                    }
                }
            }
            FromPlugin::Hb { sid, seq, held, unsynced, resynced, reconciled, .. } => {
                if !self.admit(&sid, seq, now) {
                    return;
                }
                if resynced == Some(1) {
                    self.note("session revived by the plugin after a lease expiry; its holds were released, nothing re-pressed".into());
                }
                if let Some(n) = reconciled {
                    self.note(format!("plugin released {n} key(s) whose release event never arrived"));
                }
                self.plugin_held = held;
                self.unsynced = unsynced;
            }
            FromPlugin::State { sid, seq, generation: pgen, epoch, full, s } => {
                if !self.admit(&sid, seq, now) {
                    return;
                }
                self.stats.states += 1;
                if full == 1 {
                    if self.console.full_seen && (self.console.generation != pgen || self.console.epoch != epoch) {
                        self.note(format!("plugin generation/epoch changed ({}/{} -> {}/{}): state replaced", self.console.generation, self.console.epoch, pgen, epoch));
                    }
                    self.console = ConsoleState { generation: pgen, epoch, items: s, full_seen: true, last_state_at: now };
                } else if self.console.full_seen && self.console.generation == pgen && self.console.epoch == epoch {
                    for (k, v) in s {
                        self.console.items.insert(k, v);
                    }
                    self.console.last_state_at = now;
                } else {
                    // A delta against a state we do not have: wait for the next full one.
                    self.stats.deltas_ignored += 1;
                    self.console.full_seen = false;
                }
            }
            FromPlugin::Context { sid, seq, generation: pgen, epoch, known, cg, rest } => {
                if !self.admit(&sid, seq, now) {
                    return;
                }
                self.stats.contexts += 1;
                let known = known == 1;
                let moved = match &self.context {
                    Some(prev) => prev.plugin_generation != pgen || prev.epoch != epoch || prev.known != known || prev.generation != cg,
                    None => true,
                };
                if moved {
                    self.stats.context_generations += 1;
                    if known {
                        let enc = rest.get("enc").cloned().unwrap_or(Value::Null);
                        let describe = |v: &Value, k: &str| v.get(k).map(|x| x.to_string()).unwrap_or_else(|| "?".into());
                        self.note(format!(
                            "context generation {} (epoch {}): bank {} {} page {} {} ctx {}, {} slot(s), {} executor(s), exec page {}",
                            cg.map(|g| g.to_string()).unwrap_or_else(|| "?".into()), epoch,
                            describe(&enc, "bank"), describe(&enc, "bankName"), describe(&enc, "page"), describe(&enc, "pageName"), describe(&enc, "ctx"),
                            rest.get("slots").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
                            rest.get("ex").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
                            rest.get("page").map(|v| v.to_string()).unwrap_or_else(|| "?".into())
                        ));
                    } else {
                        self.note(format!("context unknown (epoch {}): the plugin has not observed every part yet", epoch));
                    }
                }
                self.context = Some(ConsoleContext { plugin_generation: pgen, epoch, known, generation: cg, body: rest, received_at: now });
            }
            FromPlugin::Effect { sid, seq, ev, ms, frames, why } => {
                if !self.admit(&sid, seq, now) {
                    return;
                }
                self.effect_ms.push(ms);
                if ms < 0 {
                    self.note(format!("event {ev}: no observable effect ({})", why.unwrap_or_default()));
                } else {
                    self.note(format!("event {ev}: effect after {ms} ms ({frames} frame(s))"));
                }
            }
        }
    }

    /// Session and sequence check for plugin packets; marks the link up and renews the watchdog.
    fn admit(&mut self, sid: &str, seq: u64, now: f64) -> bool {
        let s = match &mut self.phase {
            Phase::Paired(s) if s.sid == sid => s,
            _ => {
                self.stats.rejected += 1;
                return false;
            }
        };
        if seq <= s.in_seq {
            self.stats.old_seq += 1;
            return false;
        }
        s.in_seq = seq;
        self.last_rx = now;
        if !self.link_up {
            self.link_up = true;
            self.note("link up".into());
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A plugin stand-in: decodes what the link sent and answers like mtpnxk_surface.lua would.
    struct FakePlugin {
        key: Key,
        sid: String,
        seq: u64,
        generation: String,
        pub seen: Vec<Value>,
    }

    impl FakePlugin {
        fn new(key: Key) -> FakePlugin {
            FakePlugin { key, sid: "feedfacefeedface".into(), seq: 0, generation: "1-deadbeef".into(), seen: Vec::new() }
        }
        fn decode(&mut self, frames: Vec<Vec<u8>>) -> Vec<Value> {
            let mut out = Vec::new();
            for f in frames {
                let text = auth::unframe(&self.key, &f, 512).expect("link frames verify");
                let v: Value = serde_json::from_str(text).unwrap();
                self.seen.push(v.clone());
                out.push(v);
            }
            out
        }
        fn frame(&mut self, mut v: Value) -> Vec<u8> {
            self.seq += 1;
            v["sid"] = json!(self.sid);
            v["seq"] = json!(self.seq);
            auth::frame(&self.key, &v.to_string())
        }
        fn welcome(&mut self, nonce: &str) -> Vec<u8> {
            let v = json!({"t":"welcome","v":1,"sid":self.sid,"nonce":nonce,"gen":self.generation,"lease":2000,"hb":250,"keys":{"ok":["Record","5"],"unsupported":["Bank"]},"modules":{"gma3_mcp_hardkeys":"0.5.0"},"input":"keyboard","epoch":1,"control":1,"controlBackend":"fake","seq":0});
            self.seq = 0;
            auth::frame(&self.key, &v.to_string())
        }
        fn welcome_without_control(&mut self, nonce: &str) -> Vec<u8> {
            let v = json!({"t":"welcome","v":1,"sid":self.sid,"nonce":nonce,"gen":self.generation,"lease":2000,"hb":250,"keys":{"ok":["Record"],"unsupported":[]},"input":"keyboard","epoch":1,"seq":0});
            self.seq = 0;
            auth::frame(&self.key, &v.to_string())
        }
        fn ctl_ack(&mut self, ev: u64, ok: bool, extra: Value) -> Vec<u8> {
            let mut v = json!({"t":"ack","ev":ev,"ok":if ok {1} else {0}});
            if let Value::Object(m) = extra { for (k, x) in m { v[k] = x; } }
            self.frame(v)
        }
        fn ack(&mut self, ev: u64, ok: bool) -> Vec<u8> {
            self.frame(json!({"t":"ack","ev":ev,"ok":if ok {1} else {0},"code":if ok {Value::Null} else {json!("unsupported")},"why":"nope"}))
        }
        fn state(&mut self, full: bool, epoch: u64, items: Value) -> Vec<u8> {
            self.frame(json!({"t":"state","gen":self.generation,"epoch":epoch,"full":if full {1} else {0},"s":items}))
        }
        fn context(&mut self, epoch: u64, known: u8, cg: Option<u64>, enc: Value) -> Vec<u8> {
            let mut v = json!({"t":"context","gen":self.generation,"epoch":epoch,"known":known,"enc":enc,"slots":[],"ex":[],"page":1,"pool":"Default","display":1});
            if let Some(g) = cg { v["cg"] = json!(g); }
            self.frame(v)
        }
    }

    fn pair(now: f64) -> (Link, FakePlugin) {
        let key = Key::from_hex(&"0123456789abcdef".repeat(4)).unwrap();
        let mut link = Link::new(Config { id: "nxk-test".into(), ..Config::default() }, key.clone(), now);
        let mut plugin = FakePlugin::new(key);
        link.tick(now);
        let hello = plugin.decode(link.take_outgoing());
        assert_eq!(hello.len(), 1);
        assert_eq!(hello[0]["t"], "hello");
        assert_eq!(hello[0]["id"], "nxk-test");
        let nonce = hello[0]["nonce"].as_str().unwrap().to_string();
        let w = plugin.welcome(&nonce);
        link.receive(&w, now + 0.01);
        assert!(link.session().is_some(), "paired");
        assert_eq!(link.session().unwrap().keys.ok, vec!["Record", "5"]);
        (link, plugin)
    }

    #[test]
    fn pairs_and_rejects_a_welcome_with_the_wrong_nonce() {
        let key = Key::from_hex(&"00".repeat(32)).unwrap();
        let mut link = Link::new(Config::default(), key.clone(), 0.0);
        let mut plugin = FakePlugin::new(key);
        link.tick(0.0);
        plugin.decode(link.take_outgoing());
        let bad = plugin.welcome("not-our-nonce");
        link.receive(&bad, 0.01);
        assert!(link.session().is_none());
        assert_eq!(link.stats.rejected, 1);
        // Hellos repeat every hello_interval while unpaired.
        link.tick(1.0);
        assert!(link.take_outgoing().is_empty());
        link.tick(2.1);
        assert_eq!(plugin.decode(link.take_outgoing())[0]["t"], "hello");
        assert_eq!(link.stats.hellos, 2);
    }

    #[test]
    fn key_events_are_acked_or_retransmitted_then_lost() {
        let (mut link, mut plugin) = pair(10.0);
        link.key_event("Record", true, 10.1);
        let sent = plugin.decode(link.take_outgoing());
        assert_eq!(sent[0]["t"], "key");
        assert_eq!(sent[0]["ev"], 1);
        assert_eq!(sent[0]["seq"], 1);
        assert_eq!(sent[0]["d"], 1);
        // Ack arrives: no retransmission, RTT recorded.
        let a = plugin.ack(1, true);
        link.receive(&a, 10.112);
        link.tick(10.2);
        let keys = |v: Vec<Value>| -> Vec<Value> { v.into_iter().filter(|p| p["t"] == "key").collect() };
        assert!(keys(plugin.decode(link.take_outgoing())).is_empty(), "acked event is not retransmitted");
        assert_eq!(link.stats.acked, 1);
        assert!((link.rtt_ms[0] - 12.0).abs() < 0.01);
        // No ack: retransmitted 3 times with the same ev and new seqs, then lost.
        link.key_event("Record", false, 11.0);
        let first = plugin.decode(link.take_outgoing());
        let mut seqs = vec![first[0]["seq"].as_u64().unwrap()];
        for i in 1..=4 {
            link.tick(11.0 + 0.07 * i as f64);
            let r = keys(plugin.decode(link.take_outgoing()));
            assert_eq!(r.len(), 1, "retransmit {i}");
            assert_eq!(r[0]["ev"], 2);
            seqs.push(r[0]["seq"].as_u64().unwrap());
        }
        link.tick(11.4);
        assert!(keys(plugin.decode(link.take_outgoing())).is_empty(), "given up after the retransmit budget");
        assert_eq!(link.stats.retransmitted, 4);
        assert_eq!(link.stats.lost, 1);
        assert!(seqs.windows(2).all(|w| w[1] > w[0]), "every copy carries a fresh seq: {seqs:?}");
        // A late ack for the lost event is harmless.
        let late = plugin.ack(2, true);
        link.receive(&late, 11.5);
        assert_eq!(link.stats.acked, 2);
    }

    #[test]
    fn a_release_stops_retransmitting_its_unacknowledged_press() {
        let (mut link, mut plugin) = pair(0.0);
        link.key_event("Record", true, 0.1);
        plugin.decode(link.take_outgoing()); // press lost on the way (never acked)
        link.key_event("Record", false, 0.12);
        let out = plugin.decode(link.take_outgoing());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["d"], 0);
        for i in 1..=5 {
            link.tick(0.12 + 0.07 * i as f64);
        }
        let later = plugin.decode(link.take_outgoing());
        assert!(later.iter().all(|p| p["t"] != "key" || p["d"] == 0), "only the release is retransmitted: {later:?}");
        assert_eq!(link.stats.superseded, 1);
    }

    #[test]
    fn refusals_are_recorded() {
        let (mut link, mut plugin) = pair(0.0);
        link.key_event("Bank", true, 0.1);
        plugin.decode(link.take_outgoing());
        let a = plugin.ack(1, false);
        link.receive(&a, 0.11);
        assert_eq!(link.stats.refused, 1);
        assert_eq!(link.refusals[0].code, "unsupported");
        assert!(link.pending.is_empty());
    }

    #[test]
    fn heartbeats_carry_the_held_set_and_old_plugin_seqs_are_dropped() {
        let (mut link, mut plugin) = pair(0.0);
        link.key_event("Record", true, 0.05);
        plugin.decode(link.take_outgoing());
        link.tick(0.3);
        let out = plugin.decode(link.take_outgoing());
        let hb = out.iter().find(|p| p["t"] == "hb").expect("heartbeat");
        assert_eq!(hb["held"], json!(["Record"]));
        let reply = plugin.frame(json!({"t":"hb","held":["Record"],"unsynced":["Clear"]}));
        link.receive(&reply, 0.31);
        assert_eq!(link.unsynced, vec!["Clear"]);
        let stale = auth::frame(&plugin.key, &json!({"t":"hb","sid":plugin.sid,"seq":1,"held":[]}).to_string());
        link.receive(&stale, 0.32);
        assert_eq!(link.stats.old_seq, 1);
        assert_eq!(link.plugin_held, vec!["Record"], "stale packet ignored");
    }

    #[test]
    fn console_state_freshness_follows_full_delta_generation_and_watchdog() {
        let (mut link, mut plugin) = pair(0.0);
        assert!(!link.console(0.0).fresh, "nothing received yet");
        let full = plugin.state(true, 1, json!({"blind":0,"highlight":1,"pending":"store"}));
        link.receive(&full, 0.1);
        let v = link.console(0.2);
        assert!(v.fresh);
        assert_eq!(v.items.unwrap()["highlight"], 1);
        let delta = plugin.state(false, 1, json!({"blind":1}));
        link.receive(&delta, 0.3);
        assert_eq!(link.console(0.3).items.unwrap()["blind"], 1);
        assert_eq!(link.console(0.3).items.unwrap()["pending"], "store");
        // An epoch change: a delta is ignored until the next full state.
        let delta2 = plugin.state(false, 2, json!({"blind":0}));
        link.receive(&delta2, 0.4);
        assert!(!link.console(0.4).fresh);
        assert_eq!(link.stats.deltas_ignored, 1);
        let full2 = plugin.state(true, 2, json!({"blind":0,"highlight":0}));
        link.receive(&full2, 0.5);
        assert!(link.console(0.5).fresh);
        // No state for state_stale: unknown. No packet for the watchdog: link down.
        assert!(!link.console(2.5).fresh);
        link.tick(2.5);
        assert!(!link.link_up);
        assert_eq!(link.stats.link_downs, 1);
        // A packet brings the link back; silence past repair_after re-pairs.
        let hb = plugin.frame(json!({"t":"hb","held":[]}));
        link.receive(&hb, 2.6);
        assert!(link.link_up);
        link.tick(7.0);
        assert!(link.session().is_none());
        let out = plugin.decode(link.take_outgoing());
        assert!(out.iter().any(|p| p["t"] == "hello"));
    }

    #[test]
    fn no_session_error_triggers_a_new_hello_and_unpaired_events_are_dropped() {
        let (mut link, mut plugin) = pair(0.0);
        let sid = plugin.sid.clone();
        let err = auth::frame(&plugin.key, &json!({"t":"err","e":"no-session","sid":sid,"seq":0}).to_string());
        link.receive(&err, 0.5);
        assert!(link.session().is_none());
        assert_eq!(link.stats.no_session, 1);
        let out = plugin.decode(link.take_outgoing());
        assert_eq!(out[0]["t"], "hello");
        link.key_event("5", true, 0.6);
        assert!(link.take_outgoing().is_empty());
        assert_eq!(link.stats.dropped_unpaired, 1);
        assert_eq!(out[0]["held"], json!([]));
        // The next hello reports what is physically down.
        link.tick(3.0);
        let out = plugin.decode(link.take_outgoing());
        assert_eq!(out[0]["held"], json!(["5"]));
    }

    #[test]
    fn tampered_and_foreign_packets_are_rejected() {
        let (mut link, mut plugin) = pair(0.0);
        let other = Key::from_hex(&"ff".repeat(32)).unwrap();
        let forged = auth::frame(&other, &json!({"t":"hb","sid":plugin.sid,"seq":5}).to_string());
        link.receive(&forged, 0.1);
        let foreign = plugin.frame(json!({"t":"hb","held":[]}));
        let mut foreign_sid = foreign.clone();
        let _ = &mut foreign_sid;
        link.receive(&auth::frame(&plugin.key, &json!({"t":"hb","sid":"0000000000000000","seq":9,"held":[]}).to_string()), 0.2);
        assert_eq!(link.stats.rejected, 2);
        link.receive(&foreign, 0.3);
        assert_eq!(link.stats.rejected, 2);
    }
    #[test]
    fn context_messages_are_kept_as_data_and_generation_moves_are_counted() {
        let (mut link, mut plugin) = pair(100.0);
        assert!(link.context.is_none());
        let unknown = plugin.context(1, 0, None, json!({"why":"not observed"}));
        link.receive(&unknown, 100.1);
        let c = link.context.as_ref().unwrap();
        assert!(!c.known && c.generation.is_none());
        assert_eq!(link.stats.contexts, 1);
        assert_eq!(link.stats.context_generations, 1);
        let known = plugin.context(1, 1, Some(1), json!({"bank":1,"bankName":"Dimmer","page":1,"pageName":"Dimmer","ctx":"Default","attr":1}));
        link.receive(&known, 100.2);
        let c = link.context.as_ref().unwrap();
        assert!(c.known && c.generation == Some(1));
        assert_eq!(c.body["enc"]["bankName"], "Dimmer");
        assert_eq!(link.stats.context_generations, 2);
        let same = plugin.context(1, 1, Some(1), json!({"bank":1,"bankName":"Dimmer","page":1,"pageName":"Dimmer","ctx":"Default","attr":1}));
        link.receive(&same, 101.2);
        assert_eq!(link.stats.contexts, 3);
        assert_eq!(link.stats.context_generations, 2, "a repeat of the same generation is not a move");
        let moved = plugin.context(1, 1, Some(2), json!({"bank":4,"bankName":"Color","page":1,"pageName":"RGB","ctx":"Default","attr":1}));
        link.receive(&moved, 101.3);
        assert_eq!(link.context.as_ref().unwrap().generation, Some(2));
        assert_eq!(link.stats.context_generations, 3);
        assert!(link.log.iter().any(|l| l.contains("context generation 2") && l.contains("Color")), "{:?}", link.log);
    }

    #[test]
    fn context_is_dropped_on_link_down_repair_and_a_new_pairing_until_a_fresh_one_arrives() {
        let (mut link, mut plugin) = pair(0.0);
        let known = plugin.context(1, 1, Some(5), json!({"bank":4,"bankName":"Color","page":1,"pageName":"RGB","ctx":"Default","attr":1}));
        link.receive(&known, 0.1);
        assert_eq!(link.context_view(0.2).unwrap().generation, Some(5));
        // Stale by age: not presented, though still stored.
        assert!(link.context_view(2.0).is_none());
        // Watchdog: link down drops it.
        link.tick(2.5);
        assert!(!link.link_up);
        assert!(link.context.is_none());
        let hb = plugin.frame(json!({"t":"hb","held":[]}));
        link.receive(&hb, 2.6);
        assert!(link.link_up);
        assert!(link.context_view(2.6).is_none(), "the link is back but no context arrived in it");
        // Silence past repair_after: unpaired, nothing kept.
        link.tick(7.0);
        assert!(link.session().is_none());
        assert!(link.context.is_none());
        // A new pairing (a new plugin run with its own generations) starts without a context.
        let out = plugin.decode(link.take_outgoing());
        let nonce = out.iter().find(|p| p["t"] == "hello").unwrap()["nonce"].as_str().unwrap().to_string();
        plugin.generation = "2-deadbeef".into();
        let w = plugin.welcome(&nonce);
        link.receive(&w, 7.1);
        assert!(link.session().is_some());
        assert!(link.context.is_none() && link.context_view(7.1).is_none());
        let fresh = plugin.context(1, 1, Some(1), json!({"bank":1,"bankName":"Dimmer","page":1,"pageName":"Dimmer","ctx":"Default","attr":1}));
        link.receive(&fresh, 7.2);
        let c = link.context_view(7.3).unwrap();
        assert_eq!((c.generation, c.plugin_generation.as_str()), (Some(1), "2-deadbeef"));
        // A context the plugin marks unknown is stored but never presented.
        let unknown = plugin.context(1, 0, None, json!({"why":"not observed"}));
        link.receive(&unknown, 7.4);
        assert!(link.context.is_some() && link.context_view(7.4).is_none());
    }

    // ---- KB-18: continuous-control transport ----

    fn slot(n: u8) -> CtlTarget { CtlTarget::Slot { slot: n } }
    fn ctl_packets(v: &[Value]) -> Vec<&Value> { v.iter().filter(|p| p["t"] == "ctl").collect() }

    #[test]
    fn control_events_need_a_known_binding_generation_and_carry_it() {
        let (mut link, mut plugin) = pair(10.0);
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 2, fine: false }, 10.1);
        link.tick(10.11);
        assert!(ctl_packets(&plugin.decode(link.take_outgoing())).is_empty(), "no context yet: nothing sent");
        assert_eq!(link.stats.ctl_unbound, 1);
        // An unknown context (known 0) binds nothing either.
        link.receive(&plugin.context(1, 0, None, json!({"why":"not observed"})), 10.2);
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Btn { down: true }, 10.21);
        assert_eq!(link.stats.ctl_unbound, 2);
        link.receive(&plugin.context(1, 1, Some(3), json!({"bank":1})), 10.3);
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 2, fine: false }, 10.31);
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: -1, fine: false }, 10.312);
        link.control_event("nxk", "Rotary2", slot(2), CtlKind::Rel { dx: 5, fine: true }, 10.313);
        assert_eq!(link.queued_motion(), 2);
        assert_eq!(link.stats.ctl_coalesced, 1);
        link.tick(10.32);
        let out = plugin.decode(link.take_outgoing());
        let c = ctl_packets(&out);
        assert_eq!(c.len(), 2, "{out:?}");
        assert_eq!(c[0]["k"], "rel"); assert_eq!(c[0]["dev"], "nxk"); assert_eq!(c[0]["c"], "Rotary1"); assert_eq!(c[0]["dx"], 1); assert_eq!(c[0]["cg"], 3); assert_eq!(c[0]["es"], 1); assert_eq!(c[0]["tgt"]["slot"], 1); assert!(c[0]["fine"].is_null());
        assert_eq!(c[1]["c"], "Rotary2"); assert_eq!(c[1]["dx"], 5); assert_eq!(c[1]["fine"], 1); assert_eq!(c[1]["es"], 2, "one sequence per device");
        assert_eq!(link.stats.ctl_max_detent, 5, "KB-19: the largest single-event delta is recorded, never amplified");
        assert_ne!(c[0]["gs"], c[1]["gs"], "each control has its own gesture");
        assert_eq!(link.stats.ctl_sent, 2);
        // Motion is never retransmitted.
        link.tick(10.5); link.tick(10.6);
        assert!(ctl_packets(&plugin.decode(link.take_outgoing())).is_empty());
        assert_eq!(link.pending_count(), 0);
    }

    #[test]
    fn motion_is_rate_bounded_per_control_merging_meanwhile_and_a_new_gesture_starts_after_idle() {
        let (mut link, mut plugin) = pair(20.0);
        link.receive(&plugin.context(1, 1, Some(1), json!({})), 20.0);
        for i in 0..10 {
            link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 1, fine: false }, 20.0 + i as f64 * 0.0004);
            link.tick(20.0 + i as f64 * 0.0004);
        }
        let out = plugin.decode(link.take_outgoing());
        let c = ctl_packets(&out);
        // The first delta goes out at once; the next nine (within 5 ms of it) merge into one packet.
        assert_eq!(c.len(), 1, "{out:?}");
        assert_eq!(c[0]["dx"], 1);
        assert_eq!(link.queued_motion(), 1);
        link.tick(20.006);
        let c2: Vec<Value> = plugin.decode(link.take_outgoing());
        let c2 = ctl_packets(&c2);
        assert_eq!(c2.len(), 1);
        assert_eq!(c2[0]["dx"], 9);
        assert_eq!(c2[0]["gs"], c[0]["gs"], "same gesture");
        assert_eq!(c2[0]["es"], 2);
        // After the idle time the next delta is a new gesture.
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 1, fine: false }, 20.5);
        link.tick(20.5);
        let c3: Vec<Value> = plugin.decode(link.take_outgoing());
        let c3 = ctl_packets(&c3);
        assert_ne!(c3[0]["gs"], c[0]["gs"]);
    }

    #[test]
    fn stale_or_old_motion_is_dropped_instead_of_sent_late() {
        let (mut link, mut plugin) = pair(30.0);
        link.receive(&plugin.context(1, 1, Some(1), json!({})), 30.0);
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 3, fine: false }, 30.0);
        // The generation moves before the tick sends it: the delta was produced against generation 1.
        link.receive(&plugin.context(1, 1, Some(2), json!({})), 30.001);
        link.tick(30.002);
        assert!(ctl_packets(&plugin.decode(link.take_outgoing())).is_empty());
        assert_eq!(link.stats.ctl_stale, 1);
        assert_eq!(link.queued_motion(), 0);
        // A delta that could not be sent within motion_max_age is dropped.
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 3, fine: false }, 30.1);
        link.tick(30.1);
        plugin.decode(link.take_outgoing());
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 1, fine: false }, 30.102); // merges, waits for the rate interval
        link.tick(30.4); // but the tick comes too late
        assert!(ctl_packets(&plugin.decode(link.take_outgoing())).is_empty());
        assert_eq!(link.stats.ctl_aged, 1);
        // Link down discards queued motion too.
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 1, fine: false }, 30.5);
        link.tick(32.1); // watchdog
        assert!(!link.link_up);
        assert_eq!(link.queued_motion(), 0);
        assert_eq!(link.stats.ctl_stale, 2);
    }

    #[test]
    fn touch_and_button_boundaries_are_retransmitted_and_a_release_supersedes_the_press_after_flushing_motion() {
        let (mut link, mut plugin) = pair(40.0);
        link.receive(&plugin.context(1, 1, Some(1), json!({})), 40.0);
        let exec = CtlTarget::Executor { ex: 201, el: "fader" };
        // The context says executor 201's fader is a Master: a newer position may replace a queued one.
        let mut v = json!({"t":"context","gen":plugin.generation,"epoch":1,"known":1,"cg":1,"enc":{},"slots":[],"page":1,"pool":"Default","display":1,
                           "ex":[{"n":201,"empty":0,"tgt":1,"fd":"Master","tok":"FaderMaster"},{"n":202,"empty":0,"tgt":1,"fd":"X","tok":"FaderX"}]});
        v["sid"] = json!(plugin.sid);
        let f = plugin.frame(v);
        link.receive(&f, 40.0);
        link.control_event("mtouch", "Strip1", exec, CtlKind::Touch { down: true }, 40.0);
        link.control_event("mtouch", "Strip1", exec, CtlKind::Abs { v: 0.25, takeover: false }, 40.001);
        link.control_event("mtouch", "Strip1", exec, CtlKind::Abs { v: 0.5, takeover: false }, 40.002);
        link.control_event("mtouch", "Strip1", exec, CtlKind::Touch { down: false }, 40.003);
        let out = plugin.decode(link.take_outgoing());
        let c = ctl_packets(&out);
        assert_eq!(c.len(), 3, "{out:?}");
        assert_eq!(c[0]["k"], "touch"); assert_eq!(c[0]["d"], 1); assert_eq!(c[0]["cg"], 1); assert_eq!(c[0]["es"], 1); assert_eq!(c[0]["tgt"]["ex"], 201); assert_eq!(c[0]["tgt"]["el"], "fader");
        assert_eq!(c[1]["k"], "abs"); assert_eq!(c[1]["v"], 0.5); assert_eq!(c[1]["es"], 2); assert_eq!(c[1]["gs"], c[0]["gs"], "the position belongs to the touch's gesture");
        assert_eq!(c[2]["k"], "touch"); assert_eq!(c[2]["d"], 0); assert!(c[2]["cg"].is_null(), "a release carries no generation"); assert_eq!(c[2]["es"], 3);
        assert_eq!(link.stats.ctl_coalesced, 1);
        // The press was never acknowledged: its release superseded it, only the release is pending.
        assert_eq!(link.pending_count(), 1);
        assert_eq!(link.stats.ctl_superseded, 1);
        let rel_ev = c[2]["ev"].as_u64().unwrap();
        link.tick(40.07);
        let again = plugin.decode(link.take_outgoing());
        let again = ctl_packets(&again);
        assert_eq!(again.len(), 1); assert_eq!(again[0]["ev"], rel_ev); assert_eq!(again[0]["es"], 3, "a retransmission keeps its sequence");
        link.receive(&plugin.ctl_ack(rel_ev, true, json!({"boundary":1})), 40.08);
        assert_eq!(link.pending_count(), 0);
        assert_eq!(link.stats.acked, 1);
        // A button on a rotary: the next delta after it is a new gesture.
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 1, fine: false }, 40.2);
        link.tick(40.2);
        let g1 = ctl_packets(&plugin.decode(link.take_outgoing()))[0]["gs"].clone();
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Btn { down: true }, 40.21);
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Btn { down: false }, 40.22);
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 1, fine: false }, 40.23);
        link.tick(40.23);
        let out = plugin.decode(link.take_outgoing());
        let c = ctl_packets(&out);
        assert_eq!(c.len(), 3);
        assert_eq!(c[0]["k"], "btn"); assert_eq!(c[2]["k"], "rel");
        assert_ne!(c[2]["gs"], g1, "a button is a boundary: a new gesture");
    }

    #[test]
    fn ack_loss_reports_and_control_refusals_are_counted() {
        let (mut link, mut plugin) = pair(50.0);
        link.receive(&plugin.context(1, 1, Some(4), json!({})), 50.0);
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 1, fine: false }, 50.0);
        link.tick(50.0);
        let ev = ctl_packets(&plugin.decode(link.take_outgoing()))[0]["ev"].as_u64().unwrap();
        link.receive(&plugin.ctl_ack(ev, true, json!({"queued":1,"lost":2})), 50.03);
        assert_eq!(link.stats.ctl_lost_reported, 2);
        assert_eq!(link.stats.acked, 1);
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 1, fine: false }, 50.1);
        link.tick(50.1);
        let ev2 = ctl_packets(&plugin.decode(link.take_outgoing()))[0]["ev"].as_u64().unwrap();
        link.receive(&plugin.ctl_ack(ev2, false, json!({"code":"stale-generation","why":"event generation 4 is not 5","cg":5})), 50.13);
        assert_eq!(link.stats.ctl_refused, 1);
        assert_eq!(link.stats.refused, 1);
        assert!(link.log.iter().any(|l| l.contains("binding generation is 5")), "{:?}", link.log);
    }

    #[test]
    fn a_plugin_without_control_drops_ctl_events_and_a_repair_clears_gestures() {
        let key = Key::from_hex(&"0123456789abcdef".repeat(4)).unwrap();
        let mut link = Link::new(Config { id: "nxk-test".into(), ..Config::default() }, key.clone(), 60.0);
        let mut plugin = FakePlugin::new(key);
        link.tick(60.0);
        let hello = plugin.decode(link.take_outgoing());
        let nonce = hello[0]["nonce"].as_str().unwrap().to_string();
        link.receive(&plugin.welcome_without_control(&nonce), 60.01);
        assert!(link.session().is_some() && !link.session().unwrap().control);
        link.receive(&plugin.context(1, 1, Some(1), json!({})), 60.02);
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 1, fine: false }, 60.03);
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Btn { down: true }, 60.04);
        link.tick(60.05);
        assert!(ctl_packets(&plugin.decode(link.take_outgoing())).is_empty());
        assert_eq!(link.stats.ctl_unsupported, 2);
        // A repair (no plugin packets) clears pending boundaries, motion and gestures.
        let (mut link, mut plugin) = pair(70.0);
        link.receive(&plugin.context(1, 1, Some(1), json!({})), 70.0);
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Btn { down: true }, 70.01);
        link.control_event("nxk", "Rotary1", slot(1), CtlKind::Rel { dx: 1, fine: false }, 70.02);
        plugin.decode(link.take_outgoing());
        link.tick(74.5);
        assert_eq!(link.pending_count(), 0);
        assert_eq!(link.queued_motion(), 0);
        assert_eq!(plugin.decode(link.take_outgoing()).last().unwrap()["t"], "hello");
    }
    #[test]
    fn positions_of_a_stateful_or_unknown_target_are_all_kept_in_order_and_bounded() {
        let (mut link, mut plugin) = pair(80.0);
        let mut v = json!({"t":"context","gen":plugin.generation,"epoch":1,"known":1,"cg":1,"enc":{},"slots":[],"page":1,"pool":"Default","display":1,
                           "ex":[{"n":201,"empty":0,"tgt":1,"fd":"Master","tok":"FaderMaster"},{"n":202,"empty":0,"tgt":1,"fd":"X","tok":"FaderX"},{"n":203,"empty":0,"tgt":1,"fd":"Temp","tok":"FaderTemp"}]});
        v["sid"] = json!(plugin.sid);
        let f = plugin.frame(v);
        link.receive(&f, 80.0);
        // Review 7: a crossfade's 1.0 -> 0.0 pair must reach the plugin as two positions.
        let x = CtlTarget::Executor { ex: 202, el: "fader" };
        link.control_event("mtouch", "Strip2", x, CtlKind::Touch { down: true }, 80.0);
        link.control_event("mtouch", "Strip2", x, CtlKind::Abs { v: 1.0, takeover: false }, 80.001);
        link.control_event("mtouch", "Strip2", x, CtlKind::Abs { v: 0.0, takeover: false }, 80.002);
        assert_eq!(link.queued_motion(), 2);
        assert_eq!(link.stats.ctl_coalesced, 0);
        link.tick(80.003);
        let c: Vec<Value> = plugin.decode(link.take_outgoing());
        let c = ctl_packets(&c);
        assert_eq!(c.len(), 2, "touch down went out at once; the first position now, the second waits for the rate interval: {c:?}");
        assert_eq!(c[1]["v"], 1.0);
        link.tick(80.01);
        let c2: Vec<Value> = plugin.decode(link.take_outgoing());
        let c2 = ctl_packets(&c2);
        assert_eq!(c2.len(), 1);
        assert_eq!(c2[0]["v"], 0.0);
        // Temp keeps its positions too; a Master merges; an executor the context does not list is kept whole.
        let t = CtlTarget::Executor { ex: 203, el: "fader" };
        link.control_event("mtouch", "Strip3", t, CtlKind::Abs { v: 0.0, takeover: false }, 80.1);
        link.control_event("mtouch", "Strip3", t, CtlKind::Abs { v: 0.3, takeover: false }, 80.101);
        let m = CtlTarget::Executor { ex: 201, el: "fader" };
        link.control_event("mtouch", "Strip1", m, CtlKind::Abs { v: 0.1, takeover: false }, 80.1);
        link.control_event("mtouch", "Strip1", m, CtlKind::Abs { v: 0.2, takeover: false }, 80.101);
        let u = CtlTarget::Executor { ex: 299, el: "fader" };
        link.control_event("mtouch", "Strip9", u, CtlKind::Abs { v: 0.1, takeover: false }, 80.1);
        link.control_event("mtouch", "Strip9", u, CtlKind::Abs { v: 0.2, takeover: false }, 80.101);
        assert_eq!(link.queued_motion(), 5);
        assert_eq!(link.stats.ctl_coalesced, 1);
        // A slot target merges (an attribute's value is stateless).
        link.control_event("mtouch", "StripS", slot(1), CtlKind::Abs { v: 0.1, takeover: false }, 80.2);
        link.control_event("mtouch", "StripS", slot(1), CtlKind::Abs { v: 0.9, takeover: false }, 80.201);
        assert_eq!(link.stats.ctl_coalesced, 2);
        // The per-control queue is bounded: beyond motion_per_control the oldest position of that control is dropped and counted.
        let mut link2 = Link::new(Config { id: "nxk-test".into(), motion_per_control: 3, ..Config::default() }, Key::from_hex(&"0123456789abcdef".repeat(4)).unwrap(), 90.0);
        let mut plugin2 = FakePlugin::new(Key::from_hex(&"0123456789abcdef".repeat(4)).unwrap());
        link2.tick(90.0);
        let hello = plugin2.decode(link2.take_outgoing());
        let nonce = hello[0]["nonce"].as_str().unwrap().to_string();
        let w = plugin2.welcome(&nonce);
        link2.receive(&w, 90.01);
        let mut v2 = json!({"t":"context","gen":plugin2.generation,"epoch":1,"known":1,"cg":1,"enc":{},"slots":[],"page":1,"pool":"Default","display":1,"ex":[{"n":202,"empty":0,"tgt":1,"fd":"X","tok":"FaderX"}]});
        v2["sid"] = json!(plugin2.sid);
        let f2 = plugin2.frame(v2);
        link2.receive(&f2, 90.02);
        for i in 0..5 {
            link2.control_event("mtouch", "Strip2", x, CtlKind::Abs { v: i as f64 / 10.0, takeover: false }, 90.1 + i as f64 * 0.0001);
        }
        assert_eq!(link2.queued_motion(), 3);
        assert_eq!(link2.stats.ctl_overflow, 2);
    }
}

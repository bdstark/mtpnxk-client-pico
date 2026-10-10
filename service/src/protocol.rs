//! Packet types of the surface protocol (docs/surface-protocol.md). Serialisation is serde_json;
//! the plugin side parses the same shapes.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const PROTOCOL_VERSION: u32 = 1;

/// Packets the service sends. `sid`/`seq` are filled in by the link for everything but `hello`.
#[derive(Debug, Serialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum ToPlugin<'a> {
    Hello {
        v: u32,
        id: &'a str,
        #[serde(rename = "gen")]
        generation: u32,
        nonce: &'a str,
        surface: &'a str,
        fw: &'a str,
        held: Vec<&'a str>,
    },
    Key {
        sid: &'a str,
        seq: u64,
        ev: u64,
        k: &'a str,
        d: u8,
    },
    /// KB-18: a continuous-control event. `k` is rel | abs | touch | btn; `es` the per-device event
    /// sequence (gaps are packet loss the plugin reports, never replays); `cg` the binding generation the
    /// event was produced against (absent only for a release); `gs` the gesture id.
    Ctl {
        sid: &'a str,
        seq: u64,
        ev: u64,
        k: &'a str,
        dev: &'a str,
        c: &'a str,
        es: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        cg: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        gs: Option<u64>,
        tgt: CtlTarget,
        #[serde(skip_serializing_if = "Option::is_none")]
        dx: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        v: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        d: Option<u8>,
        #[serde(skip_serializing_if = "Option::is_none")]
        fine: Option<u8>,
        /// KB-20: 1 on a position the surface marks as a deliberate takeover (strip key held at touch-down).
        #[serde(skip_serializing_if = "Option::is_none")]
        tk: Option<u8>,
    },
    Hb {
        sid: &'a str,
        seq: u64,
        held: Vec<&'a str>,
    },
    Bye {
        sid: &'a str,
        seq: u64,
    },
}

/// What a control event means on the console: an encoder slot of the bound display, or an element of
/// a bound executor. Serialised as `{"slot":n}` or `{"ex":n,"el":"fader"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(untagged)]
pub enum CtlTarget {
    Slot { slot: u8 },
    Executor { ex: u32, el: &'static str },
}

/// A welcome's key report: sorted lists of surface key names.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct KeyReport {
    #[serde(default)]
    pub ok: Vec<String>,
    #[serde(default)]
    pub unsupported: Vec<String>,
}

/// Packets the plugin sends.
#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum FromPlugin {
    Welcome {
        v: u32,
        sid: String,
        nonce: String,
        #[serde(rename = "gen")]
        generation: String,
        lease: u64,
        hb: u64,
        #[serde(default)]
        keys: KeyReport,
        #[serde(default)]
        modules: BTreeMap<String, String>,
        #[serde(default)]
        console: BTreeMap<String, Value>,
        #[serde(default)]
        input: Option<String>,
        #[serde(default)]
        plugin: Option<String>,
        #[serde(default)]
        epoch: Option<u64>,
        /// KB-18: 1 when the plugin admits `ctl` events (control=fake), with the backend's name.
        #[serde(default)]
        control: Option<u8>,
        #[serde(default, rename = "controlBackend")]
        control_backend: Option<String>,
        #[serde(default)]
        seq: u64,
    },
    Ack {
        sid: String,
        seq: u64,
        ev: u64,
        ok: u8,
        #[serde(default)]
        code: Option<String>,
        #[serde(default)]
        why: Option<String>,
        #[serde(default)]
        hold: Option<String>,
        #[serde(default)]
        dup: Option<u8>,
        #[serde(default)]
        noop: Option<u8>,
        #[serde(default)]
        duplicate: Option<u8>,
        /// KB-18 fields of a `ctl` ack: loss the plugin reported for the event's device, whether the
        /// delta was coalesced, the current binding generation on a stale-generation refusal.
        #[serde(default)]
        lost: Option<u64>,
        #[serde(default)]
        coalesced: Option<u8>,
        #[serde(default)]
        cg: Option<u64>,
    },
    Hb {
        sid: String,
        seq: u64,
        #[serde(default)]
        held: Vec<String>,
        #[serde(default)]
        unsynced: Vec<String>,
        #[serde(default)]
        resynced: Option<u8>,
        #[serde(default)]
        reconciled: Option<u32>,
        #[serde(default, rename = "gen")]
        generation: Option<String>,
    },
    State {
        sid: String,
        seq: u64,
        #[serde(rename = "gen")]
        generation: String,
        epoch: u64,
        full: u8,
        #[serde(default)]
        s: BTreeMap<String, Value>,
    },
    /// KB-17: the plugin's control-context message (what each encoder slot and executor would operate),
    /// a compact copy of the vendored feedback module's `contextSnapshot`. `known` is 0 while a part of it
    /// is unobserved on the plugin side (then `cg`, the binding generation, is absent). The service keeps
    /// it as data for later binding work and reconstructs no console semantics from it.
    Context {
        sid: String,
        seq: u64,
        #[serde(rename = "gen")]
        generation: String,
        epoch: u64,
        #[serde(default)]
        known: u8,
        #[serde(default)]
        cg: Option<u64>,
        #[serde(flatten)]
        rest: BTreeMap<String, Value>,
    },
    Err {
        #[serde(default)]
        sid: Option<String>,
        e: String,
        #[serde(default, rename = "gen")]
        generation: Option<String>,
        #[serde(default)]
        seq: u64,
    },
    Effect {
        sid: String,
        seq: u64,
        ev: u64,
        ms: i64,
        #[serde(default)]
        frames: i64,
        #[serde(default)]
        why: Option<String>,
    },
}

/// A state item as the surface renders it: known booleans and integers, or unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tri {
    Off,
    On,
    Int(i64),
    Unknown,
}

impl Tri {
    pub fn from_value(v: &Value) -> Tri {
        match v {
            Value::Number(n) => match n.as_i64() {
                Some(0) => Tri::Off,
                Some(1) => Tri::On,
                Some(i) => Tri::Int(i),
                None => Tri::Unknown,
            },
            _ => Tri::Unknown,
        }
    }
}

/// The `pending` item: a keyword, none, or unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    Word(String),
    None,
    Unknown,
}

impl Pending {
    pub fn from_value(v: &Value) -> Pending {
        match v {
            Value::String(s) if s == "?" => Pending::Unknown,
            Value::String(s) if s.is_empty() => Pending::None,
            Value::String(s) => Pending::Word(s.clone()),
            _ => Pending::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_packets_parse() {
        let w: FromPlugin = serde_json::from_str(r#"{"t":"welcome","v":1,"sid":"abcd","nonce":"n","gen":"1-00000001","lease":2000,"hb":250,"keys":{"ok":["Record"],"unsupported":["Bank"]},"modules":{"gma3_mcp_hardkeys":"0.5.0"},"seq":1}"#).unwrap();
        match w {
            FromPlugin::Welcome { sid, keys, lease, .. } => {
                assert_eq!(sid, "abcd");
                assert_eq!(lease, 2000);
                assert_eq!(keys.ok, vec!["Record"]);
            }
            _ => panic!("not a welcome"),
        }
        let s: FromPlugin = serde_json::from_str(r#"{"t":"state","sid":"a","seq":3,"gen":"1-1","epoch":1,"full":1,"s":{"blind":0,"highlight":1,"page":3,"pending":"store","freeze":"?"}}"#).unwrap();
        match s {
            FromPlugin::State { s, .. } => {
                assert_eq!(Tri::from_value(&s["blind"]), Tri::Off);
                assert_eq!(Tri::from_value(&s["highlight"]), Tri::On);
                assert_eq!(Tri::from_value(&s["page"]), Tri::Int(3));
                assert_eq!(Tri::from_value(&s["freeze"]), Tri::Unknown);
                assert_eq!(Pending::from_value(&s["pending"]), Pending::Word("store".into()));
            }
            _ => panic!("not a state"),
        }
        let c: FromPlugin = serde_json::from_str(r#"{"t":"context","sid":"a","seq":4,"gen":"1-1","epoch":1,"known":1,"cg":3,"display":1,"pool":"Default","page":1,"enc":{"bank":4,"bankName":"Color","page":1,"pageName":"RGB","ctx":"Default","attr":1},"slots":[{"n":1,"kind":"attribute","name":"ColorRGB_R","label":"R","unit":"ColorComponent","readout":"Percent","res":"Coarse","layer":"Absolute","cf":"","avail":"available","val":"value","abs":50}],"sel":1,"ex":[{"n":191,"empty":0,"tgt":1,"cls":"Sequence","name":"Main","kp":"Temp","ku":"","fd":"Master","lvl":100,"tok":"FaderMaster","act":0}]}"#).unwrap();
        match c {
            FromPlugin::Context { known, cg, rest, .. } => {
                assert_eq!(known, 1);
                assert_eq!(cg, Some(3));
                assert_eq!(rest["enc"]["bankName"], "Color");
                assert_eq!(rest["slots"][0]["name"], "ColorRGB_R");
                assert_eq!(rest["ex"][0]["tgt"], 1);
            }
            _ => panic!("not a context"),
        }
        let u: FromPlugin = serde_json::from_str(r#"{"t":"context","sid":"a","seq":5,"gen":"1-1","epoch":1,"known":0,"enc":{"why":"not observed"},"slots":[],"ex":[]}"#).unwrap();
        match u {
            FromPlugin::Context { known, cg, .. } => { assert_eq!(known, 0); assert_eq!(cg, None); }
            _ => panic!("not a context"),
        }
    }

    #[test]
    fn service_packets_serialise_with_type_tag() {
        let k = ToPlugin::Key { sid: "s", seq: 2, ev: 7, k: "Record", d: 1 };
        let text = serde_json::to_string(&k).unwrap();
        assert!(text.contains(r#""t":"key""#) && text.contains(r#""ev":7"#));
        let c = ToPlugin::Ctl { sid: "s", seq: 3, ev: 8, k: "rel", dev: "nxk", c: "Rotary1", es: 1, cg: Some(4), gs: Some(2), tgt: CtlTarget::Slot { slot: 1 }, dx: Some(-3), v: None, d: None, fine: Some(1), tk: None };
        let text = serde_json::to_string(&c).unwrap();
        assert!(text.contains(r#""t":"ctl""#) && text.contains(r#""tgt":{"slot":1}"#) && text.contains(r#""dx":-3"#) && !text.contains(r#""v""#), "{text}");
        let t = ToPlugin::Ctl { sid: "s", seq: 4, ev: 9, k: "touch", dev: "mtouch", c: "Strip1", es: 2, cg: None, gs: Some(3), tgt: CtlTarget::Executor { ex: 201, el: "fader" }, dx: None, v: None, d: Some(0), fine: None, tk: None };
        let text = serde_json::to_string(&t).unwrap();
        assert!(text.contains(r#""tgt":{"ex":201,"el":"fader"}"#) && text.contains(r#""d":0"#) && !text.contains(r#""cg""#), "{text}");
        let a: FromPlugin = serde_json::from_str(r#"{"t":"ack","sid":"s","seq":5,"ev":8,"ok":1,"lost":2,"coalesced":1}"#).unwrap();
        match a { FromPlugin::Ack { lost, coalesced, .. } => { assert_eq!(lost, Some(2)); assert_eq!(coalesced, Some(1)); } _ => panic!() }
        let w: FromPlugin = serde_json::from_str(r#"{"t":"welcome","v":1,"sid":"a","nonce":"n","gen":"1-1","lease":2000,"hb":250,"control":1,"controlBackend":"fake","seq":0}"#).unwrap();
        match w { FromPlugin::Welcome { control, control_backend, .. } => { assert_eq!(control, Some(1)); assert_eq!(control_backend.as_deref(), Some("fake")); } _ => panic!() }
    }
}

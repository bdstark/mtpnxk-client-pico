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
    Wheel {
        sid: &'a str,
        seq: u64,
        ev: u64,
        w: u8,
        dx: i32,
        bank: u8,
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
    }

    #[test]
    fn service_packets_serialise_with_type_tag() {
        let k = ToPlugin::Key { sid: "s", seq: 2, ev: 7, k: "Record", d: 1 };
        let text = serde_json::to_string(&k).unwrap();
        assert!(text.contains(r#""t":"key""#) && text.contains(r#""ev":7"#));
    }
}

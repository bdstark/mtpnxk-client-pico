//! Console state and local key state onto NX-K LEDs (docs/ma3-feedback.md "NX-K map",
//! docs/surface-protocol.md section 5). Pure: produces the LED writes that changed.

use crate::link::ConsoleView;
use crate::nxk::{self, LED_BLINK, LED_OFF, LED_ON};
use crate::protocol::{Pending, Tri};
use std::collections::{BTreeMap, BTreeSet};

/// Keyword keys: the LED follows `pending == word`.
const KEYWORD_KEYS: &[(&str, &str)] = &[
    ("Record", "store"),
    ("Update", "update"),
    ("Edit", "edit"),
    ("Copy", "copy"),
    ("Move", "move"),
    ("Delete", "delete"),
    ("Load", "load"),
    ("Cue", "cue"),
    ("Group", "group"),
    ("Macro", "macro"),
    ("Fade", "fade"),
    ("Delay", "delay"),
];

/// Keys whose LED echoes the physical hold (no console state exists for them).
const ECHO_KEYS: &[&str] = &["Clear", "Undo", "Next", "Last", "Menu", "Snap Shot"];

/// Local fallback for pending keywords while unpaired: lit on press, cleared by Enter, Clear,
/// another keyword, or after this many seconds.
const LOCAL_PENDING_TIMEOUT: f64 = 10.0;

pub struct Renderer {
    last: BTreeMap<u16, u16>,
    local_pending: Option<(&'static str, f64)>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer { last: BTreeMap::new(), local_pending: None }
    }

    /// Feeds a physical key event to the local fallback logic.
    pub fn key_event(&mut self, name: &'static str, down: bool, now: f64) {
        if !down {
            return;
        }
        if KEYWORD_KEYS.iter().any(|(k, _)| *k == name) {
            self.local_pending = Some((name, now));
        } else if name == "Enter" || name == "Clear" {
            self.local_pending = None;
        }
    }

    /// Forgets the last written values so every LED is rewritten (after a device reconnect).
    pub fn invalidate(&mut self) {
        self.last.clear();
    }

    /// Computes the LED values and returns only those that differ from the last call.
    pub fn render(&mut self, console: &ConsoleView<'_>, held: &BTreeSet<&'static str>, now: f64) -> Vec<(u16, u16)> {
        let mut want: BTreeMap<u16, u16> = BTreeMap::new();
        fn set(want: &mut BTreeMap<u16, u16>, name: &str, v: u16) {
            if let Some(id) = nxk::button_id(name) {
                want.insert(id, v);
            }
        }
        let items = console.items;
        let tri = |k: &str| items.and_then(|m| m.get(k)).map(Tri::from_value).unwrap_or(Tri::Unknown);
        let pending = items.and_then(|m| m.get("pending")).map(Pending::from_value).unwrap_or(Pending::Unknown);

        // Keyword keys: plugin value when fresh; local fallback while not paired.
        if let Some((name, at)) = self.local_pending {
            if now - at > LOCAL_PENDING_TIMEOUT {
                self.local_pending = None;
            }
            let _ = name;
        }
        for (name, word) in KEYWORD_KEYS {
            let v = if console.fresh {
                if pending == Pending::Word(word.to_string()) { LED_ON } else { LED_OFF }
            } else if !console.paired {
                match self.local_pending {
                    Some((n, _)) if n == *name => LED_ON,
                    _ => LED_OFF,
                }
            } else {
                LED_OFF // paired but unknown
            };
            set(&mut want, name, v);
        }
        // Modes.
        set(&mut want, "HighLight", if tri("highlight") == Tri::On { LED_BLINK } else { LED_OFF });
        set(&mut want, "Preview", if tri("preview") == Tri::On { LED_ON } else { LED_OFF });
        // Echo keys and Bank follow the physical hold.
        for name in ECHO_KEYS {
            set(&mut want, name, if held.contains(name) { LED_ON } else { LED_OFF });
        }
        let bank = held.contains("Bank");
        set(&mut want, "Bank", if bank { LED_ON } else { LED_OFF });
        for id in nxk::ENCODER_LEDS {
            want.insert(id, if bank { LED_ON } else { LED_OFF });
        }
        // Link: off when paired and fresh; blink otherwise (unpaired, link down, or stale state).
        want.insert(nxk::LINK_LED, if console.fresh { LED_OFF } else { LED_BLINK });
        set(&mut want, "Swap Prog", LED_OFF);

        let mut writes = Vec::new();
        for (id, v) in want {
            if self.last.get(&id) != Some(&v) {
                self.last.insert(id, v);
                writes.push((id, v));
            }
        }
        writes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn view<'a>(items: Option<&'a BTreeMap<String, serde_json::Value>>, paired: bool) -> ConsoleView<'a> {
        ConsoleView { fresh: items.is_some(), paired, link_up: paired, items }
    }

    fn items(v: serde_json::Value) -> BTreeMap<String, serde_json::Value> {
        serde_json::from_value(v).unwrap()
    }

    fn val(writes: &[(u16, u16)], name: &str) -> Option<u16> {
        let id = nxk::button_id(name).unwrap();
        writes.iter().find(|(i, _)| *i == id).map(|(_, v)| *v)
    }

    #[test]
    fn unknown_is_off_and_link_blinks_until_fresh() {
        let mut r = Renderer::new();
        let held = BTreeSet::new();
        let w = r.render(&view(None, false), &held, 0.0);
        assert_eq!(w.iter().find(|(i, _)| *i == nxk::LINK_LED).unwrap().1, LED_BLINK);
        assert_eq!(val(&w, "Record"), Some(LED_OFF));
        assert_eq!(val(&w, "HighLight"), Some(LED_OFF));
        let m = items(json!({"highlight":1,"preview":"?","pending":"store"}));
        let w = r.render(&view(Some(&m), true), &held, 1.0);
        assert_eq!(val(&w, "Record"), Some(LED_ON));
        assert_eq!(val(&w, "HighLight"), Some(LED_BLINK));
        assert_eq!(w.iter().find(|(i, _)| *i == nxk::LINK_LED).unwrap().1, LED_OFF);
        assert_eq!(val(&w, "Preview"), None, "unchanged LEDs are not rewritten");
        // Pending moves to edit: Record off, Edit on, nothing else touched.
        let m = items(json!({"highlight":1,"preview":"?","pending":"edit"}));
        let w = r.render(&view(Some(&m), true), &held, 1.1);
        assert_eq!(w.len(), 2);
        assert_eq!(val(&w, "Record"), Some(LED_OFF));
        assert_eq!(val(&w, "Edit"), Some(LED_ON));
        // Stale: everything unknown again, link blinks.
        let w = r.render(&view(None, true), &held, 3.0);
        assert_eq!(val(&w, "Edit"), Some(LED_OFF));
        assert_eq!(w.iter().find(|(i, _)| *i == nxk::LINK_LED).unwrap().1, LED_BLINK);
    }

    #[test]
    fn echo_bank_and_local_fallback() {
        let mut r = Renderer::new();
        let mut held = BTreeSet::new();
        held.insert("Clear");
        held.insert("Bank");
        let w = r.render(&view(None, false), &held, 0.0);
        assert_eq!(val(&w, "Clear"), Some(LED_ON));
        assert_eq!(val(&w, "Bank"), Some(LED_ON));
        assert!(nxk::ENCODER_LEDS.iter().all(|id| w.iter().any(|(i, v)| i == id && *v == LED_ON)));
        // Unpaired: a keyword press lights locally, Enter clears it, 10 s clears it.
        r.key_event("Record", true, 1.0);
        let w = r.render(&view(None, false), &BTreeSet::new(), 1.0);
        assert_eq!(val(&w, "Record"), Some(LED_ON));
        r.key_event("Enter", true, 1.5);
        let w = r.render(&view(None, false), &BTreeSet::new(), 1.5);
        assert_eq!(val(&w, "Record"), Some(LED_OFF));
        r.key_event("Edit", true, 2.0);
        r.render(&view(None, false), &BTreeSet::new(), 2.0);
        let w = r.render(&view(None, false), &BTreeSet::new(), 13.0);
        assert_eq!(val(&w, "Edit"), Some(LED_OFF));
        // Paired but unknown: the local fallback is not used (the plugin's value wins or stays off).
        r.key_event("Edit", true, 14.0);
        let w = r.render(&view(None, true), &BTreeSet::new(), 14.0);
        assert_eq!(val(&w, "Edit"), None);
    }
}

//! Obsidian / Elation NX-K keypad: packet decoder and LED values. Ported from the archived Pico
//! firmware (`legacy/pico/src/nxk_decode.c`), itself a port of avrsvc `cmd/nxk/decode.go`, and
//! verified on hardware on 2026-10-08.

pub mod usb;

pub const VID: u16 = 0x11be;
pub const PID: u16 = 0xe102;
pub const INTERFACE: u8 = 0;
pub const ALT_SETTING: u8 = 1;
pub const ENDPOINT_IN: u8 = 0x82;
pub const PACKET_MAX: usize = 64;

/// LED state: the wValue of vendor request 0x80 (bench survey 2026-10-08). Every NX-K LED is
/// single-colour: bit 0 on, bit 4 blink (with bit 0), bit 5 forces off.
pub const LED_OFF: u16 = 0x0000;
pub const LED_ON: u16 = 0x0001;
pub const LED_BLINK: u16 = 0x0011;

/// Control address: group in the high byte, control in the low byte. It is what the keypad reports
/// and what the LED write takes as wIndex.
pub const fn control_id(group: u8, control: u8) -> u16 {
    ((group as u16) << 8) | control as u16
}

pub const LINK_LED: u16 = control_id(97, 8);
pub const ENCODER_LEDS: [u16; 4] = [0x5901, 0x5911, 0x5921, 0x5931];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    KeyDown { name: &'static str, id: u16 },
    KeyUp { name: &'static str, id: u16 },
    /// Encoder pushed.
    PressDown { wheel: u8, id: u16 },
    PressUp { wheel: u8, id: u16 },
    Rotate { wheel: u8, delta: i32, id: u16 },
    Unknown { bytes: Vec<u8> },
}

struct Button {
    event_class: u8,
    group: u8,
    control: u8,
    name: &'static str,
}

const BUTTONS: &[Button] = &[
    Button { event_class: 66, group: 82, control: 0, name: "0" },
    Button { event_class: 66, group: 82, control: 1, name: "1" },
    Button { event_class: 66, group: 82, control: 2, name: "2" },
    Button { event_class: 66, group: 82, control: 3, name: "3" },
    Button { event_class: 66, group: 82, control: 4, name: "4" },
    Button { event_class: 66, group: 82, control: 5, name: "5" },
    Button { event_class: 66, group: 82, control: 6, name: "6" },
    Button { event_class: 66, group: 82, control: 7, name: "7" },
    Button { event_class: 66, group: 82, control: 8, name: "8" },
    Button { event_class: 66, group: 82, control: 9, name: "9" },
    Button { event_class: 66, group: 82, control: 18, name: "." },
    Button { event_class: 66, group: 82, control: 19, name: "Enter" },
    Button { event_class: 66, group: 82, control: 20, name: "/" },
    Button { event_class: 66, group: 82, control: 16, name: "-" },
    Button { event_class: 66, group: 82, control: 17, name: "+" },
    Button { event_class: 2, group: 82, control: 21, name: "Back" },
    Button { event_class: 66, group: 83, control: 2, name: "Thru" },
    Button { event_class: 66, group: 83, control: 1, name: "Full" },
    Button { event_class: 66, group: 82, control: 22, name: "@" },
    Button { event_class: 2, group: 84, control: 19, name: "Cue" },
    Button { event_class: 2, group: 84, control: 18, name: "Group" },
    Button { event_class: 2, group: 84, control: 17, name: "Load" },
    Button { event_class: 2, group: 84, control: 2, name: "Update" },
    Button { event_class: 2, group: 84, control: 1, name: "Record" },
    Button { event_class: 2, group: 81, control: 1, name: "Edit" },
    Button { event_class: 2, group: 81, control: 2, name: "Undo" },
    Button { event_class: 2, group: 81, control: 3, name: "Clear" },
    Button { event_class: 2, group: 81, control: 4, name: "Copy" },
    Button { event_class: 2, group: 81, control: 6, name: "Move" },
    Button { event_class: 2, group: 81, control: 7, name: "Delete" },
    Button { event_class: 2, group: 32, control: 3, name: "Menu" },
    Button { event_class: 2, group: 32, control: 1, name: "Macro" },
    Button { event_class: 2, group: 67, control: 49, name: "Snap Shot" },
    Button { event_class: 2, group: 67, control: 50, name: "Bank" },
    Button { event_class: 2, group: 32, control: 2, name: "Preview" },
    Button { event_class: 2, group: 96, control: 1, name: "HighLight" },
    Button { event_class: 2, group: 67, control: 33, name: "Fade" },
    Button { event_class: 2, group: 67, control: 34, name: "Delay" },
    Button { event_class: 2, group: 100, control: 17, name: "Swap Prog" },
    Button { event_class: 2, group: 97, control: 8, name: "Link" },
    Button { event_class: 2, group: 100, control: 1, name: "Last" },
    Button { event_class: 2, group: 100, control: 2, name: "Next" },
];

/// Control id of a named button (for LED writes).
pub fn button_id(name: &str) -> Option<u16> {
    BUTTONS.iter().find(|b| b.name == name).map(|b| control_id(b.group, b.control))
}

/// Every button name with its control id.
pub fn buttons() -> impl Iterator<Item = (&'static str, u16)> {
    BUTTONS.iter().map(|b| (b.name, control_id(b.group, b.control)))
}

fn encoder_press_wheel(control: u8) -> Option<u8> {
    match control {
        1 => Some(1),
        17 => Some(2),
        33 => Some(3),
        49 => Some(4),
        _ => None,
    }
}

fn encoder_rotate_wheel(control: u8) -> Option<u8> {
    match control {
        2 => Some(1),
        18 => Some(2),
        34 => Some(3),
        50 => Some(4),
        _ => None,
    }
}

/// Decodes one interrupt IN packet. Empty packets (idle polls) return `None`.
pub fn decode(data: &[u8]) -> Option<Event> {
    if data.is_empty() {
        return None;
    }
    if data.len() == 5 && data[0] == 1 {
        let (event_class, control, group, state) = (data[1], data[2], data[3], data[4]);
        let id = control_id(group, control);
        if event_class == 2 && group == 89 {
            if let Some(wheel) = encoder_press_wheel(control) {
                return Some(if state != 0 { Event::PressDown { wheel, id } } else { Event::PressUp { wheel, id } });
            }
        }
        for b in BUTTONS {
            if b.event_class == event_class && b.group == group && b.control == control {
                return Some(if state != 0 { Event::KeyDown { name: b.name, id } } else { Event::KeyUp { name: b.name, id } });
            }
        }
        return Some(Event::Unknown { bytes: data.to_vec() });
    }
    if data.len() == 7 && data[0] == 2 && data[1] == 66 && data[3] == 89 {
        let id = control_id(data[3], data[2]);
        if let Some(wheel) = encoder_rotate_wheel(data[2]) {
            let delta = i16::from_le_bytes([data[4], data[5]]) as i32;
            return Some(Event::Rotate { wheel, delta, id });
        }
        return Some(Event::Unknown { bytes: data.to_vec() });
    }
    Some(Event::Unknown { bytes: data.to_vec() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_keys_encoders_and_idle() {
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&[1, 2, 1, 84, 1]), Some(Event::KeyDown { name: "Record", id: 0x5401 }));
        assert_eq!(decode(&[1, 2, 1, 84, 0]), Some(Event::KeyUp { name: "Record", id: 0x5401 }));
        assert_eq!(decode(&[1, 66, 5, 82, 1]), Some(Event::KeyDown { name: "5", id: 0x5205 }));
        assert_eq!(decode(&[1, 2, 1, 96, 1]), Some(Event::KeyDown { name: "HighLight", id: 0x6001 }));
        assert_eq!(decode(&[1, 2, 17, 89, 1]), Some(Event::PressDown { wheel: 2, id: 0x5911 }));
        assert_eq!(decode(&[2, 66, 34, 89, 0xfe, 0xff, 0]), Some(Event::Rotate { wheel: 3, delta: -2, id: 0x5922 }));
        assert!(matches!(decode(&[9, 9, 9]), Some(Event::Unknown { .. })));
    }

    #[test]
    fn button_ids_match_the_led_survey() {
        assert_eq!(button_id("Record"), Some(0x5401));
        assert_eq!(button_id("Edit"), Some(0x5101));
        assert_eq!(button_id("Last"), Some(0x6401));
        assert_eq!(button_id("Next"), Some(0x6402));
        assert_eq!(button_id("Link"), Some(LINK_LED));
        assert_eq!(buttons().count(), 42);
    }
}

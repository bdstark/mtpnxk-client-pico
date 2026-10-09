//! USB access to the NX-K through nusb (cross-platform, pure Rust). The device is a vendor-class
//! interface: interface 0 alternate setting 1 exposes interrupt IN 0x82; LEDs are written with
//! vendor control requests on EP0 (`bmRequestType 0x40, bRequest 0x80, wValue = state,
//! wIndex = control address`), as verified on hardware (legacy/pico/README.md "NX-K LEDs").
//!
//! Windows needs the WinUSB driver bound to the keypad (Zadig); macOS and Linux need nothing.

use super::{decode, Event, ALT_SETTING, ENDPOINT_IN, INTERFACE, PACKET_MAX, PID, VID};
use anyhow::{anyhow, Context, Result};
use nusb::transfer::{ControlOut, ControlType, In, Interrupt, Recipient};
use nusb::MaybeFuture;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

/// What the main loop needs from a keypad, real or simulated.
pub trait Surface {
    /// Events decoded since the last call. `Err` means the device is gone.
    fn poll(&mut self) -> Result<Vec<Event>>;
    /// Queues one LED write.
    fn led(&mut self, id: u16, value: u16);
    fn name(&self) -> String;
    /// A scripted surface reports when its script is finished; a real keypad never is.
    fn done(&self) -> bool {
        false
    }
}

pub struct UsbKeypad {
    events: Receiver<Event>,
    leds: Sender<(u16, u16)>,
    name: String,
    gone: bool,
}

pub fn list() -> Result<Vec<String>> {
    let devices = nusb::list_devices().wait().context("listing USB devices")?;
    Ok(devices
        .map(|d| {
            format!(
                "{:04x}:{:04x} {} {} {}",
                d.vendor_id(),
                d.product_id(),
                d.manufacturer_string().unwrap_or("-"),
                d.product_string().unwrap_or("-"),
                d.serial_number().unwrap_or("-")
            )
        })
        .collect())
}

impl UsbKeypad {
    pub fn open() -> Result<UsbKeypad> {
        let info = nusb::list_devices()
            .wait()
            .context("listing USB devices")?
            .find(|d| d.vendor_id() == VID && d.product_id() == PID)
            .ok_or_else(|| anyhow!("no NX-K ({VID:04x}:{PID:04x}) found; on Windows bind WinUSB to it with Zadig"))?;
        let name = format!("NX-K {}", info.serial_number().unwrap_or("(no serial)"));
        let device = info.open().wait().context("opening the NX-K")?;
        let interface = device.claim_interface(INTERFACE).wait().context("claiming interface 0 (is another program using the keypad?)")?;
        interface.set_alt_setting(ALT_SETTING).wait().context("selecting alternate setting 1")?;
        let mut ep = interface.endpoint::<Interrupt, In>(ENDPOINT_IN).context("opening interrupt IN 0x82")?;

        let (ev_tx, ev_rx) = mpsc::channel::<Event>();
        let (led_tx, led_rx) = mpsc::channel::<(u16, u16)>();

        // Reader: keeps four transfers in flight, decodes each completion, drops idle polls.
        thread::Builder::new()
            .name("nxk-read".into())
            .spawn(move || {
                loop {
                    while ep.pending() < 4 {
                        let buf = ep.allocate(PACKET_MAX);
                        ep.submit(buf);
                    }
                    let Some(c) = ep.wait_next_complete(Duration::from_millis(500)) else { continue };
                    if let Err(e) = c.status {
                        eprintln!("nxk: read failed: {e}; device gone?");
                        break;
                    }
                    let data = &c.buffer[..c.actual_len.min(c.buffer.len())];
                    if let Some(ev) = decode(data) {
                        if ev_tx.send(ev).is_err() {
                            break;
                        }
                    }
                }
            })
            .context("spawning the reader thread")?;

        // LED writer: one control request at a time, in order.
        let led_if = interface.clone();
        thread::Builder::new()
            .name("nxk-led".into())
            .spawn(move || {
                while let Ok((id, value)) = led_rx.recv() {
                    let r = led_if
                        .control_out(
                            ControlOut { control_type: ControlType::Vendor, recipient: Recipient::Device, request: 0x80, value, index: id, data: &[] },
                            Duration::from_millis(200),
                        )
                        .wait();
                    if let Err(e) = r {
                        eprintln!("nxk: LED write {id:04x} <- {value:04x} failed: {e}");
                    }
                }
            })
            .context("spawning the LED thread")?;

        Ok(UsbKeypad { events: ev_rx, leds: led_tx, name, gone: false })
    }
}

impl Surface for UsbKeypad {
    fn poll(&mut self) -> Result<Vec<Event>> {
        let mut out = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(ev) => out.push(ev),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.gone = true;
                    return Err(anyhow!("NX-K disconnected"));
                }
            }
        }
        Ok(out)
    }

    fn led(&mut self, id: u16, value: u16) {
        let _ = self.leds.send((id, value));
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

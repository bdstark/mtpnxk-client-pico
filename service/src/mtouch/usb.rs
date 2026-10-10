//! USB access to the M-Touch / M-Play through nusb, on the same code path as the NX-K
//! (`nxk::usb::claim_vendor_interface`): interface 0 alternate 1, interrupt IN 0x82 read by a
//! thread that hands every packet (idle zero-length ones included, so the consumer can count
//! them) to a channel as raw bytes; vendor control OUT requests written by a second thread from a
//! channel of [`Output`] items. Nothing here has run against hardware: see
//! `docs/mtouch-protocol-reuse.md` for the live qualification procedure.

use super::{Model, Output, ENDPOINT_IN, PACKET_MAX, VID};
use crate::nxk::usb::claim_vendor_interface;
use anyhow::{anyhow, Context, Result};
use nusb::transfer::{ControlOut, ControlType, In, Interrupt, Recipient};
use nusb::MaybeFuture;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

/// The EZ-USB firmware load / reset request range (protocol.md section 1). Never sent.
pub const FORBIDDEN_REQUESTS: std::ops::RangeInclusive<u8> = 0xA0..=0xAF;

pub struct UsbSurface {
    packets: Receiver<Vec<u8>>,
    outputs: Sender<Output>,
    model: Model,
    name: String,
}

/// Models present on the bus, in enumeration order.
pub fn attached() -> Result<Vec<Model>> {
    let devices = nusb::list_devices().wait().context("listing USB devices")?;
    Ok(devices.filter(|d| d.vendor_id() == VID).filter_map(|d| Model::from_pid(d.product_id())).collect())
}

impl UsbSurface {
    /// Opens the first M-Touch or M-Play found.
    pub fn open_any() -> Result<UsbSurface> {
        let model = attached()?.into_iter().next().ok_or_else(|| anyhow!("no M-Touch ({VID:04x}:{:04x}) or M-Play ({VID:04x}:{:04x}) found", Model::MTouch.pid(), Model::MPlay.pid()))?;
        Self::open_model(model)
    }

    pub fn open_model(model: Model) -> Result<UsbSurface> {
        let (name, interface) = claim_vendor_interface(VID, model.pid(), model.name())?;
        let mut ep = interface.endpoint::<Interrupt, In>(ENDPOINT_IN).context("opening interrupt IN 0x82")?;

        let (pk_tx, pk_rx) = mpsc::channel::<Vec<u8>>();
        let (out_tx, out_rx) = mpsc::channel::<Output>();

        // Reader: keeps four transfers in flight and forwards every completion as raw bytes.
        thread::Builder::new()
            .name("mtouch-read".into())
            .spawn(move || {
                loop {
                    while ep.pending() < 4 {
                        let buf = ep.allocate(PACKET_MAX);
                        ep.submit(buf);
                    }
                    let Some(c) = ep.wait_next_complete(Duration::from_millis(500)) else { continue };
                    if let Err(e) = c.status {
                        eprintln!("{}: read failed: {e}; device gone?", model.name());
                        break;
                    }
                    let data = &c.buffer[..c.actual_len.min(c.buffer.len())];
                    if pk_tx.send(data.to_vec()).is_err() {
                        break;
                    }
                }
            })
            .context("spawning the reader thread")?;

        // Writer: one vendor control OUT at a time, in order.
        let out_if = interface.clone();
        thread::Builder::new()
            .name("mtouch-write".into())
            .spawn(move || {
                while let Ok(out) = out_rx.recv() {
                    let request = out.request();
                    debug_assert!(!FORBIDDEN_REQUESTS.contains(&request), "vendor request {request:02x} is in the EZ-USB firmware range");
                    if FORBIDDEN_REQUESTS.contains(&request) {
                        eprintln!("{}: refusing vendor request {request:02x} (EZ-USB firmware range)", model.name());
                        continue;
                    }
                    let r = out_if
                        .control_out(
                            ControlOut { control_type: ControlType::Vendor, recipient: Recipient::Device, request, value: out.value(), index: out.index(), data: out.data() },
                            Duration::from_millis(200),
                        )
                        .wait();
                    if let Err(e) = r {
                        eprintln!("{}: write {request:02x} to {:04x} failed: {e}", model.name(), out.index());
                    }
                }
            })
            .context("spawning the writer thread")?;

        Ok(UsbSurface { packets: pk_rx, outputs: out_tx, model, name })
    }

    pub fn model(&self) -> Model {
        self.model
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Raw packets read since the last call (empty vectors are idle polls). `Err` means the
    /// device is gone.
    pub fn poll(&mut self) -> Result<Vec<Vec<u8>>> {
        let mut out = Vec::new();
        loop {
            match self.packets.try_recv() {
                Ok(p) => out.push(p),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Err(anyhow!("{} disconnected", self.model.name())),
            }
        }
        Ok(out)
    }

    /// Queues one output write.
    pub fn send(&self, out: Output) {
        let _ = self.outputs.send(out);
    }
}

//! Host game controllers. macOS uses one GameController service for input and feedback.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use miko_core::MikoResult;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use super::hid::PadEvent;

/// A controller connected to this machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Controller {
    /// Its index for `input.gamepad.connect {controller}`; stable while it
    /// stays connected.
    pub id: usize,
    pub name: String,
    /// Its USB vendor and product IDs, when the system reports them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<u16>,
    /// Device currently receiving this physical controller, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound_device: Option<String>,
    /// Power information reported by the active gamepad backend.
    pub power: Power,
    /// Controls exposed by the current mapping and their live values.
    pub controls: Vec<Control>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Power {
    Unknown,
    Wired,
    Available { percent: u8 },
    Charging { percent: u8 },
    Discharging { percent: u8 },
    Charged,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Control {
    pub name: String,
    pub value: f32,
}

/// The controller to use: `id`, or the first one.
pub fn pick(all: &[Controller], id: Option<usize>) -> MikoResult<Controller> {
    let found = match id {
        Some(id) => all.iter().find(|c| c.id == id),
        None => all.first(),
    };
    found.cloned().ok_or_else(|| {
        miko_core::MikoError::not_found(match (id, all.is_empty()) {
            (_, true) => "no game controller is connected to this computer".to_string(),
            (Some(id), false) => format!(
                "no game controller {id}; connected: {}",
                all.iter()
                    .map(|c| format!("{} ({})", c.id, c.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            (None, false) => String::new(),
        })
    })
}

#[cfg(target_os = "macos")]
#[path = "host_native.rs"]
mod native;
#[cfg(target_os = "macos")]
pub use native::{list, open, vibrate};

#[cfg(not(target_os = "macos"))]
pub use unsupported::{list, open, vibrate};

#[cfg(not(target_os = "macos"))]
mod unsupported {
    use super::*;

    fn refuse() -> miko_core::MikoError {
        miko_core::MikoError::usage("game controllers can only be forwarded from macOS for now")
    }

    pub fn list() -> MikoResult<Vec<Controller>> {
        Err(refuse())
    }

    pub fn vibrate(_id: usize, _source: String, _active: bool) -> MikoResult<()> {
        Err(refuse())
    }

    pub fn open(
        _id: Option<usize>,
        _device: String,
        _tx: mpsc::Sender<PadEvent>,
        _stop: Arc<AtomicBool>,
    ) -> MikoResult<watch::Receiver<Controller>> {
        Err(refuse())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(id: usize, name: &str) -> Controller {
        Controller {
            id,
            name: name.into(),
            vendor: None,
            product: None,
            bound_device: None,
            power: Power::Unknown,
            controls: Vec::new(),
        }
    }

    #[test]
    fn picks_the_first_or_the_one_asked_for() {
        let all = [pad(0, "Xbox Wireless Controller"), pad(2, "DualSense")];
        assert_eq!(pick(&all, None).unwrap().id, 0);
        assert_eq!(pick(&all, Some(2)).unwrap().name, "DualSense");
        let e = pick(&all, Some(1)).unwrap_err();
        assert_eq!(e.code, miko_core::ErrorCode::NotFound);
        assert!(
            e.message
                .contains("0 (Xbox Wireless Controller), 2 (DualSense)"),
            "{}",
            e.message
        );
        let e = pick(&[], None).unwrap_err();
        assert_eq!(
            e.message,
            "no game controller is connected to this computer"
        );
    }
}

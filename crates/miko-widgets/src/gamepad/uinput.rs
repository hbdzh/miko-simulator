//! Android uinput gamepad events and feedback-capable registration.

use serde_json::{Value, json};

/// Incrementally decodes the adjacent JSON objects emitted by Android uinput.
#[derive(Default)]
pub struct FeedbackDecoder {
    bytes: Vec<u8>,
}

impl FeedbackDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> miko_core::MikoResult<Vec<bool>> {
        self.bytes.extend_from_slice(chunk);
        if self.bytes.len() > 65_536 {
            return Err(miko_core::MikoError::unexpected(
                "gamepad feedback exceeds limit",
            ));
        }
        let mut stream = serde_json::Deserializer::from_slice(&self.bytes).into_iter::<Value>();
        let mut consumed = 0;
        let mut events = Vec::new();
        loop {
            match stream.next() {
                Some(Ok(value)) => {
                    consumed = stream.byte_offset();
                    if value["id"] == 1
                        && value["reason"] == "vibrating"
                        && let Some(status) = value["status"].as_i64()
                    {
                        events.push(status != 0);
                    }
                }
                Some(Err(error)) if error.is_eof() => break,
                Some(Err(error)) => {
                    return Err(miko_core::MikoError::unexpected(format!(
                        "invalid gamepad feedback: {error}"
                    )));
                }
                None => {
                    consumed = self.bytes.len();
                    break;
                }
            }
        }
        self.bytes.drain(..consumed);
        Ok(events)
    }
}

const BUTTONS: [(usize, u32); 11] = [
    (0, 304),
    (1, 305),
    (3, 307),
    (4, 308),
    (6, 310),
    (7, 311),
    (10, 314),
    (11, 315),
    (12, 316),
    (13, 317),
    (14, 318),
];

/// Register a virtual gamepad with FF_RUMBLE, using Android's uinput command.
pub fn register(name: &str) -> Value {
    let axis = |code: u32, min: i32, max: i32| {
        json!({
            "code": code, "info": {"value":0,"minimum":min,"maximum":max,"fuzz":0,"flat":0,"resolution":0}
        })
    };
    json!({
        "id":1, "command":"register", "name":name,
        "vid":super::hid::VENDOR, "pid":super::hid::PRODUCT, "bus":"usb",
        "configuration":[
            {"type":100,"data":[1,3,21]},
            {"type":101,"data":BUTTONS.iter().map(|(_,code)| *code).collect::<Vec<_>>()},
            {"type":103,"data":[0,1,2,3,4,5,16,17]},
            {"type":107,"data":[80]}
        ],
        "ff_effects_max":1,
        "abs_info":[axis(0,-32768,32767),axis(1,-32768,32767),axis(2,0,255),
                    axis(3,-32768,32767),axis(4,-32768,32767),axis(5,0,255),
                    axis(16,-1,1),axis(17,-1,1)]
    })
}

/// Translate the shared 13-byte gamepad state into a complete evdev frame.
pub fn report(report: &[u8; super::hid::REPORT_LEN]) -> Value {
    let bits = u16::from_le_bytes([report[0], report[1]]);
    let mut events = Vec::<i32>::new();
    for (bit, code) in BUTTONS {
        events.extend([1, code as i32, i32::from((bits & (1 << bit)) != 0)]);
    }
    for (offset, code) in [(2, 0), (4, 1), (6, 3), (8, 4)] {
        let value = i16::from_le_bytes([report[offset], report[offset + 1]]);
        events.extend([3, code, i32::from(value)]);
    }
    events.extend([3, 2, i32::from(report[10]), 3, 5, i32::from(report[11])]);
    let (x, y) = match report[12] & 0x0f {
        1 => (0, -1),
        2 => (1, -1),
        3 => (1, 0),
        4 => (1, 1),
        5 => (0, 1),
        6 => (-1, 1),
        7 => (-1, 0),
        8 => (-1, -1),
        _ => (0, 0),
    };
    events.extend([3, 16, x, 3, 17, y, 0, 0, 0]);
    json!({"id":1,"command":"inject","events":events})
}

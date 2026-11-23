//! Single macOS GameController input and haptics service.
use super::super::hid::{Axis, Button};
use super::*;
use miko_core::MikoError;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{OnceLock, mpsc as sync_mpsc};
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(25);
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
struct Binding {
    device: String,
    controller: watch::Sender<Controller>,
    events: mpsc::Sender<PadEvent>,
    stop: Arc<AtomicBool>,
}

fn bind(
    bindings: &mut HashMap<usize, Binding>,
    mut controller: Controller,
    device: String,
    events: mpsc::Sender<PadEvent>,
    stop: Arc<AtomicBool>,
) -> MikoResult<watch::Receiver<Controller>> {
    if let Some(binding) = bindings.get(&controller.id) {
        return Err(MikoError::busy(format!(
            "controller is already bound to {}",
            binding.device
        )));
    }
    if stop.load(Ordering::Relaxed) || events.is_closed() {
        return Err(miko_core::cancel::cancelled());
    }
    controller.bound_device = Some(device.clone());
    let id = controller.id;
    let (controller, snapshot) = watch::channel(controller);
    bindings.insert(
        id,
        Binding {
            device,
            controller,
            events,
            stop,
        },
    );
    Ok(snapshot)
}

fn deliver(
    bindings: &mut HashMap<usize, Binding>,
    id: usize,
    events: Vec<PadEvent>,
    disconnected: bool,
) {
    let mut remove = disconnected;
    if let Some(binding) = bindings.get(&id) {
        for event in events {
            if binding.events.try_send(event).is_err() {
                remove = true;
                break;
            }
        }
    }
    // Closing the channel also forces neutral release if it overflowed.
    if remove {
        bindings.remove(&id);
    }
}

enum Request {
    List(sync_mpsc::Sender<MikoResult<Vec<Controller>>>),
    Open {
        id: Option<usize>,
        device: String,
        events: mpsc::Sender<PadEvent>,
        stop: Arc<AtomicBool>,
        reply: sync_mpsc::Sender<MikoResult<watch::Receiver<Controller>>>,
    },
    Vibrate {
        id: usize,
        source: String,
        active: bool,
    },
}
struct Process {
    child: Child,
    input: ChildStdin,
    _file: tempfile::TempPath,
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn start() -> MikoResult<(Process, sync_mpsc::Receiver<Value>)> {
    let mut file = tempfile::Builder::new()
        .prefix("miko-gamepad-")
        .tempfile()
        .map_err(|e| MikoError::unexpected(format!("creating gamepad helper: {e}")))?;
    file.write_all(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/miko-gamepad-native"
    )))
    .map_err(|e| MikoError::unexpected(format!("writing gamepad helper: {e}")))?;
    file.as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o700))
        .map_err(|e| MikoError::unexpected(format!("setting gamepad helper permissions: {e}")))?;
    let file = file.into_temp_path();
    let mut child = Command::new(&*file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| MikoError::unexpected(format!("starting gamepad helper: {e}")))?;
    let input = child
        .stdin
        .take()
        .ok_or_else(|| MikoError::unexpected("gamepad helper has no input"))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| MikoError::unexpected("gamepad helper has no output"))?;
    let (sender, receiver) = sync_mpsc::channel::<Value>();
    std::thread::Builder::new()
        .name("miko-gamepad-output".into())
        .spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if let Ok(value) = serde_json::from_str(&line)
                    && sender.send(value).is_err()
                {
                    break;
                }
            }
        })
        .map_err(|e| MikoError::unexpected(format!("reading gamepad helper: {e}")))?;
    let mut process = Process {
        child,
        input,
        _file: file,
    };
    match receiver.recv_timeout(REPLY_TIMEOUT) {
        Ok(v) if v["type"] == "ready" => {
            send(&mut process, &json!({"type":"list"}))?;
            Ok((process, receiver))
        }
        _ => Err(MikoError::unexpected("gamepad helper did not become ready")),
    }
}
fn send(process: &mut Process, value: &Value) -> MikoResult<()> {
    writeln!(process.input, "{value}")
        .and_then(|()| process.input.flush())
        .map_err(|e| MikoError::unexpected(format!("sending gamepad command: {e}")))
}
static SERVICE: OnceLock<MikoResult<sync_mpsc::Sender<Request>>> = OnceLock::new();
fn service() -> MikoResult<sync_mpsc::Sender<Request>> {
    SERVICE
        .get_or_init(|| {
            let (requests, receiver) = sync_mpsc::channel();
            let (started, ready) = sync_mpsc::channel();
            std::thread::Builder::new()
                .name("miko-gamepad".into())
                .spawn(move || match start() {
                    Ok((process, events)) => {
                        let _ = started.send(Ok(()));
                        run(process, events, receiver);
                    }
                    Err(error) => {
                        let _ = started.send(Err(error));
                    }
                })
                .map_err(|e| MikoError::unexpected(format!("starting gamepad service: {e}")))?;
            ready
                .recv_timeout(REPLY_TIMEOUT)
                .map_err(|_| MikoError::unexpected("gamepad service did not start"))??;
            Ok(requests)
        })
        .clone()
}
fn map_event(control: &str, value: f32) -> Option<PadEvent> {
    let button = match control {
        "South" => Button::South,
        "East" => Button::East,
        "West" => Button::West,
        "North" => Button::North,
        "LeftTrigger" => Button::LeftBumper,
        "RightTrigger" => Button::RightBumper,
        "Select" => Button::Back,
        "Start" => Button::Start,
        "Mode" => Button::Guide,
        "LeftThumb" => Button::LeftStick,
        "RightThumb" => Button::RightStick,
        "DPadUp" => Button::Up,
        "DPadDown" => Button::Down,
        "DPadLeft" => Button::Left,
        "DPadRight" => Button::Right,
        "LeftTrigger2" => return Some(PadEvent::Axis(Axis::LeftTrigger, value)),
        "RightTrigger2" => return Some(PadEvent::Axis(Axis::RightTrigger, value)),
        "LeftStickX" => return Some(PadEvent::Axis(Axis::LeftX, value)),
        "LeftStickY" => return Some(PadEvent::Axis(Axis::LeftY, value)),
        "RightStickX" => return Some(PadEvent::Axis(Axis::RightX, value)),
        "RightStickY" => return Some(PadEvent::Axis(Axis::RightY, value)),
        _ => return None,
    };
    Some(PadEvent::Button(button, value > 0.5))
}
fn run(
    mut process: Process,
    events: sync_mpsc::Receiver<Value>,
    requests: sync_mpsc::Receiver<Request>,
) {
    let mut bindings: HashMap<usize, Binding> = HashMap::new();
    let mut vibration: HashMap<usize, HashSet<String>> = HashMap::new();
    let mut controllers: Vec<Controller> = match events.recv_timeout(REPLY_TIMEOUT) {
        Ok(value) => serde_json::from_value(value["controllers"].clone()).unwrap_or_default(),
        Err(_) => return,
    };
    let mut last_poll = Instant::now() - Duration::from_secs(1);
    loop {
        let removed: Vec<_> = bindings
            .iter()
            .filter(|(_, b)| b.stop.load(Ordering::Relaxed) || b.events.is_closed())
            .map(|(id, _)| *id)
            .collect();
        for id in removed {
            bindings.remove(&id);
            vibration.remove(&id);
            let _ = send(
                &mut process,
                &json!({"type":"vibrate","id":id,"active":false}),
            );
        }
        for _ in 0..64 {
            let Ok(request) = requests.try_recv() else {
                break;
            };
            match request {
                Request::List(reply) => {
                    let mut list = controllers.clone();
                    for c in &mut list {
                        c.bound_device = bindings.get(&c.id).map(|b| b.device.clone());
                    }
                    let _ = reply.send(Ok(list));
                }
                Request::Open {
                    id,
                    device,
                    events,
                    stop,
                    reply,
                } => {
                    let result = pick(&controllers, id)
                        .and_then(|c| bind(&mut bindings, c, device, events, stop));
                    if let Err(undelivered) = reply.send(result)
                        && let Ok(c) = undelivered.0
                    {
                        bindings.remove(&c.borrow().id);
                    }
                }
                Request::Vibrate { id, source, active } => {
                    if bindings.contains_key(&id) {
                        let sources = vibration.entry(id).or_default();
                        let previous = !sources.is_empty();
                        if active {
                            sources.insert(source);
                        } else {
                            sources.remove(&source);
                        }
                        let current = !sources.is_empty();
                        if previous != current {
                            let _ = send(
                                &mut process,
                                &json!({"type":"vibrate","id":id,"active":current}),
                            );
                        }
                    }
                }
            }
        }
        if last_poll.elapsed() >= Duration::from_millis(250) {
            if send(&mut process, &json!({"type":"list"})).is_err() {
                return;
            }
            last_poll = Instant::now();
        }
        match events.recv_timeout(TICK) {
            Ok(event) => match event["type"].as_str() {
                Some("controllers") => {
                    if let Ok(list) =
                        serde_json::from_value::<Vec<Controller>>(event["controllers"].clone())
                    {
                        controllers = list;
                    }
                }
                Some("event") => {
                    if let (Some(id), Some(control), Some(value)) = (
                        event["id"].as_u64(),
                        event["control"].as_str(),
                        event["value"].as_f64(),
                    ) && let Some(mapped) = map_event(control, value as f32)
                    {
                        let id = id as usize;
                        deliver(&mut bindings, id, vec![mapped], false);
                        if !bindings.contains_key(&id) && vibration.remove(&id).is_some() {
                            let _ = send(
                                &mut process,
                                &json!({"type":"vibrate","id":id,"active":false}),
                            );
                        }
                    }
                }
                Some("disconnected") => {
                    if let Some(id) = event["id"].as_u64() {
                        let id = id as usize;
                        let identity = event["vendor"].as_u64().zip(event["product"].as_u64());
                        if let Some(binding) = bindings.get(&id)
                            && let Some((vendor, product)) = identity
                        {
                            binding.controller.send_modify(|controller| {
                                controller.vendor = u16::try_from(vendor).ok();
                                controller.product = u16::try_from(product).ok();
                            });
                        }
                        let unique = identity.is_some() && event["ambiguous"] == false;
                        deliver(
                            &mut bindings,
                            id,
                            vec![if unique {
                                PadEvent::Disconnected
                            } else {
                                PadEvent::DisconnectedAmbiguous
                            }],
                            true,
                        );
                        vibration.remove(&id);
                        controllers.retain(|c| c.id != id);
                    }
                }
                Some("vibration") if event["ok"] == false => {
                    tracing::warn!(controller = %event["id"], reason = %event["message"], "physical gamepad vibration failed");
                }
                _ => {}
            },
            Err(sync_mpsc::RecvTimeoutError::Disconnected) => return,
            Err(sync_mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}
pub fn list() -> MikoResult<Vec<Controller>> {
    let (reply, result) = sync_mpsc::channel();
    service()?
        .send(Request::List(reply))
        .map_err(|_| MikoError::unexpected("gamepad service stopped"))?;
    result
        .recv_timeout(REPLY_TIMEOUT)
        .map_err(|_| MikoError::unexpected("gamepad list timed out"))?
}
pub fn open(
    id: Option<usize>,
    device: String,
    events: mpsc::Sender<PadEvent>,
    stop: Arc<AtomicBool>,
) -> MikoResult<watch::Receiver<Controller>> {
    let (reply, result) = sync_mpsc::channel();
    service()?
        .send(Request::Open {
            id,
            device,
            events,
            stop,
            reply,
        })
        .map_err(|_| MikoError::unexpected("gamepad service stopped"))?;
    result
        .recv_timeout(REPLY_TIMEOUT)
        .map_err(|_| MikoError::unexpected("opening gamepad timed out"))?
}
pub fn vibrate(id: usize, source: String, active: bool) -> MikoResult<()> {
    service()?
        .send(Request::Vibrate { id, source, active })
        .map_err(|_| MikoError::unexpected("gamepad service stopped"))
}
#[cfg(test)]
mod tests {
    use super::*;

    fn controller(id: usize) -> Controller {
        Controller {
            id,
            name: "Pad".into(),
            vendor: None,
            product: None,
            bound_device: None,
            power: Power::Unknown,
            controls: Vec::new(),
        }
    }

    #[tokio::test]
    async fn bindings_are_exclusive_and_overflow_closes_only_its_stream() {
        let mut bindings = HashMap::new();
        let (a, mut a_rx) = mpsc::channel(1);
        let (b, mut b_rx) = mpsc::channel(1);
        let stop = || Arc::new(AtomicBool::new(false));
        bind(&mut bindings, controller(1), "A".into(), a, stop()).unwrap();
        bind(&mut bindings, controller(2), "B".into(), b, stop()).unwrap();
        let (duplicate, _rx) = mpsc::channel(1);
        assert_eq!(
            bind(&mut bindings, controller(1), "C".into(), duplicate, stop())
                .unwrap_err()
                .code,
            miko_core::ErrorCode::Busy
        );
        deliver(
            &mut bindings,
            1,
            vec![PadEvent::Button(Button::South, true)],
            false,
        );
        deliver(
            &mut bindings,
            1,
            vec![PadEvent::Button(Button::South, false)],
            false,
        );
        assert!(!bindings.contains_key(&1));
        assert!(bindings.contains_key(&2));
        assert_eq!(
            a_rx.recv().await,
            Some(PadEvent::Button(Button::South, true))
        );
        assert_eq!(a_rx.recv().await, None);
        deliver(&mut bindings, 2, vec![PadEvent::Disconnected], true);
        assert_eq!(b_rx.recv().await, Some(PadEvent::Disconnected));
        assert_eq!(b_rx.recv().await, None);
    }

    #[tokio::test]
    async fn two_bound_controllers_route_to_their_own_devices() {
        let mut bindings = HashMap::new();
        let (a, mut a_rx) = mpsc::channel(4);
        let (b, mut b_rx) = mpsc::channel(4);
        let first_stop = Arc::new(AtomicBool::new(false));
        let second_stop = Arc::new(AtomicBool::new(false));
        bind(&mut bindings, controller(1), "A".into(), a, first_stop).unwrap();
        bind(&mut bindings, controller(2), "B".into(), b, second_stop).unwrap();
        let first = PadEvent::Button(Button::South, true);
        let second = PadEvent::Axis(Axis::RightX, 0.5);
        deliver(&mut bindings, 1, vec![first], false);
        deliver(&mut bindings, 2, vec![second], false);
        assert_eq!(a_rx.recv().await, Some(first));
        assert_eq!(b_rx.recv().await, Some(second));
        assert!(a_rx.try_recv().is_err());
        assert!(b_rx.try_recv().is_err());
        deliver(&mut bindings, 1, vec![PadEvent::Disconnected], true);
        assert_eq!(a_rx.recv().await, Some(PadEvent::Disconnected));
        deliver(
            &mut bindings,
            2,
            vec![PadEvent::Button(Button::East, true)],
            false,
        );
        assert_eq!(
            b_rx.recv().await,
            Some(PadEvent::Button(Button::East, true))
        );
    }
}

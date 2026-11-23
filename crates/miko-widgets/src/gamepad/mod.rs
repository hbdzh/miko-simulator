//! Game controller passthrough (#23): a controller on this machine drives
//! the device. `input.gamepad.connect` is a job that reads the controller
//! ([`host`]) and replays it through a virtual USB controller in the guest
//! ([`hid`]), made by AOSP's `hid` command over `adb shell`. No root: the
//! `shell` user may open `/dev/uhid`. `input.gamepad.disconnect` ends it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use miko_core::cancel::Cancel;
use miko_core::provider::{CallCtx, CommandSpec, DeviceParams, NoParams, parse_params};
use miko_core::{MikoError, MikoResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::device::target;
use crate::running::Running;

pub mod calibration;
pub mod hid;
pub mod host;
pub mod uinput;

use hid::{Pad, PadEvent};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectParams {
    /// Device name or id.
    pub device: String,
    /// Which controller: an `id` from `input.gamepad.list`. Default: the
    /// first one connected.
    #[serde(default)]
    pub controller: Option<usize>,
    /// Optional analog calibration; omitted settings preserve the raw input.
    #[serde(default)]
    pub calibration: calibration::Calibration,
    /// Optional client-generated capture session id.
    #[serde(default)]
    pub session: Option<uuid::Uuid>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DisconnectParams {
    /// Device name or id.
    device: String,
    /// Stop only this capture session; omit to stop the current binding.
    #[serde(default)]
    session: Option<uuid::Uuid>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FeedbackParams {
    device: String,
    session: uuid::Uuid,
    source: uuid::Uuid,
    vibrating: bool,
}

/// Stop flags of the controllers being forwarded, by device.
pub type Pads = Running;

/// How often the forwarding loop checks for a stop or a cancel.
const TICK: Duration = Duration::from_millis(100);
/// Android must expose the uniquely named input device within this period.
const REGISTER_TIMEOUT: Duration = Duration::from_secs(10);
const REGISTER_POLL: Duration = Duration::from_millis(200);
const READY_MESSAGE: &str = "miko.gamepad.ready";
const DISCONNECTED_MESSAGE: &str = "miko.gamepad.disconnected";
const RECONNECTING_MESSAGE: &str = "miko.gamepad.reconnecting";
const RECONNECT_POLL: Duration = Duration::from_secs(1);
const WRITE_TIMEOUT: Duration = Duration::from_secs(1);

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new::<NoParams>(
            "input.gamepad.list",
            "List the game controllers connected to this computer",
        ),
        CommandSpec::new::<ConnectParams>(
            "input.gamepad.connect",
            "Forward a game controller on this computer to the device until input.gamepad.disconnect",
        )
        .long_running(),
        CommandSpec::new::<ConnectParams>(
            "input.gamepad.capture",
            "Capture controller events for touch mapping until input.gamepad.disconnect",
        )
        .long_running(),
        CommandSpec::new::<DisconnectParams>(
            "input.gamepad.disconnect",
            "Stop forwarding the game controller to the device",
        ),
        CommandSpec::new::<DeviceParams>(
            "input.gamepad.get",
            "Whether a game controller is being forwarded to the device",
        ),
        CommandSpec::new::<FeedbackParams>(
            "input.gamepad.feedback",
            "Relay vibration from a synchronized device to the bound controller",
        ),
    ]
}

fn capture_event(event: PadEvent) -> Option<Value> {
    use hid::{Axis, Button};
    let name = match event {
        PadEvent::Button(button, down) => {
            let name = match button {
                Button::South => "South",
                Button::East => "East",
                Button::West => "West",
                Button::North => "North",
                Button::LeftBumper => "LeftBumper",
                Button::RightBumper => "RightBumper",
                Button::Back => "Back",
                Button::Start => "Start",
                Button::Guide => "Guide",
                Button::LeftStick => "LeftStick",
                Button::RightStick => "RightStick",
                Button::Up => "Up",
                Button::Down => "Down",
                Button::Left => "Left",
                Button::Right => "Right",
            };
            return Some(json!({"kind":"button", "control":name, "down":down}));
        }
        PadEvent::Axis(axis, _) => match axis {
            Axis::LeftX => "LeftX",
            Axis::LeftY => "LeftY",
            Axis::RightX => "RightX",
            Axis::RightY => "RightY",
            Axis::LeftTrigger => "LeftTrigger",
            Axis::RightTrigger => "RightTrigger",
        },
        PadEvent::Disconnected | PadEvent::DisconnectedAmbiguous => return None,
    };
    if let PadEvent::Axis(_, value) = event {
        return Some(json!({"kind":"axis", "control":name, "value":value}));
    }
    None
}

/// Sends every input transition with a sequence number; consumers must release
/// all active touches if they observe a gap or lose the subscription.
async fn capture_events(
    ctx: &CallCtx,
    device: uuid::Uuid,
    session: uuid::Uuid,
    rx: &mut mpsc::Receiver<PadEvent>,
    stop: &AtomicBool,
    calibration: calibration::Calibration,
    sequence: &mut u64,
) -> MikoResult<Ended> {
    let mut health = tokio::time::interval(Duration::from_millis(250));
    let result = loop {
        if let Err(error) = ctx.cancel.check() {
            break Err(error);
        }
        if stop.load(Ordering::Relaxed) {
            break Ok(Ended::Disconnected);
        }
        tokio::select! {
            _ = health.tick() => {
                if ctx.connection.is_some_and(|connection| !ctx.daemon.connection_alive(connection)) { break Ok(Ended::Disconnected); }
                if ctx.daemon.connect_info(device).await.is_err() { break Ok(Ended::DeviceStopped); }
                *sequence += 1;
                ctx.daemon.emit("input.gamepad.event", json!({"device":device, "session":session, "sequence":*sequence, "event":{"kind":"heartbeat"}}));
            }
            event = tokio::time::timeout(TICK, rx.recv()) => match event {
                Err(_) => {},
                Ok(None) => break Ok(Ended::InputFailed),
                Ok(Some(PadEvent::Disconnected)) => break Ok(Ended::Unplugged),
                Ok(Some(PadEvent::DisconnectedAmbiguous)) => break Ok(Ended::IdentityAmbiguous),
                Ok(Some(event)) => if let Some(payload) = capture_event(calibration.apply(event)) {
                    *sequence += 1;
                    ctx.daemon.emit("input.gamepad.event", json!({"device":device, "session":session, "sequence":*sequence, "event":payload}));
                },
            }
        }
    };
    let reason = result.as_ref().map_or("cancelled", |ended| ended.as_str());
    *sequence += 1;
    ctx.daemon.emit("input.gamepad.event", json!({"device":device, "session":session, "sequence":*sequence, "event":{"kind":"ended", "reason":reason}}));
    result
}

async fn capture(ctx: &CallCtx, pads: &Pads, p: ConnectParams) -> MikoResult<Value> {
    let calibration = p.calibration.validate()?;
    let target = target(ctx, &p.device).await?;
    let id = target.device.id;
    let reg = pads.start_replacing_recovery(id, || {
        "a game controller is already bound to this device".into()
    })?;
    let session = p.session.unwrap_or_else(uuid::Uuid::new_v4);
    pads.set_capture(id, session);
    let mut wanted = p.controller;
    let mut sequence = 0;
    let (controller, ended) = loop {
        let (tx, mut rx) = mpsc::channel(256);
        let reader_stop = Arc::new(AtomicBool::new(false));
        let reader = StopReader(reader_stop.clone());
        let name = target.device.name.clone();
        let controller =
            tokio::task::spawn_blocking(move || host::open(wanted, name, tx, reader_stop))
                .await
                .map_err(|error| {
                    MikoError::unexpected(format!("opening game controller: {error}"))
                })??;
        ctx.cancel.check()?;
        if rx.is_closed() || !pads.mark_ready(id) {
            return Err(MikoError::not_found(
                "game controller disconnected before capture",
            ));
        }
        ctx.report(None, READY_MESSAGE);
        let result = capture_events(
            ctx,
            id,
            session,
            &mut rx,
            &reg.flag,
            calibration,
            &mut sequence,
        )
        .await;
        pads.clear_ready(id);
        drop(reader);
        let controller = controller.borrow().clone();
        let ended = result?;
        if ended != Ended::Unplugged || reg.flag.load(Ordering::Relaxed) {
            break (controller, ended);
        }
        pads.mark_recovering(id);
        ctx.report(None, DISCONNECTED_MESSAGE);
        let next = loop {
            if reg.flag.load(Ordering::Relaxed)
                || ctx.cancel.is_cancelled()
                || ctx
                    .connection
                    .is_some_and(|connection| !ctx.daemon.connection_alive(connection))
            {
                break None;
            }
            if !matches!(ctx.daemon.connect_info(id).await, Ok(info) if info == target.info) {
                break None;
            }
            let all = tokio::task::spawn_blocking(host::list)
                .await
                .map_err(|error| {
                    MikoError::unexpected(format!("listing controllers: {error}"))
                })??;
            match match_reconnect(&controller, &all) {
                ReconnectMatch::Available(id) => break Some(id),
                ReconnectMatch::Ambiguous | ReconnectMatch::Occupied => break None,
                ReconnectMatch::Wait => tokio::time::sleep(RECONNECT_POLL).await,
            }
        };
        let Some(next) = next else {
            break (controller, ended);
        };
        if !pads.begin_reconnect(&reg) {
            break (controller, ended);
        }
        let current = crate::device::target(ctx, &id.to_string()).await?;
        if current.info != target.info || current.device.runtime != target.device.runtime {
            break (controller, ended);
        }
        wanted = Some(next);
        ctx.report(None, RECONNECTING_MESSAGE);
    };
    Ok(json!({"controller": controller, "session": session, "ended": ended.as_str()}))
}

/// Why forwarding stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// `input.gamepad.disconnect`.
    Disconnected,
    /// The controller went away.
    Unplugged,
    /// Another identical controller prevents a safe automatic match.
    IdentityAmbiguous,
    /// The host input stream ended without a physical disconnect event.
    InputFailed,
    /// The guest's `hid` command stopped taking reports (the device stopped).
    DeviceStopped,
}

impl Ended {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Unplugged => "unplugged",
            Self::IdentityAmbiguous => "identity_ambiguous",
            Self::InputFailed => "input_failed",
            Self::DeviceStopped => "device_stopped",
        }
    }
}

async fn write_line<W: AsyncWrite + Unpin>(w: &mut W, v: &Value) -> std::io::Result<()> {
    let mut line = v.to_string().into_bytes();
    line.push(b'\n');
    w.write_all(&line).await?;
    w.flush().await
}

async fn write_bounded<W: AsyncWrite + Unpin>(w: &mut W, v: &Value) -> std::io::Result<()> {
    tokio::time::timeout(WRITE_TIMEOUT, write_line(w, v))
        .await
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "gamepad input write timed out",
            )
        })?
}

/// Replays `rx` into `hid` on `w`, one report per change, until a stop,
/// a cancel (an error), the controller going away or `w` closing. Returns
/// why it ended and how many reports it sent.
pub async fn forward<W: AsyncWrite + Unpin>(
    rx: &mut mpsc::Receiver<PadEvent>,
    w: &mut W,
    stop: &AtomicBool,
    cancel: &Cancel,
) -> MikoResult<(Ended, u64)> {
    forward_calibrated(rx, w, stop, cancel, calibration::Calibration::default()).await
}

async fn forward_calibrated<W: AsyncWrite + Unpin>(
    rx: &mut mpsc::Receiver<PadEvent>,
    w: &mut W,
    stop: &AtomicBool,
    cancel: &Cancel,
    calibration: calibration::Calibration,
) -> MikoResult<(Ended, u64)> {
    forward_synced(rx, w, stop, cancel, calibration, None).await
}

async fn forward_synced<W: AsyncWrite + Unpin>(
    rx: &mut mpsc::Receiver<PadEvent>,
    w: &mut W,
    stop: &AtomicBool,
    cancel: &Cancel,
    calibration: calibration::Calibration,
    sync: Option<(&CallCtx, uuid::Uuid, uuid::Uuid)>,
) -> MikoResult<(Ended, u64)> {
    let guest = sync.map(|(ctx, device, session)| ForwardGuest {
        ctx,
        device,
        session,
        target: None,
        uinput: false,
    });
    forward_with_format(rx, w, stop, cancel, calibration, guest).await
}

struct ForwardGuest<'a> {
    ctx: &'a CallCtx,
    device: uuid::Uuid,
    session: uuid::Uuid,
    target: Option<(&'a miko_core::ConnectInfo, &'a mut tokio::process::Child)>,
    uinput: bool,
}

async fn target_gone(ctx: &CallCtx, original: &miko_core::ConnectInfo) -> bool {
    match ctx.daemon.connect_info(original.device).await {
        Ok(info) => {
            info.grpc_port != original.grpc_port
                || info.grpc_token != original.grpc_token
                || info.adb_serial != original.adb_serial
        }
        Err(error) if error.code == miko_core::ErrorCode::NotFound => {
            // The daemon temporarily removes the instance while stopping it.
            match ctx.daemon.device(&original.device.to_string()).await {
                Ok(device) => matches!(
                    device.status,
                    miko_core::DeviceStatus::Stopped
                        | miko_core::DeviceStatus::Crashed { .. }
                        | miko_core::DeviceStatus::Hibernated
                ),
                Err(error) => error.code == miko_core::ErrorCode::NotFound,
            }
        }
        Err(_) => false,
    }
}

async fn forward_with_format<W: AsyncWrite + Unpin>(
    rx: &mut mpsc::Receiver<PadEvent>,
    w: &mut W,
    stop: &AtomicBool,
    cancel: &Cancel,
    calibration: calibration::Calibration,
    mut guest: Option<ForwardGuest<'_>>,
) -> MikoResult<(Ended, u64)> {
    let uinput_mode = guest.as_ref().is_some_and(|guest| guest.uinput);
    let mut pad = Pad::default();
    let mut sent = 0;
    let mut sequence = 0u64;
    let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
    let result = loop {
        if let Err(error) = cancel.check() {
            break Err(error);
        }
        if stop.load(Ordering::Relaxed) {
            break Ok((Ended::Disconnected, sent));
        }
        let ev = tokio::select! {
            _ = heartbeat.tick(), if guest.is_some() => {
                if let Some(ForwardGuest { ctx, device, session, target, .. }) = guest.as_mut() {
                    if let Some((info, child)) = target
                        && (!matches!(child.try_wait(), Ok(None)) || target_gone(ctx, info).await)
                    {
                        break Ok((Ended::DeviceStopped, sent));
                    }
                    sequence += 1;
                    ctx.daemon.emit("input.gamepad.native", json!({"device":device,"session":session,"sequence":sequence,"report":pad.report()}));
                }
                continue;
            }
            event = tokio::time::timeout(TICK, rx.recv()) => match event {
            Err(_) => continue,
            Ok(None) => break Ok((Ended::InputFailed, sent)),
            Ok(Some(PadEvent::Disconnected)) => break Ok((Ended::Unplugged, sent)),
            Ok(Some(PadEvent::DisconnectedAmbiguous)) => {
                break Ok((Ended::IdentityAmbiguous, sent));
            }
            Ok(Some(ev)) => ev,
            }
        };
        if pad.apply(calibration.apply(ev)) {
            let report = if uinput_mode {
                uinput::report(&pad.report())
            } else {
                hid::report_line(&pad)
            };
            if write_bounded(w, &report).await.is_err() {
                break Ok((Ended::DeviceStopped, sent));
            }
            sent += 1;
            if let Some(ForwardGuest {
                ctx,
                device,
                session,
                ..
            }) = guest.as_ref()
            {
                sequence += 1;
                ctx.daemon.emit("input.gamepad.native", json!({"device":device,"session":session,"sequence":sequence,"report":pad.report()}));
            }
        }
    };
    if let Some(ForwardGuest {
        ctx,
        device,
        session,
        ..
    }) = guest
    {
        sequence += 1;
        ctx.daemon.emit(
            "input.gamepad.native",
            json!({"device":device,"session":session,"sequence":sequence,"ended":true}),
        );
    }
    // Release held controls before closing the virtual device. A failed write
    // is still followed by device removal and its independent confirmation.
    let neutral = if uinput_mode {
        uinput::report(&Pad::default().report())
    } else {
        hid::report_line(&Pad::default())
    };
    let _ = tokio::time::timeout(TICK, write_line(w, &neutral)).await;
    result
}

/// Sets the reader thread's stop flag however the job ends.
struct StopReader(Arc<AtomicBool>);

impl Drop for StopReader {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

fn before_registration(mut error: MikoError) -> MikoError {
    let data = error.data.get_or_insert_with(|| json!({}));
    if let Some(data) = data.as_object_mut() {
        data.insert("input_released".into(), Value::Bool(true));
    }
    error
}

async fn finish_forward(
    mut child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    result: MikoResult<(Ended, u64)>,
    guest: Option<(&miko_adb::Adb, &str)>,
) -> MikoResult<(Ended, u64)> {
    // EOF ends `hid`, and the guest removes the controller.
    drop(stdin);
    match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
        Ok(Ok(_)) => {}
        _ => {
            return Err(MikoError::unexpected(
                "virtual controller removal was not confirmed",
            ));
        }
    }
    if let Some((adb, name)) = guest {
        confirm_removal(adb, name, Duration::from_secs(5)).await?;
    }
    result.map_err(|mut error| {
        let data = error.data.get_or_insert_with(|| json!({}));
        if let Some(data) = data.as_object_mut() {
            data.insert("input_released".into(), Value::Bool(true));
        }
        error
    })
}

pub async fn confirm_removal(adb: &miko_adb::Adb, name: &str, timeout: Duration) -> MikoResult<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(MikoError::unexpected(
                "virtual controller removal was not confirmed by Android",
            ));
        }
        match tokio::time::timeout(
            remaining.min(Duration::from_secs(1)),
            adb.shell("dumpsys input"),
        )
        .await
        {
            Ok(Ok(dump)) if !dump.trim().is_empty() && !input_device_present(&dump, name) => {
                return Ok(());
            }
            Ok(Err(error)) => {
                return Err(MikoError::unexpected(format!(
                    "checking virtual controller removal: {error}"
                )));
            }
            _ => {}
        }
        tokio::time::sleep(
            TICK.min(deadline.saturating_duration_since(tokio::time::Instant::now())),
        )
        .await;
    }
}

fn input_device_present(dump: &str, name: &str) -> bool {
    dump.lines().any(|line| {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Name:") {
            return value.trim().trim_matches('"') == name;
        }
        let numbered = line.strip_prefix("Device ").unwrap_or(line);
        numbered.split_once(':').is_some_and(|(id, value)| {
            !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) && value.trim() == name
        })
    })
}

/// Exposes the readiness parser to the isolated emulator acceptance test.
#[cfg(feature = "emulator")]
#[doc(hidden)]
pub fn android_input_device_present(dump: &str, name: &str) -> bool {
    input_device_present(dump, name)
}

#[cfg(feature = "emulator")]
#[doc(hidden)]
pub async fn android_confirm_gamepad_removal(adb: &miko_adb::Adb, name: &str) -> MikoResult<()> {
    confirm_removal(adb, name, Duration::from_secs(5)).await
}

pub async fn wait_for_registration(
    adb: &miko_adb::Adb,
    child: &mut tokio::process::Child,
    name: &str,
    stop: &AtomicBool,
    cancel: &Cancel,
) -> MikoResult<()> {
    let deadline = tokio::time::Instant::now() + REGISTER_TIMEOUT;
    loop {
        cancel.check()?;
        if stop.load(Ordering::Relaxed) {
            return Err(miko_core::cancel::cancelled());
        }
        if child
            .try_wait()
            .map_err(|e| MikoError::unexpected(format!("checking virtual controller: {e}")))?
            .is_some()
        {
            return Err(MikoError::sdk(
                "virtual controller exited before Android registered it",
            ));
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(MikoError::sdk(
                "Android did not register the virtual controller in time",
            ));
        }
        let check = tokio::time::timeout(
            remaining.min(Duration::from_secs(2)),
            adb.shell("dumpsys input"),
        )
        .await;
        match check {
            Ok(Ok(dump)) if input_device_present(&dump, name) => return Ok(()),
            Ok(Err(error)) => return Err(error),
            _ => {}
        }
        tokio::time::sleep(
            REGISTER_POLL.min(deadline.saturating_duration_since(tokio::time::Instant::now())),
        )
        .await;
    }
}

async fn forward_once(
    ctx: &CallCtx,
    pads: &Pads,
    t: &crate::device::Target,
    wanted: Option<usize>,
    stop: &AtomicBool,
    calibration: calibration::Calibration,
) -> MikoResult<(host::Controller, Ended, u64)> {
    if stop.load(Ordering::Relaxed) {
        return Err(before_registration(miko_core::cancel::cancelled()));
    }
    let name = t.device.name.clone();
    let device_id = t.device.id;
    let adb = t.adb();
    let (uinput_status, _) = adb
        .shell_status("command -v uinput >/dev/null && test -w /dev/uinput")
        .await
        .map_err(before_registration)?;
    let uinput_mode = uinput_status == 0;
    if !uinput_mode {
        let (status, _) = adb
            .shell_status("command -v hid")
            .await
            .map_err(before_registration)?;
        if status != 0 {
            return Err(MikoError::usage(format!(
                "{name} has no `uinput` or `hid` command"
            )));
        }
    }

    let (tx, mut rx) = mpsc::channel(256);
    let reader_stop = Arc::new(AtomicBool::new(false));
    let _reader = StopReader(reader_stop.clone());
    let bound_device = name.clone();
    let controller =
        tokio::task::spawn_blocking(move || host::open(wanted, bound_device, tx, reader_stop))
            .await
            .map_err(|e| {
                before_registration(MikoError::unexpected(format!(
                    "opening the controller: {e}"
                )))
            })?
            .map_err(before_registration)?;

    ctx.cancel.check().map_err(before_registration)?;

    let mut child = if uinput_mode {
        adb.spawn_with_stdio(&["shell", "uinput", "-"])
    } else {
        adb.spawn_with_stdin(&["shell", "hid", "-"])
    }
    .map_err(before_registration)?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| MikoError::unexpected("adb shell hid has no stdin"))?;
    // Keep the unique part short and intact even if Android truncates names.
    let virtual_name = format!("miko-{}", uuid::Uuid::new_v4().simple());
    let registration = async {
        let register = if uinput_mode {
            uinput::register(&virtual_name)
        } else {
            hid::register_line(&virtual_name)
        };
        write_bounded(&mut stdin, &register)
            .await
            .map_err(|e| MikoError::sdk(format!("registering virtual controller: {e}")))?;
        wait_for_registration(&adb, &mut child, &virtual_name, stop, &ctx.cancel).await?;
        let neutral = if uinput_mode {
            uinput::report(&Pad::default().report())
        } else {
            hid::report_line(&Pad::default())
        };
        write_bounded(&mut stdin, &neutral)
            .await
            .map_err(|e| MikoError::sdk(format!("initializing virtual controller input: {e}")))?;
        if child
            .try_wait()
            .map_err(|e| MikoError::unexpected(format!("checking virtual controller input: {e}")))?
            .is_some()
        {
            return Err(MikoError::sdk(
                "virtual controller exited while initializing input",
            ));
        }
        ctx.cancel.check()?;
        if rx.is_closed() {
            return Err(MikoError::not_found(
                "physical game controller disconnected during registration",
            ));
        }
        if !pads.mark_ready(device_id) {
            return Err(miko_core::cancel::cancelled());
        }
        Ok(())
    }
    .await;
    if let Err(error) = registration {
        finish_forward(child, stdin, Err(error), Some((&adb, &virtual_name))).await?;
        return Err(MikoError::unexpected(
            "registration failed without an error",
        ));
    }
    ctx.report(None, READY_MESSAGE);
    let session = uuid::Uuid::new_v4();
    let controller_id = controller.borrow().id;
    pads.set_native_controller(device_id, session, controller_id);
    let feedback = child.stdout.take().map(|mut stdout| {
        let daemon = ctx.daemon.clone();
        let controller = controller_id;
        tokio::spawn(async move {
            let mut decoder = uinput::FeedbackDecoder::default();
            let mut bytes = [0u8; 4096];
            loop {
                match stdout.read(&mut bytes).await {
                    Ok(0) | Err(_) => break,
                    Ok(count) => match decoder.push(&bytes[..count]) {
                        Ok(events) => for vibrating in events {
                            if let Err(error) = host::vibrate(controller, "leader".into(), vibrating) {
                                tracing::warn!(%error, "gamepad vibration failed");
                            }
                            daemon.emit("input.gamepad.feedback", json!({"device":device_id,"controller":controller,"session":session,"vibrating":vibrating}));
                        },
                        Err(_) => break,
                    }
                }
            }
            let _ = host::vibrate(controller, "leader".into(), false);
            daemon.emit("input.gamepad.feedback", json!({"device":device_id,"controller":controller,"session":session,"vibrating":false}));
        })
    });
    let result = forward_with_format(
        &mut rx,
        &mut stdin,
        stop,
        &ctx.cancel,
        calibration,
        Some(ForwardGuest {
            ctx,
            device: device_id,
            session,
            target: Some((&t.info, &mut child)),
            uinput: uinput_mode,
        }),
    )
    .await;
    pads.clear_ready(device_id);
    let (result, guest) = if target_gone(ctx, &t.info).await {
        (
            Ok((
                Ended::DeviceStopped,
                result.map_or(0, |(_, reports)| reports),
            )),
            None,
        )
    } else {
        (result, Some((&adb, virtual_name.as_str())))
    };
    let (ended, reports) = finish_forward(child, stdin, result, guest).await?;
    if let Some(task) = feedback {
        let _ = tokio::time::timeout(Duration::from_secs(1), task).await;
    }
    Ok((controller.borrow().clone(), ended, reports))
}

#[derive(Debug, PartialEq, Eq)]
enum ReconnectMatch {
    Wait,
    Ambiguous,
    Occupied,
    Available(usize),
}

fn match_reconnect(original: &host::Controller, all: &[host::Controller]) -> ReconnectMatch {
    let (Some(vendor), Some(product)) = (original.vendor, original.product) else {
        return ReconnectMatch::Ambiguous;
    };
    let mut matches = all.iter().filter(|c| {
        c.name == original.name
            && c.vendor.is_none_or(|id| id == vendor)
            && c.product.is_none_or(|id| id == product)
    });
    let Some(found) = matches.next() else {
        return ReconnectMatch::Wait;
    };
    if matches.next().is_some() {
        return ReconnectMatch::Ambiguous;
    }
    if found.vendor.is_none() || found.product.is_none() {
        return ReconnectMatch::Wait;
    }
    if found.bound_device.is_some() {
        ReconnectMatch::Occupied
    } else {
        ReconnectMatch::Available(found.id)
    }
}

async fn connect(ctx: &CallCtx, pads: &Pads, p: ConnectParams) -> MikoResult<Value> {
    let calibration = p.calibration.validate().map_err(before_registration)?;
    let t = target(ctx, &p.device).await.map_err(before_registration)?;
    let name = t.device.name.clone();
    let reg = pads.start_replacing_recovery(t.device.id, || format!("a game controller is already forwarded to {name}; disconnect it first (input.gamepad.disconnect)"))
        .map_err(before_registration)?;
    let (mut controller, mut ended, mut reports) =
        forward_once(ctx, pads, &t, p.controller, &reg.flag, calibration).await?;
    while ended == Ended::Unplugged
        && !reg.flag.load(Ordering::Relaxed)
        && !ctx.cancel.is_cancelled()
    {
        let Some(lease) = ctx.input_lease.as_ref() else {
            break;
        };
        // The guest process has exited successfully, so no injected control remains.
        lease.suspend()?;
        pads.mark_recovering(t.device.id);
        ctx.report(None, DISCONNECTED_MESSAGE);
        let next = loop {
            if reg.flag.load(Ordering::Relaxed) {
                break None;
            }
            if ctx.cancel.is_cancelled() {
                return Err(before_registration(miko_core::cancel::cancelled()));
            }
            if !matches!(ctx.daemon.connect_info(t.device.id).await, Ok(info) if info == t.info) {
                break None;
            }
            let all = tokio::task::spawn_blocking(host::list)
                .await
                .map_err(|e| {
                    before_registration(MikoError::unexpected(format!("listing controllers: {e}")))
                })?
                .map_err(before_registration)?;
            match match_reconnect(&controller, &all) {
                ReconnectMatch::Available(id) => break Some(id),
                ReconnectMatch::Ambiguous | ReconnectMatch::Occupied => break None,
                ReconnectMatch::Wait => tokio::time::sleep(RECONNECT_POLL).await,
            }
        };
        let Some(id) = next else { break };
        if !pads.begin_reconnect(&reg) || lease.resume().is_err() {
            break;
        }
        ctx.report(None, RECONNECTING_MESSAGE);
        let next_target = target(ctx, &t.device.id.to_string())
            .await
            .map_err(before_registration)?;
        if next_target.info != t.info || next_target.device.runtime != t.device.runtime {
            break;
        }
        let (next_controller, next_ended, count) =
            forward_once(ctx, pads, &next_target, Some(id), &reg.flag, calibration).await?;
        controller = next_controller;
        ended = next_ended;
        reports += count;
    }
    Ok(json!({ "controller": controller, "reports": reports, "ended": ended.as_str() }))
}

pub async fn call(ctx: &CallCtx, pads: &Pads, method: &str, params: Value) -> MikoResult<Value> {
    match method {
        "input.gamepad.list" => {
            let _: NoParams = parse_params(params)?;
            let all = tokio::task::spawn_blocking(host::list)
                .await
                .map_err(|e| MikoError::unexpected(format!("listing controllers: {e}")))??;
            Ok(json!({ "controllers": all }))
        }
        "input.gamepad.connect" => connect(ctx, pads, parse_params(params)?).await,
        "input.gamepad.capture" => capture(ctx, pads, parse_params(params)?).await,
        "input.gamepad.feedback" => {
            let p: FeedbackParams = parse_params(params)?;
            let device = ctx.daemon.device(&p.device).await?;
            if let Some(controller) = pads.native_controller(device.id, p.session) {
                host::vibrate(controller, p.source.to_string(), p.vibrating)?;
            }
            Ok(json!({}))
        }
        "input.gamepad.disconnect" => {
            let p: DisconnectParams = parse_params(params)?;
            let d = ctx.daemon.device(&p.device).await?;
            let stopped = p.session.map_or_else(
                || pads.stop(d.id),
                |session| pads.stop_session(d.id, session),
            );
            Ok(json!({ "disconnected": stopped }))
        }
        "input.gamepad.get" => {
            let p: DeviceParams = parse_params(params)?;
            let d = ctx.daemon.device(&p.device).await?;
            let (connected, recovering, capture, session) = pads.status(d.id);
            if capture {
                Ok(
                    json!({ "connected": connected, "recovering": recovering, "mode": "touch", "session": session }),
                )
            } else if recovering {
                Ok(json!({ "connected": false, "recovering": true }))
            } else {
                Ok(json!({ "connected": connected }))
            }
        }
        _ => Err(MikoError::usage(format!("unknown method {method}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::hid::{Axis, Button};
    use super::*;

    struct CaptureContext {
        paths: miko_core::MikoPaths,
        events: std::sync::Mutex<Vec<Value>>,
        connected: AtomicBool,
    }

    #[async_trait::async_trait]
    impl miko_core::provider::DaemonContext for CaptureContext {
        fn paths(&self) -> &miko_core::MikoPaths {
            &self.paths
        }
        fn connection_alive(&self, _: u64) -> bool {
            self.connected.load(Ordering::Relaxed)
        }
        fn adb_path(&self) -> MikoResult<std::path::PathBuf> {
            unimplemented!()
        }
        async fn device(&self, _: &str) -> MikoResult<miko_core::Device> {
            unimplemented!()
        }
        async fn devices(&self) -> Vec<miko_core::Device> {
            unimplemented!()
        }
        async fn connect_info(&self, device: uuid::Uuid) -> MikoResult<miko_core::ConnectInfo> {
            Ok(miko_core::ConnectInfo {
                device,
                grpc_endpoint: String::new(),
                grpc_port: 0,
                grpc_token: String::new(),
                adb_serial: String::new(),
                adb_path: Default::default(),
                console_port: 0,
            })
        }
        async fn update_device(
            &self,
            _: uuid::Uuid,
            _: miko_core::provider::DeviceUpdate,
        ) -> MikoResult<miko_core::Device> {
            unimplemented!()
        }
        async fn start_device(
            &self,
            _: uuid::Uuid,
            _: bool,
            _: &Cancel,
        ) -> MikoResult<miko_core::Device> {
            unimplemented!()
        }
        async fn stop_device(&self, _: uuid::Uuid) -> MikoResult<miko_core::Device> {
            unimplemented!()
        }
        async fn shut_down_device(&self, _: uuid::Uuid) -> MikoResult<miko_core::Device> {
            unimplemented!()
        }
        fn emit(&self, topic: &str, data: Value) {
            assert!(matches!(
                topic,
                "input.gamepad.event" | "input.gamepad.native"
            ));
            self.events.lock().unwrap().push(data);
        }
    }

    #[tokio::test]
    async fn native_sync_emits_ordered_states_and_ended() {
        let context = Arc::new(CaptureContext {
            paths: miko_core::MikoPaths::under(std::env::temp_dir()),
            events: std::sync::Mutex::new(Vec::new()),
            connected: AtomicBool::new(true),
        });
        let ctx = CallCtx::new(context.clone());
        let (tx, mut rx) = mpsc::channel(4);
        tx.send(PadEvent::Button(Button::South, true))
            .await
            .unwrap();
        tx.send(PadEvent::Button(Button::South, false))
            .await
            .unwrap();
        tx.send(PadEvent::Disconnected).await.unwrap();
        let session = uuid::Uuid::new_v4();
        let device = uuid::Uuid::new_v4();
        let (ended, _) = forward_synced(
            &mut rx,
            &mut tokio::io::sink(),
            &AtomicBool::new(false),
            &Cancel::new(),
            calibration::Calibration::default(),
            Some((&ctx, device, session)),
        )
        .await
        .unwrap();
        assert_eq!(ended, Ended::Unplugged);
        let events = context.events.lock().unwrap();
        assert!(events.len() >= 3);
        for (index, event) in events.iter().enumerate() {
            assert_eq!(event["sequence"], (index + 1) as u64);
            assert_eq!(event["session"], session.to_string());
        }
        assert_eq!(events.last().unwrap()["ended"], true);
        assert!(events.iter().any(|event| event["report"][0] == 1));
        assert!(events.iter().any(|event| event["report"][0] == 0));
    }

    #[tokio::test]
    async fn capture_emits_ordered_events_and_a_terminal_release_signal() {
        let context = Arc::new(CaptureContext {
            paths: miko_core::MikoPaths::under(std::env::temp_dir()),
            events: std::sync::Mutex::new(Vec::new()),
            connected: AtomicBool::new(true),
        });
        let ctx = CallCtx::new(context.clone());
        let (tx, mut rx) = mpsc::channel(8);
        tx.send(PadEvent::Button(Button::South, true))
            .await
            .unwrap();
        tx.send(PadEvent::Button(Button::South, false))
            .await
            .unwrap();
        tx.send(PadEvent::Axis(Axis::LeftX, 0.5)).await.unwrap();
        tx.send(PadEvent::Disconnected).await.unwrap();
        let session = uuid::Uuid::new_v4();
        let device = uuid::Uuid::new_v4();
        let mut sequence = 0;
        assert_eq!(
            capture_events(
                &ctx,
                device,
                session,
                &mut rx,
                &AtomicBool::new(false),
                calibration::Calibration::default(),
                &mut sequence,
            )
            .await
            .unwrap(),
            Ended::Unplugged
        );
        {
            let events = context.events.lock().unwrap();
            for (index, event) in events.iter().enumerate() {
                assert_eq!(event["sequence"], (index + 1) as u64);
                assert_eq!(event["session"], session.to_string());
            }
            let values: Vec<_> = events
                .iter()
                .map(|event| &event["event"])
                .filter(|event| event["kind"] != "heartbeat")
                .collect();
            assert_eq!(values.len(), 4);
            assert_eq!(values[0]["down"], true);
            assert_eq!(values[1]["down"], false);
            assert_eq!(values[2]["value"], 0.5);
            assert_eq!(values[3]["kind"], "ended");
        }
        let (tx, mut rx) = mpsc::channel(2);
        tx.send(PadEvent::Button(Button::North, true))
            .await
            .unwrap();
        tx.send(PadEvent::Disconnected).await.unwrap();
        capture_events(
            &ctx,
            device,
            session,
            &mut rx,
            &AtomicBool::new(false),
            calibration::Calibration::default(),
            &mut sequence,
        )
        .await
        .unwrap();
        let events = context.events.lock().unwrap();
        for (index, event) in events.iter().enumerate() {
            assert_eq!(
                event["sequence"],
                (index + 1) as u64,
                "sequence remains continuous after reconnect"
            );
        }
    }

    #[tokio::test]
    async fn cancelled_capture_still_emits_terminal_signal() {
        let context = Arc::new(CaptureContext {
            paths: miko_core::MikoPaths::under(std::env::temp_dir()),
            events: std::sync::Mutex::new(Vec::new()),
            connected: AtomicBool::new(true),
        });
        let ctx = CallCtx::new(context.clone());
        ctx.cancel.cancel();
        let (_tx, mut rx) = mpsc::channel(1);
        let error = capture_events(
            &ctx,
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            &mut rx,
            &AtomicBool::new(false),
            calibration::Calibration::default(),
            &mut 0,
        )
        .await
        .unwrap_err();
        assert!(miko_core::cancel::is_cancelled(&error));
        let events = context.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["sequence"], 1);
        assert_eq!(events[0]["event"]["kind"], "ended");
        assert_eq!(events[0]["event"]["reason"], "cancelled");
    }

    #[tokio::test]
    async fn capture_ends_when_subscribing_client_disconnects() {
        let context = Arc::new(CaptureContext {
            paths: miko_core::MikoPaths::under(std::env::temp_dir()),
            events: std::sync::Mutex::new(Vec::new()),
            connected: AtomicBool::new(false),
        });
        let ctx = CallCtx::new(context.clone()).on_connection(7);
        let (_tx, mut rx) = mpsc::channel(1);
        let ended = capture_events(
            &ctx,
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            &mut rx,
            &AtomicBool::new(false),
            calibration::Calibration::default(),
            &mut 0,
        )
        .await
        .unwrap();
        assert_eq!(ended, Ended::Disconnected);
        assert_eq!(context.events.lock().unwrap()[0]["event"]["kind"], "ended");
    }

    fn controller(
        id: usize,
        name: &str,
        vendor: Option<u16>,
        product: Option<u16>,
        bound: Option<&str>,
    ) -> host::Controller {
        host::Controller {
            id,
            name: name.into(),
            vendor,
            product,
            bound_device: bound.map(str::to_owned),
            power: host::Power::Unknown,
            controls: Vec::new(),
        }
    }

    #[test]
    fn reconnect_requires_one_unoccupied_matching_model() {
        let original = controller(1, "Pad", Some(2), Some(3), None);
        assert_eq!(match_reconnect(&original, &[]), ReconnectMatch::Wait);
        assert_eq!(
            match_reconnect(&original, &[controller(4, "Pad", Some(2), Some(3), None)]),
            ReconnectMatch::Available(4)
        );
        assert_eq!(
            match_reconnect(
                &original,
                &[controller(4, "Pad", Some(2), Some(3), Some("other"))]
            ),
            ReconnectMatch::Occupied
        );
        assert_eq!(
            match_reconnect(
                &original,
                &[
                    controller(4, "Pad", Some(2), Some(3), None),
                    controller(5, "Pad", Some(2), Some(3), None)
                ]
            ),
            ReconnectMatch::Ambiguous
        );
        assert_eq!(
            match_reconnect(&controller(1, "Pad", None, Some(3), None), &[]),
            ReconnectMatch::Ambiguous
        );
        assert_eq!(
            match_reconnect(&original, &[controller(4, "Pad", Some(2), Some(4), None)]),
            ReconnectMatch::Wait
        );
    }

    #[test]
    fn capture_events_keep_button_transitions_and_analog_values() {
        assert_eq!(
            capture_event(PadEvent::Button(Button::South, true)),
            Some(json!({"kind":"button","control":"South","down":true}))
        );
        assert_eq!(
            capture_event(PadEvent::Button(Button::South, false)),
            Some(json!({"kind":"button","control":"South","down":false}))
        );
        assert_eq!(
            capture_event(PadEvent::Axis(Axis::LeftX, 0.5)),
            Some(json!({"kind":"axis","control":"LeftX","value":0.5}))
        );
        assert_eq!(capture_event(PadEvent::Disconnected), None);
    }

    #[test]
    fn registration_requires_the_exact_android_device_name() {
        let dump = "  Name: \"miko Pad abc\"\n  Name: \"miko Pad abc-other\"\n";
        assert!(input_device_present(dump, "miko Pad abc"));
        assert!(!input_device_present(dump, "miko Pad"));
        assert!(!input_device_present(
            "Recent: miko Pad abc",
            "miko Pad abc"
        ));
        assert!(input_device_present("    14: miko Pad abc", "miko Pad abc"));
        assert!(input_device_present(
            "  Device 14: miko Pad abc",
            "miko Pad abc"
        ));
        assert!(!input_device_present(
            "  Device 14: miko Pad abc-other",
            "miko Pad abc"
        ));
    }

    #[tokio::test]
    async fn registration_waits_for_android_and_respects_cancellation() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("adb");
        std::fs::write(&script, "#!/bin/sh\nprintf '  Name: \"miko-test\"\\n'\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let adb = miko_adb::Adb::new(script);
        let mut child = tokio::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        wait_for_registration(
            &adb,
            &mut child,
            "miko-test",
            &AtomicBool::new(false),
            &Cancel::new(),
        )
        .await
        .unwrap();
        let cancel = Cancel::new();
        cancel.cancel();
        let error = wait_for_registration(
            &adb,
            &mut child,
            "miko-test",
            &AtomicBool::new(false),
            &cancel,
        )
        .await
        .unwrap_err();
        assert!(miko_core::cancel::is_cancelled(&error));
    }

    #[tokio::test]
    async fn release_requires_android_to_stop_listing_the_device() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("adb");
        std::fs::write(&script, "#!/bin/sh\nprintf '  Device 7: miko-test\\n'\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let adb = miko_adb::Adb::new(&script);
        assert!(
            confirm_removal(&adb, "miko-test", Duration::from_millis(250))
                .await
                .is_err()
        );
        std::fs::write(&script, "#!/bin/sh\nprintf 'Input Reader State\\n'\n").unwrap();
        confirm_removal(&adb, "miko-test", Duration::from_secs(1))
            .await
            .unwrap();
    }

    fn lines(buf: &[u8]) -> Vec<Value> {
        String::from_utf8_lossy(buf)
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn forwards_changes_until_the_controller_goes_away() {
        let (tx, mut rx) = mpsc::channel(8);
        for ev in [
            PadEvent::Button(Button::South, true),
            PadEvent::Button(Button::South, true), // no change, no report
            PadEvent::Axis(Axis::LeftTrigger, 1.0),
            PadEvent::Disconnected,
        ] {
            tx.send(ev).await.unwrap();
        }
        let mut out = Vec::new();
        let (ended, sent) = forward(&mut rx, &mut out, &AtomicBool::new(false), &Cancel::new())
            .await
            .unwrap();
        assert_eq!((ended, sent), (Ended::Unplugged, 2));
        let l = lines(&out);
        assert_eq!(l.len(), 3);
        assert_eq!(l[0]["command"], "report");
        assert_eq!(l[1]["report"][0], 1, "A still held");
        assert_eq!(l[1]["report"][10], 255, "left trigger");
        assert_eq!(
            l[2],
            hid::report_line(&Pad::default()),
            "unplug releases all controls"
        );
    }

    #[tokio::test]
    async fn forward_applies_calibration_before_writing_reports() {
        let (tx, mut rx) = mpsc::channel(4);
        tx.send(PadEvent::Axis(Axis::LeftX, 0.1)).await.unwrap();
        tx.send(PadEvent::Axis(Axis::LeftX, 0.6)).await.unwrap();
        tx.send(PadEvent::Disconnected).await.unwrap();
        let mut tuning = calibration::Calibration::default();
        tuning.left.deadzone = 0.2;
        tuning.left.sensitivity = 2.0;
        tuning.left.invert_x = true;
        let mut out = Vec::new();
        let (ended, sent) = forward_calibrated(
            &mut rx,
            &mut out,
            &AtomicBool::new(false),
            &Cancel::new(),
            tuning.validate().unwrap(),
        )
        .await
        .unwrap();
        assert_eq!((ended, sent), (Ended::Unplugged, 1));
        let reports = lines(&out);
        assert_eq!(reports[0]["report"][2], 1);
        assert_eq!(reports[0]["report"][3], 128);
        assert_eq!(reports[1], hid::report_line(&Pad::default()));
    }

    #[tokio::test]
    async fn stops_on_disconnect_and_on_cancel() {
        let (_tx, mut rx) = mpsc::channel::<PadEvent>(1);
        let stop = AtomicBool::new(true);
        let (ended, sent) = forward(&mut rx, &mut Vec::new(), &stop, &Cancel::new())
            .await
            .unwrap();
        assert_eq!((ended, sent), (Ended::Disconnected, 0));

        let cancel = Cancel::new();
        cancel.cancel();
        let e = forward(&mut rx, &mut Vec::new(), &AtomicBool::new(false), &cancel)
            .await
            .unwrap_err();
        assert!(miko_core::cancel::is_cancelled(&e));
    }

    #[tokio::test]
    async fn stop_and_takeover_cancel_release_held_controls() {
        use tokio::io::{AsyncBufReadExt, BufReader};

        for takeover in [false, true] {
            let (tx, mut rx) = mpsc::channel(4);
            let (mut writer, reader) = tokio::io::duplex(1024);
            let stop = Arc::new(AtomicBool::new(false));
            let cancel = Cancel::new();
            let task_stop = stop.clone();
            let task_cancel = cancel.clone();
            let task = tokio::spawn(async move {
                forward(&mut rx, &mut writer, &task_stop, &task_cancel).await
            });
            tx.send(PadEvent::Button(Button::South, true))
                .await
                .unwrap();
            tx.send(PadEvent::Axis(Axis::LeftX, 1.0)).await.unwrap();
            let mut lines = BufReader::new(reader).lines();
            lines.next_line().await.unwrap().unwrap();
            lines.next_line().await.unwrap().unwrap();
            if takeover {
                cancel.cancel();
            } else {
                stop.store(true, Ordering::Relaxed);
            }
            let released: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(released, hid::report_line(&Pad::default()));
            let result = task.await.unwrap();
            if takeover {
                assert!(miko_core::cancel::is_cancelled(&result.unwrap_err()));
            } else {
                assert_eq!(result.unwrap().0, Ended::Disconnected);
            }
        }
    }

    #[tokio::test]
    async fn cancelled_forward_closes_input_before_confirming_release() {
        let mut child = tokio::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let error = finish_forward(child, stdin, Err(miko_core::cancel::cancelled()), None)
            .await
            .unwrap_err();
        assert!(miko_core::cancel::is_cancelled(&error));
        assert_eq!(error.data.unwrap()["input_released"], true);
    }

    #[tokio::test]
    async fn a_closed_guest_ends_the_job() {
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(PadEvent::Button(Button::Start, true))
            .await
            .unwrap();
        let (w, r) = tokio::io::duplex(64);
        drop(r);
        let mut w = w;
        let (ended, sent) = forward(&mut rx, &mut w, &AtomicBool::new(false), &Cancel::new())
            .await
            .unwrap();
        assert_eq!((ended, sent), (Ended::DeviceStopped, 0));
    }

    #[tokio::test]
    async fn a_stalled_guest_cannot_hold_the_forwarder_forever() {
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(PadEvent::Button(Button::South, true))
            .await
            .unwrap();
        let (mut writer, _unread) = tokio::io::duplex(1);
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            forward(
                &mut rx,
                &mut writer,
                &AtomicBool::new(false),
                &Cancel::new(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.0, Ended::DeviceStopped);
    }

    #[tokio::test]
    async fn a_failed_host_stream_or_ambiguous_disconnect_does_not_look_like_reconnectable_unplug()
    {
        let (tx, mut rx) = mpsc::channel(1);
        drop(tx);
        let (ended, _) = forward(
            &mut rx,
            &mut Vec::new(),
            &AtomicBool::new(false),
            &Cancel::new(),
        )
        .await
        .unwrap();
        assert_eq!(ended, Ended::InputFailed);
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(PadEvent::DisconnectedAmbiguous).await.unwrap();
        let (ended, _) = forward(
            &mut rx,
            &mut Vec::new(),
            &AtomicBool::new(false),
            &Cancel::new(),
        )
        .await
        .unwrap();
        assert_eq!(ended, Ended::IdentityAmbiguous);
    }
}

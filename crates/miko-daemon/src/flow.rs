//! Conditional workflow jobs exposed to all daemon clients.
use crate::flow_device::FlowDevice;
use async_trait::async_trait;
use miko_automation::{Flow, FlowAction, FlowHost, Locator, Point, run_flow};
use miko_core::{
    Cancel, ConnectInfo, ErrorCode, MikoError, MikoResult,
    provider::{CallCtx, CommandProvider, CommandSpec, parse_params},
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

#[cfg(all(test, feature = "emulator"))]
#[path = "flow_reconnect_emulator_test.rs"]
mod flow_reconnect_emulator_test;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RunParams {
    device: String,
    flow: Flow,
    #[serde(default)]
    start_paused: bool,
    #[serde(default)]
    screenshots: bool,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ControlParams {
    device: String,
    command: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StatusParams {
    device: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ValidateParams {
    flow: Flow,
}

#[derive(Default)]
struct SessionState {
    paused: bool,
    steps: u32,
    current: String,
}
struct Session {
    state: Mutex<SessionState>,
    cancel: Cancel,
}
#[derive(Default)]
pub struct FlowProvider {
    sessions: Mutex<HashMap<Uuid, Arc<Session>>>,
}

struct Host<'a> {
    device: FlowDevice,
    session: Arc<Session>,
    ctx: &'a CallCtx,
    screenshots: bool,
    dir: PathBuf,
    connection: ConnectInfo,
}

fn connection_lost(error: &MikoError) -> bool {
    if error
        .data
        .as_ref()
        .and_then(|d| d.get("flow_fatal"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return false;
    }
    if matches!(
        error
            .data
            .as_ref()
            .and_then(|d| d.get("grpc_code"))
            .and_then(Value::as_str),
        Some("Unavailable" | "DeadlineExceeded")
    ) {
        return true;
    }
    // Tonic can surface a broken HTTP/2 connection as Unknown/Internal with
    // the specific message "transport error", rather than Unavailable.
    if matches!(
        error
            .data
            .as_ref()
            .and_then(|d| d.get("grpc_code"))
            .and_then(Value::as_str),
        Some("Unknown" | "Internal")
    ) && error.message.ends_with(": transport error")
    {
        return true;
    }
    if error
        .data
        .as_ref()
        .and_then(|d| d.get("grpc_code"))
        .and_then(Value::as_str)
        == Some("Cancelled")
        && error.message.ends_with(": operation was canceled")
    {
        return true;
    }
    if matches!(error.code, ErrorCode::NotFound | ErrorCode::Busy)
        && error.data.as_ref().and_then(|d| d.get("serial")).is_some()
    {
        return true;
    }
    matches!(
        error.code,
        ErrorCode::Sdk | ErrorCode::Busy | ErrorCode::NotFound
    ) && [
        "device offline",
        "device not found",
        "no devices/emulators found",
        "device still authorizing",
    ]
    .iter()
    .any(|s| error.message.to_lowercase().contains(s))
}

impl Host<'_> {
    async fn reconnect(
        &mut self,
        started: std::time::Instant,
        original: &MikoError,
        cancel: &Cancel,
    ) -> MikoResult<()> {
        self.ctx.report(None, "Waiting for device reconnect");
        while started.elapsed() < Duration::from_secs(30) {
            self.ctx.cancel.check()?;
            cancel.check()?;
            tokio::time::sleep(Duration::from_millis(500)).await;
            self.ctx.cancel.check()?;
            cancel.check()?;
            let info = self.ctx.daemon.connect_info(self.connection.device).await?;
            if info != self.connection {
                return Err(MikoError::unexpected(
                    "device connection changed; restart the workflow",
                ));
            }
            if self.device.reconnect(&info).await.is_ok() {
                return Ok(());
            }
        }
        Err(original.clone())
    }
}

#[async_trait]
trait ScreenshotRecovery: Send {
    async fn capture(&mut self) -> MikoResult<Vec<u8>>;
    async fn restore(
        &mut self,
        started: std::time::Instant,
        error: &MikoError,
        cancel: &Cancel,
    ) -> MikoResult<()>;
}

#[async_trait]
impl ScreenshotRecovery for Host<'_> {
    async fn capture(&mut self) -> MikoResult<Vec<u8>> {
        self.device.screenshot_png().await
    }
    async fn restore(
        &mut self,
        started: std::time::Instant,
        error: &MikoError,
        cancel: &Cancel,
    ) -> MikoResult<()> {
        self.reconnect(started, error, cancel).await
    }
}

async fn capture_with_recovery(
    host: &mut impl ScreenshotRecovery,
    cancel: &Cancel,
) -> MikoResult<Vec<u8>> {
    let mut started = None;
    loop {
        cancel.check()?;
        let result = {
            let capture = host.capture();
            tokio::pin!(capture);
            loop {
                tokio::select! {
                    result = &mut capture => break result,
                    () = tokio::time::sleep(Duration::from_millis(20)) => { cancel.check()?; }
                }
            }
        };
        match result {
            Err(error) if connection_lost(&error) => {
                let since = *started.get_or_insert_with(std::time::Instant::now);
                host.restore(since, &error, cancel).await?;
            }
            result => return result,
        }
    }
}

#[async_trait]
impl FlowHost for Host<'_> {
    async fn find(&mut self, locator: &Locator) -> MikoResult<Option<Point>> {
        let mut started = None;
        loop {
            match self.device.find(locator).await {
                Err(error) if connection_lost(&error) => {
                    let since = *started.get_or_insert_with(std::time::Instant::now);
                    let cancel = self.ctx.cancel.clone();
                    self.reconnect(since, &error, &cancel).await?;
                }
                result => return result,
            }
        }
    }
    async fn action(&mut self, action: &FlowAction, cancel: &Cancel) -> MikoResult<()> {
        let mut started = None;
        loop {
            match self.device.action(action, cancel).await {
                Err(error) if connection_lost(&error) => {
                    cancel.check()?;
                    let since = *started.get_or_insert_with(std::time::Instant::now);
                    self.reconnect(since, &error, cancel).await?;
                }
                result => return result,
            }
        }
    }
    async fn checkpoint(&mut self, node: &str, cancel: &Cancel) -> MikoResult<()> {
        loop {
            cancel.check()?;
            let proceed = {
                let mut s = crate::state::lock(&self.session.state);
                s.current = node.to_owned();
                if !s.paused {
                    true
                } else if s.steps > 0 {
                    s.steps -= 1;
                    true
                } else {
                    false
                }
            };
            self.ctx.report(None, &format!("flow node {node}"));
            if proceed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if self.screenshots {
            let png = capture_with_recovery(self, cancel).await?;
            tokio::fs::write(
                self.dir
                    .join(format!("node-{}-{}.png", node, Uuid::new_v4())),
                png,
            )
            .await
            .map_err(|e| MikoError::unexpected(format!("saving flow screenshot: {e}")))?;
        }
        Ok(())
    }
}

fn safe_error(mut error: MikoError) -> MikoError {
    let mut data = error.data.take().unwrap_or_else(|| json!({}));
    if let Some(map) = data.as_object_mut() {
        map.insert("input_released".into(), json!(true));
    }
    error.data = Some(data);
    error
}

/// Records a rejected or failed start even when no node has executed.
pub(crate) async fn log_start_failure(
    dir: &std::path::Path,
    name: &str,
    elapsed_ms: u64,
    error: &mut MikoError,
) {
    let report = miko_automation::FlowReport {
        name: name.to_owned(),
        elapsed_ms,
        logs: vec![miko_automation::FlowLog {
            node: "startup".into(),
            elapsed_ms,
            outcome: error.message.clone(),
        }],
        failed_node: Some("startup".into()),
        error: Some(error.message.clone()),
    };
    let written = async {
        tokio::fs::create_dir_all(dir).await?;
        let bytes = serde_json::to_vec_pretty(&report).map_err(std::io::Error::other)?;
        tokio::fs::write(dir.join("report.json"), bytes).await
    }
    .await;
    let data = error.data.get_or_insert_with(|| json!({}));
    if let Some(map) = data.as_object_mut() {
        match written {
            Ok(()) => {
                map.insert("log_dir".into(), json!(dir));
            }
            Err(e) => {
                map.insert("log_error".into(), json!(e.to_string()));
            }
        }
    }
}

impl FlowProvider {
    async fn run(&self, ctx: &CallCtx, params: Value) -> MikoResult<Value> {
        let started = std::time::Instant::now();
        let name = params
            .pointer("/flow/name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let dir = ctx
            .daemon
            .paths()
            .logs
            .join("flows")
            .join(Uuid::new_v4().to_string());
        let mut execution_started = false;
        let result = self
            .run_inner(ctx, params, &dir, &mut execution_started)
            .await;
        match result {
            Err(mut error) => {
                if !execution_started && !dir.join("report.json").exists() {
                    log_start_failure(
                        &dir,
                        &name,
                        started.elapsed().as_millis() as u64,
                        &mut error,
                    )
                    .await;
                }
                Err(error)
            }
            result => result,
        }
    }

    async fn run_inner(
        &self,
        ctx: &CallCtx,
        params: Value,
        dir: &std::path::Path,
        execution_started: &mut bool,
    ) -> MikoResult<Value> {
        tokio::fs::create_dir_all(dir).await.map_err(|error| {
            safe_error(MikoError::unexpected(format!("creating flow log: {error}")))
        })?;
        let p: RunParams = parse_params(params).map_err(safe_error)?;
        p.flow.validate().map_err(safe_error)?;
        let dev = ctx.daemon.device(&p.device).await.map_err(safe_error)?;
        let info = ctx.daemon.connect_info(dev.id).await.map_err(safe_error)?;
        let device = FlowDevice::new(info.clone(), ctx.daemon.paths().data.clone())
            .await
            .map_err(safe_error)?;
        let session = Arc::new(Session {
            state: Mutex::new(SessionState {
                paused: p.start_paused,
                ..Default::default()
            }),
            cancel: ctx.cancel.clone(),
        });
        {
            let mut sessions = crate::state::lock(&self.sessions);
            if sessions.contains_key(&dev.id) {
                return Err(safe_error(MikoError::busy("workflow already running")));
            }
            sessions.insert(dev.id, session.clone());
        }
        let mut host = Host {
            device,
            session,
            ctx,
            screenshots: p.screenshots,
            dir: dir.to_path_buf(),
            connection: info,
        };
        *execution_started = true;
        let result = run_flow(&p.flow, &mut host, &ctx.cancel).await;
        let release = host.device.release().await;
        crate::state::lock(&self.sessions).remove(&dev.id);
        let mut report = match result {
            Ok(report) => report,
            Err(error) => {
                release?;
                return Err(safe_error(error));
            }
        };
        if let Err(error) = release.as_ref() {
            report.error = Some(format!("input release failed: {}", error.message));
        }
        let bytes =
            serde_json::to_vec_pretty(&report).map_err(|e| MikoError::unexpected(e.to_string()))?;
        let saved = tokio::fs::write(dir.join("report.json"), bytes)
            .await
            .map_err(|e| MikoError::unexpected(format!("saving flow log: {e}")));
        if let Err(mut error) = release {
            error.data = Some(json!({"report":report,"log_dir":dir}));
            return Err(error);
        }
        saved.map_err(safe_error)?;
        if let Some(error) = report.error.as_ref() {
            let mut err = if ctx.cancel.is_cancelled() {
                miko_core::cancel::cancelled()
            } else {
                MikoError::unexpected(error)
            };
            err.data = Some(
                json!({"input_released":true,"cancelled":ctx.cancel.is_cancelled(),"report":report,"log_dir":dir}),
            );
            return Err(err);
        }
        Ok(json!({"report": report, "log_dir": dir}))
    }
}

#[async_trait]
impl CommandProvider for FlowProvider {
    fn name(&self) -> &str {
        "flow"
    }
    fn commands(&self) -> Vec<CommandSpec> {
        vec![
            CommandSpec::new::<RunParams>("flow.run", "Run a conditional workflow on one device")
                .long_running(),
            CommandSpec::new::<ValidateParams>("flow.validate", "Validate a conditional workflow"),
            CommandSpec::new::<StatusParams>("flow.status", "Get the device workflow state"),
            CommandSpec::new::<ControlParams>(
                "flow.control",
                "Pause, resume, step or stop a workflow",
            ),
        ]
    }
    async fn call(&self, ctx: &CallCtx, method: &str, params: Value) -> MikoResult<Value> {
        match method {
            "flow.validate" => {
                let p: ValidateParams = parse_params(params)?;
                p.flow.validate()?;
                Ok(json!({"valid":true}))
            }
            "flow.run" => self.run(ctx, params).await,
            "flow.status" | "flow.control" => {
                let (device, command) = if method == "flow.control" {
                    let p: ControlParams = parse_params(params)?;
                    (p.device, Some(p.command))
                } else {
                    let p: StatusParams = parse_params(params)?;
                    (p.device, None)
                };
                let dev = ctx.daemon.device(&device).await?;
                let session = crate::state::lock(&self.sessions)
                    .get(&dev.id)
                    .cloned()
                    .ok_or_else(|| MikoError::not_found("no running workflow"))?;
                let mut state = crate::state::lock(&session.state);
                if let Some(command) = command {
                    match command.as_str() {
                        "pause" => {
                            state.paused = true;
                            state.steps = 0;
                        }
                        "resume" => {
                            state.paused = false;
                            state.steps = 0;
                        }
                        "step" => {
                            state.paused = true;
                            state.steps = 1;
                        }
                        "stop" => session.cancel.cancel(),
                        _ => {
                            return Err(MikoError::usage(
                                "command must be pause, resume, step or stop",
                            ));
                        }
                    }
                }
                Ok(json!({"device":dev.id,"paused":state.paused,"node":state.current}))
            }
            _ => Err(MikoError::usage("unknown workflow command")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct DisconnectedScreenshot {
        attempts: u32,
        reconnects: u32,
    }
    #[async_trait]
    impl ScreenshotRecovery for DisconnectedScreenshot {
        async fn capture(&mut self) -> MikoResult<Vec<u8>> {
            self.attempts += 1;
            if self.attempts == 1 {
                Err(MikoError::sdk("disconnected").with_data(json!({"grpc_code":"Unavailable"})))
            } else {
                Ok(vec![1, 2, 3])
            }
        }
        async fn restore(
            &mut self,
            _: std::time::Instant,
            _: &MikoError,
            cancel: &Cancel,
        ) -> MikoResult<()> {
            cancel.check()?;
            self.reconnects += 1;
            Ok(())
        }
    }

    #[tokio::test]
    async fn node_screenshot_recovers_after_disconnect() {
        let mut screenshot = DisconnectedScreenshot::default();
        assert_eq!(
            capture_with_recovery(&mut screenshot, &Cancel::new())
                .await
                .unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!((screenshot.attempts, screenshot.reconnects), (2, 1));
    }

    #[test]
    fn reconnect_only_for_transient_and_safe_failures() {
        let transport = MikoError::sdk("flow screenshot: transport error")
            .with_data(json!({"grpc_code":"Unknown"}));
        assert!(connection_lost(&transport));
        let other = MikoError::sdk("flow screenshot: invalid response")
            .with_data(json!({"grpc_code":"Unknown"}));
        assert!(!connection_lost(&other));
        let cancelled_transport = MikoError::sdk("flow screenshot: operation was canceled")
            .with_data(json!({"grpc_code":"Cancelled"}));
        assert!(connection_lost(&cancelled_transport));
        let offline = MikoError::sdk("adb: device offline");
        assert!(connection_lost(&offline));
        let grpc = MikoError::sdk("unavailable").with_data(json!({"grpc_code":"Unavailable"}));
        assert!(connection_lost(&grpc));
        let missing = MikoError::not_found("device 'emulator-5554' not found")
            .with_data(json!({"serial":"emulator-5554"}));
        assert!(connection_lost(&missing));
        let uncertain = MikoError::sdk("device offline").with_data(json!({"flow_fatal":true}));
        assert!(!connection_lost(&uncertain));
        assert!(!connection_lost(&MikoError::not_found("script missing")));
    }
}

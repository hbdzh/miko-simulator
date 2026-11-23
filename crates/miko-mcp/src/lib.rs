//! MCP server exposing Miko to AI agents (`miko mcp`, stdio).
//!
//! Generic tools cover every protocol command (`command_list`,
//! `command_run`, `jobs_*`); typed tools cover the hot paths of driving a
//! device (`screenshot`, `tap`, `swipe`, `type_text`, `press_key`, `ui_dump`,
//! `logcat_tail`) and of a proxy for security testing (`proxy_set`,
//! `proxy_clear`, `ca_cert_install`). Destructive commands follow D5.

pub mod device;

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use miko_client::Client;
use miko_core::provider::CommandSpec;
use miko_core::{ConnectInfo, Device, MikoError, MikoResult};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::device::DeviceIo;

/// Methods that only make sense on a raw protocol connection.
const NOT_CALLABLE: &[&str] = &[
    "hello",
    "events.subscribe",
    "logcat.follow",
    "daemon.shutdown",
    "input.acquire",
    "input.release",
];
const DEFAULT_RUN_TIMEOUT_S: u64 = 900;
const UI_DUMP_LIMIT: usize = 400_000;
/// The Mac as the emulator sees it: where Burp or mitmproxy listens.
const PROXY_HOST: &str = "10.0.2.2";

#[cfg(test)]
mod handoff_tests {
    use super::*;
    use miko_daemon::testing::TestDaemon;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn server_shutdown_cancels_and_waits_for_input_cleanup() {
        let tmp = tempfile::tempdir().unwrap();
        let daemon = TestDaemon::start(tmp.path()).await.unwrap();
        let agent = Client::connect(&daemon.paths.socket(), "agent")
            .await
            .unwrap();
        let player = Client::connect(&daemon.paths.socket(), "player")
            .await
            .unwrap();
        player
            .call("device.create", json!({"name":"P","image":"35"}))
            .await
            .unwrap();
        let mcp = MikoMcp::new(agent, McpOptions::default());
        let tasks = mcp.input_tasks.clone();
        let (started_tx, started_rx) = oneshot::channel();
        let (cancelled_tx, cancelled_rx) = oneshot::channel();
        let (released_tx, released_rx) = oneshot::channel();
        let request = tokio::spawn(async move {
            mcp.run_input("P", move |mut cancel| async move {
                let _ = started_tx.send(());
                cancel.changed().await.unwrap();
                let _ = cancelled_tx.send(());
                released_rx.await.unwrap();
                Ok(())
            })
            .await
        });
        started_rx.await.unwrap();
        let shutdown = tokio::spawn({
            let tasks = tasks.clone();
            async move { tasks.shutdown().await }
        });
        tokio::time::timeout(Duration::from_secs(2), cancelled_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(!shutdown.is_finished());
        assert!(tasks.begin().is_none());
        released_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), shutdown)
            .await
            .unwrap()
            .unwrap();
        assert!(request.await.unwrap().is_err());
        let lease = player
            .call("input.acquire", json!({"device":"P"}))
            .await
            .unwrap();
        player
            .call(
                "input.release",
                json!({"device":"P","token":lease["token"]}),
            )
            .await
            .unwrap();
        daemon.stop().await;
    }

    #[tokio::test]
    async fn dropped_request_finishes_release_before_unlocking() {
        let tmp = tempfile::tempdir().unwrap();
        let daemon = TestDaemon::start(tmp.path()).await.unwrap();
        let agent = Client::connect(&daemon.paths.socket(), "agent")
            .await
            .unwrap();
        let player = Client::connect(&daemon.paths.socket(), "player")
            .await
            .unwrap();
        player
            .call("device.create", json!({"name":"P","image":"35"}))
            .await
            .unwrap();
        let mcp = MikoMcp::new(agent, McpOptions::default());
        let tasks = mcp.input_tasks.clone();
        let (started_tx, started_rx) = oneshot::channel();
        let (cancelled_tx, cancelled_rx) = oneshot::channel();
        let (released_tx, released_rx) = oneshot::channel();
        let request = tokio::spawn(async move {
            mcp.run_input("P", move |mut cancel| async move {
                let _ = started_tx.send(());
                cancel.changed().await.unwrap();
                let _ = cancelled_tx.send(());
                released_rx.await.unwrap();
                Ok(())
            })
            .await
        });
        started_rx.await.unwrap();
        request.abort();
        tokio::time::timeout(Duration::from_secs(2), cancelled_rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            player
                .call("input.acquire", json!({"device":"P"}))
                .await
                .unwrap_err()
                .code,
            miko_core::ErrorCode::Busy
        );
        released_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), tasks.shutdown())
            .await
            .unwrap();
        let lease = player
            .call("input.acquire", json!({"device":"P"}))
            .await
            .unwrap();
        player
            .call(
                "input.release",
                json!({"device":"P","token":lease["token"]}),
            )
            .await
            .unwrap();
        daemon.stop().await;
    }

    #[tokio::test]
    async fn takeover_waits_for_cancelled_operation_to_finish_releasing() {
        let tmp = tempfile::tempdir().unwrap();
        let daemon = TestDaemon::start(tmp.path()).await.unwrap();
        let agent = Client::connect(&daemon.paths.socket(), "agent")
            .await
            .unwrap();
        let player = Arc::new(
            Client::connect(&daemon.paths.socket(), "player")
                .await
                .unwrap(),
        );
        player
            .call("device.create", json!({"name":"P","image":"35"}))
            .await
            .unwrap();
        let mcp = MikoMcp::new(agent, McpOptions::default());
        let (started_tx, started_rx) = oneshot::channel();
        let (cancelled_tx, cancelled_rx) = oneshot::channel();
        let (released_tx, released_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            mcp.run_input("P", move |mut cancel| async move {
                let _ = started_tx.send(());
                cancel.changed().await.unwrap();
                let _ = cancelled_tx.send(());
                released_rx.await.unwrap();
                Ok(())
            })
            .await
        });
        started_rx.await.unwrap();
        let takeover = {
            let player = player.clone();
            tokio::spawn(async move {
                player
                    .call("input.acquire", json!({"device":"P","takeover":true}))
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(2), cancelled_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !takeover.is_finished(),
            "input must remain owned until release completes"
        );
        released_tx.send(()).unwrap();
        assert!(task.await.unwrap().is_err());
        let lease = tokio::time::timeout(Duration::from_secs(2), takeover)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        player
            .call(
                "input.release",
                json!({"device":"P","token":lease["token"]}),
            )
            .await
            .unwrap();
        daemon.stop().await;
    }
}

#[derive(Debug, Clone, Default)]
pub struct McpOptions {
    /// Allow destructive commands on devices the agent doesn't own (D5).
    pub allow_destructive: bool,
}

pub struct MikoMcp {
    client: Arc<Client>,
    opts: McpOptions,
    input_tasks: Arc<InputTasks>,
}

struct InputTasks {
    state: std::sync::Mutex<(bool, usize)>,
    finished: tokio::sync::Notify,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl InputTasks {
    fn new() -> Self {
        Self {
            state: std::sync::Mutex::new((false, 0)),
            finished: tokio::sync::Notify::new(),
            shutdown: tokio::sync::watch::channel(false).0,
        }
    }

    async fn shutdown(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).0 = true;
        self.shutdown.send_replace(true);
        loop {
            let finished = self.finished.notified();
            if self.state.lock().unwrap_or_else(|e| e.into_inner()).1 == 0 {
                return;
            }
            finished.await;
        }
    }

    fn begin(self: &Arc<Self>) -> Option<InputTaskGuard> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.0 {
            return None;
        }
        state.1 += 1;
        Some(InputTaskGuard(self.clone()))
    }
}

struct InputTaskGuard(Arc<InputTasks>);
impl Drop for InputTaskGuard {
    fn drop(&mut self) {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner()).1 -= 1;
        self.0.finished.notify_one();
    }
}

impl std::fmt::Debug for MikoMcp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MikoMcp")
            .field("opts", &self.opts)
            .finish_non_exhaustive()
    }
}

/// Serves MCP on stdin/stdout until the client disconnects.
pub async fn serve_stdio(client: Client, opts: McpOptions) -> MikoResult<()> {
    let handler = MikoMcp::new(client, opts);
    let input_tasks = handler.input_tasks.clone();
    let service = handler
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| MikoError::unexpected(format!("MCP initialize failed: {e}")))?;
    let result = service
        .waiting()
        .await
        .map_err(|e| MikoError::unexpected(format!("MCP server: {e}")));
    input_tasks.shutdown().await;
    result.map(|_| ())
}

// ---- tool params --------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CommandListArgs {
    /// Only commands whose id starts with this, e.g. `device.`.
    #[serde(default)]
    prefix: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CommandRunArgs {
    /// Command id from command_list, e.g. `device.start`.
    command: String,
    /// Params object matching the command's schema.
    #[serde(default)]
    params: Option<Value>,
    /// For long-running commands: wait at most this long (default 900 s),
    /// then return the job so it can be checked with jobs_list.
    #[serde(default)]
    timeout_s: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PoolGetArgs {
    /// Base name or id (`command_run base.list`).
    base: String,
    /// Lease lifetime in seconds. Without it the device is yours until this
    /// MCP session ends (plus 2 minutes).
    #[serde(default)]
    ttl_s: Option<u64>,
    /// How long to wait for room when the machine is full (default 120 s).
    #[serde(default)]
    timeout_s: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PoolReleaseArgs {
    /// The lease id from pool_get.
    #[serde(default)]
    lease: Option<String>,
    /// Or the pool device's name.
    #[serde(default)]
    device: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct JobsListArgs {
    /// Include finished jobs.
    #[serde(default)]
    all: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct JobArgs {
    /// Job id.
    job: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeviceArgs {
    /// Device name or id.
    device: String,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Coords {
    /// Pixels of the screenshot tool's image (default).
    #[default]
    Screenshot,
    /// Raw device pixels.
    Device,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TapArgs {
    device: String,
    x: f64,
    y: f64,
    /// Coordinate space of x/y (default: the screenshot image).
    #[serde(default)]
    coords: Coords,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SwipeArgs {
    device: String,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    /// Gesture duration in ms (default 300).
    #[serde(default)]
    duration_ms: Option<u32>,
    #[serde(default)]
    coords: Coords,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TypeArgs {
    device: String,
    /// Text to type into the focused field (ASCII is reliable).
    text: String,
    /// Press Enter afterwards.
    #[serde(default)]
    enter: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct KeyArgs {
    device: String,
    /// `GoHome`, `GoBack`, `AppSwitch`, `Enter`, `Power`, `AudioVolumeUp`,
    /// `AudioVolumeDown`, `Tab`, `Backspace`, `ArrowUp`/`Down`/`Left`/`Right`.
    key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LogcatArgs {
    device: String,
    /// Number of recent lines (default 200).
    #[serde(default)]
    lines: Option<u32>,
    /// logcat filter specs, e.g. `["ActivityManager:I", "*:S"]`.
    #[serde(default)]
    filter: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProxyArgs {
    device: String,
    /// Proxy host as the device sees it (default `10.0.2.2`, this Mac).
    #[serde(default)]
    host: Option<String>,
    /// Proxy port, e.g. 8080 for Burp.
    port: u16,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CaCertArgs {
    /// A running Full-root (Magisk) device.
    device: String,
    /// Path on this Mac to the CA certificate, PEM or DER (Burp's
    /// `cacert.der`, `~/.mitmproxy/mitmproxy-ca-cert.pem`).
    path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SyncStartArgs {
    /// Running leader device (name or id). Input flows while its player window is open.
    leader: String,
    /// Running follower devices (names or ids).
    followers: Vec<String>,
    /// Optional sync options (`adaptation`, `sync_keyboard`, `sync_text`, `sync_tool_keys`, `lock_followers`, `jitter_ms`).
    #[serde(default)]
    options: Option<Value>,
}

fn schema<T: JsonSchema>() -> JsonObject {
    let mut v = serde_json::to_value(schemars::schema_for!(T))
        .unwrap_or_else(|_| json!({ "type": "object" }));
    if let Some(o) = v.as_object_mut() {
        o.remove("$schema");
        o.remove("title");
    }
    match v {
        Value::Object(o) => o,
        _ => JsonObject::new(),
    }
}

fn tool<T: JsonSchema>(name: &'static str, desc: &'static str, ann: ToolAnnotations) -> Tool {
    Tool::new(name, desc, Arc::new(schema::<T>())).with_annotations(ann)
}

fn read_only() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false)
}

fn interacts() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(false)
        .destructive(false)
        .idempotent(false)
        .open_world(false)
}

fn idempotent() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(false)
        .destructive(false)
        .idempotent(true)
        .open_world(false)
}

fn parse<T: serde::de::DeserializeOwned>(args: Option<JsonObject>) -> MikoResult<T> {
    miko_core::provider::parse_params(Value::Object(args.unwrap_or_default()))
}

/// A Miko error as a tool result the agent can read and act on.
fn tool_error(e: &MikoError) -> CallToolResult {
    let mut r = CallToolResult::error(vec![ContentBlock::text(format!(
        "{}: {}",
        e.code, e.message
    ))]);
    r.structured_content = Some(json!({ "code": e.code, "message": e.message, "data": e.data }));
    r
}

fn ok_json(v: Value) -> CallToolResult {
    let text = serde_json::to_string_pretty(&v).unwrap_or_default();
    let mut r = CallToolResult::success(vec![ContentBlock::text(text)]);
    r.structured_content = Some(if v.is_object() {
        v
    } else {
        json!({ "result": v })
    });
    r
}

impl MikoMcp {
    pub fn new(client: Client, opts: McpOptions) -> Self {
        Self {
            client: Arc::new(client),
            opts,
            input_tasks: Arc::new(InputTasks::new()),
        }
    }

    pub fn tools() -> Vec<Tool> {
        vec![
            tool::<CommandListArgs>(
                "command_list",
                "List every Miko command (devices, SDK, images, root, sensors, snapshots…) with its params schema and flags. Use with command_run.",
                read_only(),
            ),
            tool::<CommandRunArgs>(
                "command_run",
                "Run any Miko command by id, e.g. {\"command\":\"device.start\",\"params\":{\"device\":\"Phone\"}}. Long-running commands are waited for. Destructive commands only work on devices you created (agent-owned) unless the server allows them.",
                ToolAnnotations::new()
                    .read_only(false)
                    .destructive(true)
                    .idempotent(false)
                    .open_world(false),
            ),
            tool::<PoolGetArgs>(
                "pool_get",
                "Get your own throwaway Android device: a fresh clone of a base, booted in seconds. Returns its serial; use that device, never the user's (e.g. not emulator-5554). Call pool_release when done.",
                ToolAnnotations::new()
                    .read_only(false)
                    .destructive(false)
                    .idempotent(false)
                    .open_world(false),
            ),
            tool::<PoolReleaseArgs>(
                "pool_release",
                "Give back a device from pool_get: it is stopped and deleted.",
                ToolAnnotations::new()
                    .read_only(false)
                    .destructive(true)
                    .idempotent(true)
                    .open_world(false),
            ),
            tool::<JobsListArgs>(
                "jobs_list",
                "Long-running operations (boots, installs) and their progress.",
                read_only(),
            ),
            tool::<JobArgs>(
                "jobs_cancel",
                "Cancel a running job; the device stays as it was before.",
                ToolAnnotations::new()
                    .read_only(false)
                    .destructive(false)
                    .idempotent(true)
                    .open_world(false),
            ),
            tool::<DeviceArgs>(
                "screenshot",
                "PNG of a running device's screen, fitted in 1024×1024. tap/swipe take coordinates in this image by default.",
                read_only(),
            ),
            tool::<TapArgs>("tap", "Tap a point on a running device.", interacts()),
            tool::<SwipeArgs>(
                "swipe",
                "Swipe/drag between two points (scroll, open the app drawer, …).",
                interacts(),
            ),
            tool::<TypeArgs>(
                "type_text",
                "Type text into the focused field of a running device.",
                interacts(),
            ),
            tool::<KeyArgs>(
                "press_key",
                "Press a device key: GoHome, GoBack, AppSwitch, Enter, Power, volume…",
                interacts(),
            ),
            tool::<DeviceArgs>(
                "ui_dump",
                "The on-screen UI hierarchy (uiautomator XML: text, resource-id, bounds) of a running device. Use bounds to find where to tap.",
                read_only(),
            ),
            tool::<LogcatArgs>(
                "logcat_tail",
                "Recent device log lines, optionally filtered.",
                read_only(),
            ),
            tool::<ProxyArgs>(
                "proxy_set",
                "Send a running device's HTTP(S) traffic through a proxy on this Mac (Burp, mitmproxy): host defaults to 10.0.2.2.",
                idempotent(),
            ),
            tool::<DeviceArgs>(
                "proxy_clear",
                "Remove a running device's HTTP proxy.",
                idempotent(),
            ),
            tool::<CaCertArgs>(
                "ca_cert_install",
                "Trust a CA certificate (Burp, mitmproxy) as a system cert on a running Full-root device, so apps accept the proxy. Restarts the device (about a minute) and verifies; root.cacert.list / .remove through command_run.",
                idempotent(),
            ),
            tool::<SyncStartArgs>(
                "sync_start",
                "Start an input synchronizer session (one session at a time): mirrors player input from leader to followers while the leader's player window is open.",
                interacts(),
            ),
            tool::<EmptyArgs>(
                "sync_stop",
                "Stop the active input synchronizer session.",
                idempotent(),
            ),
            tool::<EmptyArgs>(
                "sync_status",
                "Return the active input synchronizer session, or null.",
                read_only(),
            ),
        ]
    }

    async fn specs(&self) -> MikoResult<Vec<CommandSpec>> {
        self.client.call_as("commands.list", json!({})).await
    }

    async fn device(&self, selector: &str) -> MikoResult<Device> {
        let d: Value = self
            .client
            .call("device.details", json!({ "device": selector }))
            .await?;
        serde_json::from_value(d["device"].clone())
            .map_err(|e| MikoError::unexpected(e.to_string()))
    }

    async fn io(&self, selector: &str) -> MikoResult<DeviceIo> {
        let info: ConnectInfo = self
            .client
            .call_as("device.connect_info", json!({ "device": selector }))
            .await?;
        Ok(DeviceIo::new(info))
    }

    async fn run_input<T, F, Fut>(&self, device: &str, operation: F) -> MikoResult<T>
    where
        T: Send + 'static,
        F: FnOnce(tokio::sync::watch::Receiver<bool>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = MikoResult<T>> + Send + 'static,
    {
        let socket = self.client.socket().to_owned();
        let device = device.to_owned();
        // Dropping the request signals cancellation but must not drop the
        // input future: it may still need to send a touch-up to Android.
        let (request_alive, mut request_closed) = tokio::sync::oneshot::channel::<()>();
        let tasks = self.input_tasks.clone();
        let mut shutdown = tasks.shutdown.subscribe();
        let guard = tasks.begin().ok_or_else(miko_core::cancel::cancelled)?;
        let task = tokio::spawn(async move {
            let _guard = guard;
            Self::run_owned_input(
                &socket,
                &device,
                operation,
                &mut request_closed,
                &mut shutdown,
            )
            .await
        });
        let result = task
            .await
            .map_err(|error| MikoError::unexpected(format!("input task failed: {error}")))?;
        drop(request_alive);
        result
    }

    async fn run_owned_input<T, F, Fut>(
        socket: &std::path::Path,
        device: &str,
        operation: F,
        request_closed: &mut tokio::sync::oneshot::Receiver<()>,
        shutdown: &mut tokio::sync::watch::Receiver<bool>,
    ) -> MikoResult<T>
    where
        F: FnOnce(tokio::sync::watch::Receiver<bool>) -> Fut,
        Fut: std::future::Future<Output = MikoResult<T>>,
    {
        // A separate connection makes cancellation and parallel MCP calls
        // independently owned. A failed action leaves the owner abandoned.
        let client = Client::connect(socket, "miko-mcp-input").await?;
        let mut events = client.subscribe(&["input.revoke"]).await?;
        let acquired = client
            .call("input.acquire", json!({"device":device,"preemptible":true}))
            .await?;
        let token = acquired["token"]
            .as_str()
            .ok_or_else(|| MikoError::unexpected("invalid input ownership reply"))?;
        if *shutdown.borrow()
            || matches!(
                request_closed.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Closed)
            )
        {
            client
                .call("input.release", json!({"device":device,"token":token}))
                .await?;
            return Err(miko_core::cancel::cancelled());
        }
        let (cancel, cancelled) = tokio::sync::watch::channel(false);
        let operation = operation(cancelled);
        tokio::pin!(operation);
        let mut revoked = false;
        let mut connection_error = None;
        let result = loop {
            tokio::select! {
                result = &mut operation => break result?,
                _ = &mut *request_closed, if !revoked => {
                    revoked = true;
                    let _ = cancel.send(true);
                },
                changed = shutdown.changed(), if !revoked => {
                    if changed.is_err() || *shutdown.borrow() {
                        revoked = true;
                        let _ = cancel.send(true);
                    }
                },
                event = events.recv(), if !revoked => match event {
                    Ok(event) if event.topic == "input.revoke" && event.data["token"].as_str() == Some(token) => {
                        revoked = true;
                        let _ = cancel.send(true);
                    }
                    Ok(_) => {},
                    Err(error) => {
                        revoked = true;
                        connection_error = Some(MikoError::unexpected(format!("input handoff event failed: {error}")));
                        let _ = cancel.send(true);
                    },
                },
                () = client.wait_closed(), if connection_error.is_none() => {
                    revoked = true;
                    connection_error = Some(MikoError::unexpected("input ownership connection closed"));
                    let _ = cancel.send(true);
                },
            }
        };
        if let Some(error) = connection_error {
            return Err(error);
        }
        client
            .call("input.release", json!({"device":device,"token":token}))
            .await?;
        if revoked {
            return Err(miko_core::cancel::cancelled());
        }
        Ok(result)
    }

    /// D5: destructive commands need `--allow-destructive` or an agent-owned device.
    async fn check_policy(&self, spec: &CommandSpec, params: &Value) -> MikoResult<()> {
        if !spec.destructive || self.opts.allow_destructive {
            return Ok(());
        }
        let selector = params.get("device").and_then(Value::as_str);
        if let Some(sel) = selector
            && self.device(sel).await?.agent_owned
        {
            return Ok(());
        }
        Err(MikoError::usage(format!(
            "{} is destructive and {} isn't agent-owned; ask the user to run it, to start `miko mcp --allow-destructive`, or to mark the device agent-owned (`miko device edit <name> --agent-owned true`)",
            spec.id,
            selector.unwrap_or("the target")
        ))
        .with_data(json!({ "policy": "D5", "command": spec.id })))
    }

    async fn command_run(&self, args: CommandRunArgs) -> MikoResult<Value> {
        if NOT_CALLABLE.contains(&args.command.as_str()) {
            return Err(MikoError::usage(format!(
                "{} can't be called through MCP",
                args.command
            )));
        }
        let specs = self.specs().await?;
        let spec = specs.iter().find(|s| s.id == args.command).ok_or_else(|| {
            MikoError::usage(format!(
                "unknown command {}; see command_list",
                args.command
            ))
        })?;
        let mut params = args.params.unwrap_or_else(|| json!({}));
        // Devices an agent creates are its own (D5), unless it says otherwise.
        if spec.id == "device.create"
            && let Some(o) = params.as_object_mut()
        {
            o.entry("agent_owned").or_insert(json!(true));
        }
        self.check_policy(spec, &params).await?;
        let v = self.client.call(&spec.id, params).await?;
        let Some(job) = miko_client::pending_job(&v) else {
            return Ok(v);
        };
        let deadline =
            Instant::now() + Duration::from_secs(args.timeout_s.unwrap_or(DEFAULT_RUN_TIMEOUT_S));
        loop {
            let j = self
                .client
                .wait_job(job, Some(Duration::from_secs(5)))
                .await?;
            match j.state {
                miko_core::JobState::Succeeded => return Ok(j.result.unwrap_or(Value::Null)),
                miko_core::JobState::Failed => {
                    return Err(j
                        .error
                        .unwrap_or_else(|| MikoError::unexpected(format!("{} failed", spec.id))));
                }
                miko_core::JobState::Cancelled => return Err(miko_core::cancel::cancelled()),
                _ if Instant::now() >= deadline => {
                    return Ok(
                        json!({ "job": j, "pending": true, "note": "still running; check jobs_list or cancel with jobs_cancel" }),
                    );
                }
                _ => {}
            }
        }
    }

    async fn point(&self, io: &DeviceIo, x: f64, y: f64, coords: Coords) -> MikoResult<(u32, u32)> {
        match coords {
            Coords::Device => Ok((x.max(0.0).round() as u32, y.max(0.0).round() as u32)),
            Coords::Screenshot => Ok(device::to_device((x, y), io.screen_size().await?)),
        }
    }

    async fn dispatch(&self, name: &str, args: Option<JsonObject>) -> MikoResult<CallToolResult> {
        match name {
            "command_list" => {
                let a: CommandListArgs = parse(args)?;
                let specs: Vec<CommandSpec> = self
                    .specs()
                    .await?
                    .into_iter()
                    .filter(|s| a.prefix.as_deref().is_none_or(|p| s.id.starts_with(p)))
                    .collect();
                Ok(ok_json(json!({ "commands": specs })))
            }
            "command_run" => Ok(ok_json(self.command_run(parse(args)?).await?)),
            "pool_get" => {
                let a: PoolGetArgs = parse(args)?;
                let mut params = json!({ "base": a.base });
                if let Some(t) = a.ttl_s {
                    params["ttl_s"] = json!(t);
                }
                if let Some(t) = a.timeout_s {
                    params["timeout_s"] = json!(t);
                }
                let run = CommandRunArgs {
                    command: "pool.get".into(),
                    params: Some(params),
                    timeout_s: a.timeout_s.map(|t| t + 300),
                };
                Ok(ok_json(self.command_run(run).await?))
            }
            "pool_release" => {
                let a: PoolReleaseArgs = parse(args)?;
                let params = match (a.lease, a.device) {
                    (Some(l), None) => json!({ "lease": l }),
                    (None, Some(d)) => json!({ "device": d }),
                    _ => return Err(MikoError::usage("give either lease or device")),
                };
                let run = CommandRunArgs {
                    command: "pool.release".into(),
                    params: Some(params),
                    timeout_s: None,
                };
                Ok(ok_json(self.command_run(run).await?))
            }
            "jobs_list" => {
                let a: JobsListArgs = parse(args)?;
                Ok(ok_json(
                    json!({ "jobs": self.client.call("jobs.list", json!({ "all": a.all })).await? }),
                ))
            }
            "jobs_cancel" => {
                let a: JobArgs = parse(args)?;
                Ok(ok_json(
                    self.client
                        .call("jobs.cancel", json!({ "job": a.job }))
                        .await?,
                ))
            }
            "screenshot" => {
                let a: DeviceArgs = parse(args)?;
                let io = self.io(&a.device).await?;
                let (png, w, h) = io.screenshot().await?;
                let screen = io.screen_size().await.unwrap_or((w, h));
                let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
                let note = format!(
                    "Screenshot {w}×{h} of a {}×{} screen (×{:.3} to device pixels). tap/swipe use this image's coordinates by default.",
                    screen.0,
                    screen.1,
                    device::shot_scale(screen.0, screen.1)
                );
                let mut r = CallToolResult::success(vec![
                    ContentBlock::image(b64, "image/png"),
                    ContentBlock::text(note),
                ]);
                r.structured_content = Some(
                    json!({ "width": w, "height": h, "screen": { "width": screen.0, "height": screen.1 } }),
                );
                Ok(r)
            }
            "tap" => {
                let a: TapArgs = parse(args)?;
                let io = self.io(&a.device).await?;
                let (x, y) = self.point(&io, a.x, a.y, a.coords).await?;
                self.run_input(&a.device, move |_| async move { io.tap(x, y).await })
                    .await?;
                Ok(ok_json(json!({ "tapped": { "x": x, "y": y } })))
            }
            "swipe" => {
                let a: SwipeArgs = parse(args)?;
                let io = self.io(&a.device).await?;
                let from = self.point(&io, a.x1, a.y1, a.coords).await?;
                let to = self.point(&io, a.x2, a.y2, a.coords).await?;
                self.run_input(&a.device, move |cancel| async move {
                    io.swipe(
                        from,
                        to,
                        a.duration_ms.unwrap_or(300).clamp(10, 10_000),
                        cancel,
                    )
                    .await
                })
                .await?;
                Ok(ok_json(
                    json!({ "swiped": { "from": [from.0, from.1], "to": [to.0, to.1] } }),
                ))
            }
            "type_text" => {
                let a: TypeArgs = parse(args)?;
                device::validate_input_text(&a.text)?;
                let io = self.io(&a.device).await?;
                let text = a.text.clone();
                let enter = a.enter;
                self.run_input(&a.device, move |cancel| async move {
                    io.type_text(&text).await?;
                    if enter && !*cancel.borrow() {
                        io.press_key("Enter").await?;
                    }
                    Ok(())
                })
                .await?;
                Ok(ok_json(json!({ "typed": a.text.chars().count() })))
            }
            "press_key" => {
                let a: KeyArgs = parse(args)?;
                let io = self.io(&a.device).await?;
                let key = a.key.clone();
                self.run_input(&a.device, move |_| async move { io.press_key(&key).await })
                    .await?;
                Ok(ok_json(json!({ "pressed": a.key })))
            }
            "ui_dump" => {
                let a: DeviceArgs = parse(args)?;
                let mut xml = self.io(&a.device).await?.ui_dump().await?;
                if xml.len() > UI_DUMP_LIMIT {
                    let cut = (0..=UI_DUMP_LIMIT)
                        .rev()
                        .find(|i| xml.is_char_boundary(*i))
                        .unwrap_or(0);
                    xml.truncate(cut);
                    xml.push_str("\n<!-- truncated -->");
                }
                Ok(CallToolResult::success(vec![ContentBlock::text(xml)]))
            }
            "logcat_tail" => {
                let a: LogcatArgs = parse(args)?;
                let v = self
                    .client
                    .call("logcat.dump", json!({ "device": a.device, "lines": a.lines.unwrap_or(200), "filter": a.filter }))
                    .await?;
                Ok(CallToolResult::success(vec![ContentBlock::text(
                    v["text"].as_str().unwrap_or_default(),
                )]))
            }
            "proxy_set" => {
                let a: ProxyArgs = parse(args)?;
                let host = a.host.as_deref().unwrap_or(PROXY_HOST);
                Ok(ok_json(
                    self.client
                        .call(
                            "network.proxy.set",
                            json!({ "device": a.device, "host": host, "port": a.port }),
                        )
                        .await?,
                ))
            }
            "proxy_clear" => {
                let a: DeviceArgs = parse(args)?;
                Ok(ok_json(
                    self.client
                        .call("network.proxy.clear", json!({ "device": a.device }))
                        .await?,
                ))
            }
            "ca_cert_install" => {
                let a: CaCertArgs = parse(args)?;
                Ok(ok_json(
                    self.command_run(CommandRunArgs {
                        command: "root.cacert.install".into(),
                        params: Some(json!({ "device": a.device, "path": a.path })),
                        timeout_s: None,
                    })
                    .await?,
                ))
            }
            "sync_start" => {
                let a: SyncStartArgs = parse(args)?;
                let mut params = json!({ "leader": a.leader, "followers": a.followers });
                if let Some(opts) = a.options {
                    params["options"] = opts;
                }
                Ok(ok_json(self.client.call("sync.start", params).await?))
            }
            "sync_stop" => {
                let _: EmptyArgs = parse(args)?;
                Ok(ok_json(self.client.call("sync.stop", json!({})).await?))
            }
            "sync_status" => {
                let _: EmptyArgs = parse(args)?;
                Ok(ok_json(self.client.call("sync.status", json!({})).await?))
            }
            other => Err(MikoError::usage(format!("unknown tool {other}"))),
        }
    }
}

impl ServerHandler for MikoMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("miko", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Miko runs Android emulators (optionally rooted). For a device of your own, call pool_get \
                 {base} (command_run base.list shows the bases): it returns a throwaway device and its serial in \
                 seconds; use that serial, never the user's devices, and call pool_release when done. For a device \
                 that must persist, use command_run device.create {name, image: \"35\", profile: \"agent-lean\"} / \
                 device.start. Then screenshot → tap/swipe/type_text → ui_dump to find elements. If a start fails \
                 with resource_exhausted the machine is full: wait, release or hibernate devices you own \
                 (device.hibernate). command_list shows every command. Devices you create are yours to delete.",
            )
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(Self::tools())))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let name = request.name.to_string();
        if !Self::tools().iter().any(|t| t.name == name) {
            return Err(McpError::invalid_params(
                format!("unknown tool {name}"),
                None,
            ));
        }
        let started = Instant::now();
        let result = match self.dispatch(&name, request.arguments).await {
            Ok(r) => r,
            Err(e) => tool_error(&e),
        };
        tracing::debug!(tool = %name, ms = started.elapsed().as_millis() as u64, "tool call");
        Ok(CallToolResponse::Complete(result))
    }
}

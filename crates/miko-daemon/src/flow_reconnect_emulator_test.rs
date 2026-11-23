//! Real emulator screenshot recovery through an interruptible local TCP relay.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::{Daemon, DaemonConfig};
use miko_backend::testing::{AvdGuard, unique_avd};
use miko_core::MikoPaths;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream, UnixStream},
    sync::watch,
};

struct Client {
    read: BufReader<tokio::net::unix::OwnedReadHalf>,
    write: tokio::net::unix::OwnedWriteHalf,
    id: u32,
}
impl Client {
    async fn open(path: &std::path::Path) -> Self {
        let (read, write) = UnixStream::connect(path).await.unwrap().into_split();
        Self {
            read: BufReader::new(read),
            write,
            id: 0,
        }
    }
    async fn call(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        self.write
            .write_all(
                format!(
                    "{}\n",
                    json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params})
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        loop {
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(300), self.read.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["id"] == self.id {
                return response
                    .get("result")
                    .cloned()
                    .unwrap_or_else(|| panic!("{method}: {response}"));
            }
        }
    }
    async fn job(&mut self, id: &Value) -> Value {
        loop {
            let result = self
                .call("jobs.wait", json!({"job":id,"timeout_s":5}))
                .await;
            if matches!(
                result["state"].as_str(),
                Some("succeeded" | "failed" | "cancelled")
            ) {
                return result;
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "starts an isolated emulator and interrupts its gRPC connection"]
async fn screenshot_recovers_after_real_transport_disconnect() {
    let sdk = std::env::var_os("MIKO_TEST_SDK").expect("MIKO_TEST_SDK is required");
    let image = std::env::var("MIKO_TEST_IMAGE").expect("MIKO_TEST_IMAGE is required");
    let tmp = tempfile::Builder::new()
        .prefix("miko-reconnect-")
        .tempdir_in("/tmp")
        .unwrap();
    let paths = MikoPaths::under(tmp.path().join("m"));
    let daemon = Daemon::builder(
        DaemonConfig {
            paths: paths.clone(),
            studio_sdk: Some(sdk.into()),
            idle_timeout: None,
            sdk_repo: None,
        },
        Arc::new(miko_backend::AospBackend::new()),
    )
    .build()
    .unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(daemon.clone().serve(async {
        let _ = stopped.await;
    }));
    while !paths.socket().exists() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut c = Client::open(&paths.socket()).await;
    c.call(
        "hello",
        json!({"version":crate::VERSION,"client":"reconnect-test"}),
    )
    .await;
    let name = unique_avd("miko-reconnect");
    let device = c
        .call(
            "device.create",
            json!({"name":name,"image":image,"profile":"agent-lean","root_mode":"none"}),
        )
        .await;
    let guard = AvdGuard::new(device["avd"].as_str().unwrap());
    let id = uuid::Uuid::parse_str(device["id"].as_str().unwrap()).unwrap();
    let start = c.call("device.start", json!({"device":name})).await;
    assert_eq!(c.job(&start["job"]).await["state"], "succeeded");

    let original_port = daemon.state().instance(id).unwrap().grpc_port;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_port = listener.local_addr().unwrap().port();
    let (online, rx) = watch::channel(true);
    let rejected = Arc::new(AtomicUsize::new(0));
    let rejected_proxy = rejected.clone();
    let relay = tokio::spawn(async move {
        loop {
            let (mut client, _) = listener.accept().await.unwrap();
            let mut state = rx.clone();
            if !*state.borrow_and_update() {
                rejected_proxy.fetch_add(1, Ordering::SeqCst);
                continue;
            }
            tokio::spawn(async move {
                if let Ok(mut server) = TcpStream::connect(("127.0.0.1", original_port)).await {
                    tokio::select! { _ = tokio::io::copy_bidirectional(&mut client, &mut server) => {}, _ = state.changed() => {} }
                }
            });
        }
    });
    crate::state::lock(&daemon.state().instances)
        .get_mut(&id)
        .unwrap()
        .grpc_port = proxy_port;
    let flow = json!({"name":"screenshot-reconnect","nodes":[{"type":"wait","ms":1}]});
    let job = c
        .call(
            "flow.run",
            json!({"device":name,"flow":flow,"screenshots":true,"start_paused":true}),
        )
        .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let status = c.call("flow.status", json!({"device":name})).await;
            if status["node"] == "1" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    online.send(false).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    c.call("flow.control", json!({"device":name,"command":"step"}))
        .await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while rejected.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("screenshot reconnect did not try the interrupted transport");
    online.send(true).unwrap();
    let result = c.job(&job["job"]).await;
    println!("SCREENSHOT_RECONNECT {result}");
    assert_eq!(result["state"], "succeeded");
    let dir = std::path::Path::new(result["result"]["log_dir"].as_str().unwrap());
    assert!(std::fs::read_dir(dir).unwrap().any(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|e| e == "png")
    }));
    if let Some(apk) = std::env::var_os("MIKO_TEST_TOUCH_APK") {
        let install = c
            .call(
                "file.install",
                json!({"device":name,"apk":apk.to_string_lossy(),"replace":true}),
            )
            .await;
        if install["pending"] == true {
            assert_eq!(c.job(&install["job"]).await["state"], "succeeded");
        } else {
            assert!(install["installed"].is_string(), "install: {install}");
        }
        let info = c.call("device.connect_info", json!({"device":name})).await;
        let adb = info["adb_path"].as_str().unwrap();
        let serial = info["adb_serial"].as_str().unwrap();
        let run_adb = |args: &[&str]| {
            std::process::Command::new(adb)
                .args(["-s", serial])
                .args(args)
                .output()
                .unwrap()
        };
        assert!(
            run_adb(&["shell", "am", "start", "-n", "ly.miko.hello/.MainActivity"])
                .status
                .success()
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
        run_adb(&["logcat", "-c"]);
        let script = uuid::Uuid::new_v4();
        let root = paths.data.join("scripts");
        std::fs::create_dir_all(&root).unwrap();
        let document = json!({"format":"miko.operation-recording","version":2,"id":script,"name":"timing", "created_at":"2026-11-23T00:00:00Z","device":"test","display":{"width":720,"height":1280},"playback":{"mode":"count","count":1,"duration_ms":0,"interval_ms":0,"speed":1.0},"events":[{"t":0,"type":"touch","x":0.35,"y":0.55,"down":true},{"t":200,"type":"touch","x":0.35,"y":0.55,"down":false}]});
        std::fs::write(root.join(format!("{script}.json")), document.to_string()).unwrap();
        let flow = json!({"name":"timing", "nodes":[{"type":"action","action":{"type":"script","id":script}}]});
        let job = c.call("flow.run", json!({"device":name,"flow":flow})).await;
        assert_eq!(c.job(&job["job"]).await["state"], "succeeded");
        tokio::time::sleep(Duration::from_secs(1)).await;
        let output = run_adb(&["logcat", "-d", "-s", "MikoTouchProbe:I", "*:S"]);
        let log = String::from_utf8_lossy(&output.stdout);
        println!("SCRIPT_TIMING {log}");
        let time = |pattern: &str| {
            let line = log.lines().find(|line| line.contains(pattern)).unwrap();
            let value = line.split_whitespace().nth(1).unwrap();
            let mut parts = value.split(':');
            let h: f64 = parts.next().unwrap().parse().unwrap();
            let m: f64 = parts.next().unwrap().parse().unwrap();
            let s: f64 = parts.next().unwrap().parse().unwrap();
            h * 3600.0 + m * 60.0 + s
        };
        assert!(
            time("action=1") - time("action=0") >= 0.18,
            "script touch was shortened: {log}"
        );
    }
    crate::state::lock(&daemon.state().instances)
        .get_mut(&id)
        .unwrap()
        .grpc_port = original_port;
    relay.abort();
    // A process exit must cancel a paused workflow and free input for the
    // next emulator process, including when screenshots are enabled.
    let job = c
        .call(
            "flow.run",
            json!({"device":name,"flow":flow,"screenshots":true,"start_paused":true}),
        )
        .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let status = c.call("flow.status", json!({"device":name})).await;
            if status["node"] == "1" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let info = c.call("device.connect_info", json!({"device":name})).await;
    let killed = std::process::Command::new(info["adb_path"].as_str().unwrap())
        .args(["-s", info["adb_serial"].as_str().unwrap(), "emu", "kill"])
        .output()
        .unwrap();
    assert!(killed.status.success(), "adb emu kill: {killed:?}");
    let result = tokio::time::timeout(Duration::from_secs(30), c.job(&job["job"]))
        .await
        .expect("flow was not cancelled after emulator exit");
    println!("CRASH_FLOW {result}");
    assert_eq!(result["state"], "cancelled");
    let start = c.call("device.start", json!({"device":name})).await;
    let result = c.job(&start["job"]).await;
    println!("CRASH_RESTART {result}");
    assert_eq!(result["state"], "succeeded");
    let restarted_flow = json!({"name":"after-crash","nodes":[{"type":"action","action":{"type":"key","key":"volume_up"}}]});
    let job = c
        .call(
            "flow.run",
            json!({"device":name,"flow":restarted_flow,"screenshots":true}),
        )
        .await;
    let result = c.job(&job["job"]).await;
    println!("CRASH_RESTART_FLOW {result}");
    assert_eq!(result["state"], "succeeded");
    let dir = std::path::Path::new(result["result"]["log_dir"].as_str().unwrap());
    assert!(std::fs::read_dir(dir).unwrap().any(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|e| e == "png")
    }));
    c.call("device.stop", json!({"device":name})).await;
    drop(guard);
    let _ = stop.send(());
    server.await.unwrap().unwrap();
}

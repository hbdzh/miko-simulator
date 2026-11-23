//! Opt-in local two-device baseline. The SDK is read-only and AVDs are temporary.
#![cfg(feature = "emulator")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use miko_backend::testing::{AvdGuard, unique_avd};
use miko_core::MikoPaths;
use miko_daemon::{Daemon, DaemonConfig};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn crop_template(source: &std::path::Path, dest: &std::path::Path) {
    let decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(source).unwrap(),
    ));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        other => panic!("unexpected screenshot color: {other:?}"),
    };
    let (x, y, width, height) = (10usize, 60usize, 250usize, 100usize);
    let mut pixels = Vec::new();
    for row in y..y + height {
        let start = (row * info.width as usize + x) * channels;
        pixels.extend_from_slice(&buf[start..start + width * channels]);
    }
    let mut encoder = png::Encoder::new(
        std::fs::File::create(dest).unwrap(),
        width as u32,
        height as u32,
    );
    encoder.set_color(info.color_type);
    encoder.set_depth(info.bit_depth);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&pixels)
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "boots two real emulators and records a local resource baseline"]
async fn two_device_baseline() {
    let sdk = common::emulator::test_sdk();
    let image = common::emulator::test_image();
    let tmp = tempfile::Builder::new()
        .prefix("miko-multi-")
        .tempdir_in("/tmp")
        .unwrap();
    let paths = MikoPaths::under(tmp.path().join("m"));
    let daemon = Daemon::builder(
        DaemonConfig {
            paths: paths.clone(),
            studio_sdk: Some(sdk),
            idle_timeout: None,
            sdk_repo: None,
        },
        Arc::new(miko_backend::AospBackend::new()),
    )
    .build()
    .unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(daemon.clone().serve(async {
        let _ = stopped.await;
    }));
    while !paths.socket().exists() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut c = common::Conn::open(&paths.socket()).await;
    c.read_timeout = Duration::from_secs(300);
    c.ok(
        "hello",
        json!({"version":miko_daemon::VERSION,"client":"baseline"}),
    )
    .await;
    let names = [unique_avd("miko-baseline-a"), unique_avd("miko-baseline-b")];
    let mut guards = Vec::new();
    for name in &names {
        let device = c.ok("device.create", json!({"name":name,"image":image.package(),"profile":"agent-lean","root_mode":"none"})).await;
        guards.push(AvdGuard::new(device["avd"].as_str().unwrap()));
    }
    for name in &names {
        let began = Instant::now();
        c.run_long("device.start", json!({"device":name})).await;
        println!("BOOT {name} {:.2}s", began.elapsed().as_secs_f64());
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    if std::env::var_os("MIKO_BASELINE_INSTALL_ONLY").is_none() {
        println!("WARMUP 45s");
        tokio::time::sleep(Duration::from_secs(45)).await;
        for sample in 0..6 {
            daemon.state().sample_footprints().await;
            let budget = c.ok("host.budget", json!({})).await;
            println!("SAMPLE {sample} {budget}");
            let ps = std::process::Command::new("ps")
                .args(["-axo", "pid,ppid,%cpu,rss,command"])
                .output()
                .unwrap();
            for line in String::from_utf8_lossy(&ps.stdout).lines() {
                if line.contains("qemu-system") && names.iter().any(|name| line.contains(name)) {
                    let columns: Vec<_> = line.split_whitespace().take(4).collect();
                    println!("QEMU {sample} {}", columns.join(" "));
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
    if let Some(apk) = std::env::var_os("MIKO_BASELINE_APK") {
        let apk = std::path::PathBuf::from(apk);
        let apk = apk.to_str().expect("test APK path must be UTF-8");
        let missing = tmp.path().join("missing-test.apk");
        let error = c
            .err(
                "file.install",
                json!({"device":&names[0],"apk":missing,"replace":true}),
            )
            .await;
        println!("EXPECTED_INSTALL_FAILURE {error}");
        for name in &names {
            let result = c
                .run_long(
                    "file.install",
                    json!({"device":name,"apk":apk,"replace":true}),
                )
                .await;
            println!("INSTALL {name} {result}");
            let info = c.ok("device.connect_info", json!({"device":name})).await;
            let output = std::process::Command::new(info["adb_path"].as_str().unwrap())
                .args([
                    "-s",
                    info["adb_serial"].as_str().unwrap(),
                    "shell",
                    "pm",
                    "path",
                    "ly.miko.hello",
                ])
                .output()
                .unwrap();
            let path = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && path.contains("package:"),
                "installed package not found: {path}"
            );
            println!("VERIFY_INSTALL {name} package found");
        }
        if std::env::var_os("MIKO_BASELINE_FLOW").is_some() {
            for name in &names {
                let info = c.ok("device.connect_info", json!({"device":name})).await;
                let output = std::process::Command::new(info["adb_path"].as_str().unwrap())
                    .args([
                        "-s",
                        info["adb_serial"].as_str().unwrap(),
                        "shell",
                        "am",
                        "start",
                        "-n",
                        "ly.miko.hello/.MainActivity",
                    ])
                    .output()
                    .unwrap();
                assert!(output.status.success(), "launch test app: {:?}", output);
            }
            let locator = json!({"type":"ui", "text":"Hello from Miko", "resource_id":null});
            let flow = json!({"name":"real-device-flow", "nodes":[
                {"type":"wait_element", "locator":locator, "present":true, "timeout_ms":20000, "interval_ms":500},
                {"type":"action", "action":{"type":"tap_element", "locator":locator}},
                {"type":"branch", "locator":locator, "then_nodes":[{"type":"wait", "ms":1}], "else_nodes":[{"type":"wait_element", "locator":locator, "present":false, "timeout_ms":1000, "interval_ms":200}]}
            ]});
            let mut jobs = Vec::new();
            for name in &names {
                jobs.push(
                    c.ok(
                        "flow.run",
                        json!({"device":name,"flow":flow,"screenshots":true}),
                    )
                    .await["job"]
                        .clone(),
                );
            }
            let mut template = None;
            for job in &jobs {
                let result = c.wait_job(job).await;
                println!("FLOW_SUCCESS {result}");
                assert_eq!(result["state"], "succeeded");
                let dir = std::path::Path::new(result["result"]["log_dir"].as_str().unwrap());
                assert!(dir.join("report.json").exists());
                assert!(std::fs::read_dir(dir).unwrap().any(|entry| {
                    entry
                        .unwrap()
                        .path()
                        .extension()
                        .is_some_and(|ext| ext == "png")
                }));
                if template.is_none() {
                    let source = std::fs::read_dir(dir)
                        .unwrap()
                        .map(|entry| entry.unwrap().path())
                        .find(|path| {
                            path.file_name()
                                .is_some_and(|name| name.to_string_lossy().starts_with("node-3-"))
                        })
                        .unwrap();
                    let dest = tmp.path().join("template.png");
                    crop_template(&source, &dest);
                    template = Some(dest);
                }
            }
            let cli = std::env::current_dir()
                .unwrap()
                .join("../../target/debug/miko");
            let cli_flow = tmp.path().join("cli-flow.json");
            std::fs::write(
                &cli_flow,
                json!({"name":"cli-probe", "nodes":[{"type":"wait", "ms":1}]}).to_string(),
            )
            .unwrap();
            let cli_result = std::process::Command::new(&cli)
                .env("MIKO_HOME", tmp.path().join("m"))
                .args([
                    "flow",
                    "run",
                    cli_flow.to_str().unwrap(),
                    "--device",
                    &names[1],
                ])
                .output()
                .unwrap();
            assert!(
                cli_result.status.success(),
                "CLI: {}",
                String::from_utf8_lossy(&cli_result.stderr)
            );
            let cli_json: serde_json::Value = serde_json::from_slice(&cli_result.stdout).unwrap();
            let cli_job = c.wait_job(&cli_json[0]["job"]["job"]).await;
            println!("FLOW_CLI {cli_job}");
            assert_eq!(cli_job["state"], "succeeded");
            let python = r#"
import json, os, subprocess, sys
p=subprocess.Popen([sys.argv[1],'mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,bufsize=1)
def request(v):
 p.stdin.write(json.dumps(v)+'\n'); p.stdin.flush()
 while True:
  line=p.stdout.readline()
  if not line: raise RuntimeError('MCP exited')
  obj=json.loads(line)
  if obj.get('id') == v.get('id'): return obj
request({'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2025-03-26','capabilities':{},'clientInfo':{'name':'acceptance','version':'1'}}})
p.stdin.write(json.dumps({'jsonrpc':'2.0','method':'notifications/initialized'})+'\n'); p.stdin.flush()
result=request({'jsonrpc':'2.0','id':2,'method':'tools/call','params':{'name':'command_run','arguments':{'command':'flow.run','params':{'device':sys.argv[2],'flow':{'name':'mcp-probe','nodes':[{'type':'wait','ms':1}]}}}}})
print(json.dumps(result)); p.stdin.close(); p.wait(timeout=10)
if result.get('result',{}).get('isError') or 'error' in result: sys.exit(1)
"#;
            let mcp = std::process::Command::new("python3")
                .env("MIKO_HOME", tmp.path().join("m"))
                .args(["-c", python, cli.to_str().unwrap(), &names[1]])
                .output()
                .unwrap();
            println!("FLOW_MCP {}", String::from_utf8_lossy(&mcp.stdout));
            assert!(
                mcp.status.success(),
                "MCP: {}",
                String::from_utf8_lossy(&mcp.stderr)
            );
            let template_flow = json!({"name":"real-device-template", "nodes":[{"type":"action", "action":{"type":"tap_element", "locator":{"type":"template", "path":template.unwrap(), "threshold":1.0}}}]});
            let job = c
                .ok(
                    "flow.run",
                    json!({"device":&names[0], "flow":template_flow}),
                )
                .await;
            let result = c.wait_job(&job["job"]).await;
            println!("FLOW_TEMPLATE {result}");
            assert_eq!(result["state"], "succeeded");
            let missing = json!({"type":"ui", "text":"No such element", "resource_id":null});
            let failing = json!({"name":"real-device-retry", "nodes":[{"type":"retry", "max_attempts":2, "timeout_ms":20000, "interval_ms":200, "nodes":[{"type":"action", "action":{"type":"tap_element", "locator":missing}}]}]});
            let job = c
                .ok("flow.run", json!({"device":&names[0], "flow":failing}))
                .await;
            let result = c.wait_job(&job["job"]).await;
            println!("FLOW_EXPECTED_RETRY_FAILURE {result}");
            assert_eq!(result["state"], "failed");
            let report = &result["error"]["data"]["report"];
            assert!(
                report["error"].as_str().unwrap().contains("not found"),
                "{report}"
            );
            assert_eq!(
                report["logs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|log| log["node"] == "1.1")
                    .count(),
                2
            );
            let timeout_flow = json!({"name":"real-device-timeout", "nodes":[{"type":"wait_element", "locator":missing, "present":true, "timeout_ms":2500, "interval_ms":200}]});
            let job = c
                .ok("flow.run", json!({"device":&names[0], "flow":timeout_flow}))
                .await;
            let result = c.wait_job(&job["job"]).await;
            println!("FLOW_EXPECTED_TIMEOUT {result}");
            assert_eq!(result["state"], "failed");
            assert!(
                result["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("timed out")
            );
            let long = json!({"name":"takeover", "nodes":[{"type":"wait", "ms":30000}]});
            let job = c
                .ok("flow.run", json!({"device":&names[0], "flow":long}))
                .await;
            tokio::time::sleep(Duration::from_secs(1)).await;
            let mut manual = common::Conn::open(&paths.socket()).await;
            manual
                .ok(
                    "hello",
                    json!({"version":miko_daemon::VERSION,"client":"manual-test"}),
                )
                .await;
            let lease = manual
                .ok("input.acquire", json!({"device":&names[0],"takeover":true}))
                .await;
            let result = c.wait_job(&job["job"]).await;
            println!("FLOW_MANUAL_TAKEOVER {result}");
            assert_eq!(result["state"], "cancelled");
            assert_eq!(result["error"]["data"]["input_released"], true);
            manual
                .ok(
                    "input.release",
                    json!({"device":&names[0],"token":lease["token"]}),
                )
                .await;
        }
    }
    if std::env::var_os("MIKO_BASELINE_GUI").is_some() {
        println!(
            "GUI_READY home={} devices={},{}",
            tmp.path().join("m").display(),
            names[0],
            names[1]
        );
        for _ in 0..900 {
            if tmp.path().join("gui-done").exists() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    if !server.is_finished() {
        for name in &names {
            let result = c.call("device.stop", json!({"device":name})).await;
            println!("STOP {name} {result}");
            assert!(
                result.get("result").is_some(),
                "temporary device did not stop cleanly"
            );
        }
    } else {
        assert_eq!(
            daemon.state().running_count(),
            0,
            "daemon exited with devices running"
        );
    }
    drop(guards);
    let _ = stop.send(());
    server.await.unwrap().unwrap();
}

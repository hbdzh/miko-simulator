//! Workflow registration and input ownership without an emulator.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use common::Harness;
use serde_json::json;

#[tokio::test]
async fn failed_workflow_connection_returns_input_ownership() {
    let h = Harness::start().await;
    let mut first = h.client().await;
    first
        .ok("device.create", json!({"name":"P","image":"35"}))
        .await;
    first.run("device.start", json!({"device":"P"})).await;
    let flow = json!({"name":"example","nodes":[{"type":"wait","ms":1}]});
    first.ok("flow.validate", json!({"flow":flow})).await;
    let job = first
        .ok("flow.run", json!({"device":"P","flow":flow}))
        .await;
    let finished = first.ok("jobs.wait", json!({"job":job["job"]})).await;
    assert_eq!(finished["state"], "failed");
    assert_eq!(finished["error"]["data"]["input_released"], true);
    let log_dir = finished["error"]["data"]["log_dir"].as_str().unwrap();
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(std::path::Path::new(log_dir).join("report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report["name"], "example");
    assert_eq!(report["failed_node"], "startup");
    assert_eq!(report["error"], finished["error"]["message"]);
    let mut second = h.client().await;
    let lease = second.ok("input.acquire", json!({"device":"P"})).await;
    second
        .ok(
            "input.release",
            json!({"device":"P","token":lease["token"]}),
        )
        .await;
    let rejected = second
        .call("flow.run", json!({"device":"missing", "flow":flow}))
        .await;
    let log_dir = rejected["error"]["data"]["details"]["log_dir"]
        .as_str()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(std::path::Path::new(log_dir).join("report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report["failed_node"], "startup");
    assert_eq!(report["error"], rejected["error"]["message"]);
    h.stop().await;
}

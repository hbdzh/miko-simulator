//! CLI/daemon key requests share ownership with player input.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use miko_client::Client;
use miko_core::ErrorCode;
use miko_daemon::testing::TestDaemon;
use serde_json::json;

struct ControlledInput {
    started: tokio::sync::Notify,
    cancelled: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl miko_core::provider::CommandProvider for ControlledInput {
    fn name(&self) -> &str {
        "controlled-input"
    }
    fn commands(&self) -> Vec<miko_core::provider::CommandSpec> {
        use miko_core::provider::{CommandSpec, DeviceParams};
        vec![
            CommandSpec::new::<miko_widgets::devtools::KeyParams>("key.press", "test key"),
            CommandSpec::new::<DeviceParams>("input.gamepad.connect", "test pad").long_running(),
        ]
    }
    async fn call(
        &self,
        ctx: &miko_core::provider::CallCtx,
        method: &str,
        _: serde_json::Value,
    ) -> miko_core::MikoResult<serde_json::Value> {
        self.started.notify_one();
        if method == "input.gamepad.connect" {
            while !ctx.cancel.is_cancelled() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            self.cancelled.notify_one();
        }
        self.release.notified().await;
        if method == "input.gamepad.connect" {
            return Err(miko_core::cancel::cancelled()
                .with_data(json!({"cancelled":true,"input_released":true})));
        }
        Ok(json!({"key":"home"}))
    }
}

#[tokio::test]
async fn cli_input_handoff_waits_for_key_up_and_cancelled_gamepad_cleanup() {
    use std::{sync::Arc, time::Duration};
    for method in ["key.press", "input.gamepad.connect"] {
        let tmp = tempfile::tempdir().unwrap();
        let controlled = Arc::new(ControlledInput {
            started: Default::default(),
            cancelled: Default::default(),
            release: Default::default(),
        });
        let daemon = TestDaemon::start_with_providers(tmp.path(), vec![controlled.clone()])
            .await
            .unwrap();
        let cli = Arc::new(
            Client::connect(&daemon.paths.socket(), "cli")
                .await
                .unwrap(),
        );
        let player = Arc::new(
            Client::connect(&daemon.paths.socket(), "player")
                .await
                .unwrap(),
        );
        cli.call("device.create", json!({"name":"P","image":"35"}))
            .await
            .unwrap();
        let input = {
            let cli = cli.clone();
            tokio::spawn(async move {
                let params = if method == "key.press" {
                    json!({"device":"P","key":"home"})
                } else {
                    json!({"device":"P"})
                };
                cli.call(method, params).await
            })
        };
        tokio::time::timeout(Duration::from_secs(2), controlled.started.notified())
            .await
            .unwrap();
        let takeover = {
            let player = player.clone();
            tokio::spawn(async move {
                player
                    .call("input.acquire", json!({"device":"P","takeover":true}))
                    .await
            })
        };
        if method == "input.gamepad.connect" {
            tokio::time::timeout(Duration::from_secs(2), controlled.cancelled.notified())
                .await
                .unwrap();
        } else {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(!takeover.is_finished(), "handoff must await input release");
        controlled.release.notify_one();
        let lease = tokio::time::timeout(Duration::from_secs(2), takeover)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let response = input.await.unwrap().unwrap();
        if method == "input.gamepad.connect" {
            let result = cli
                .call("jobs.wait", json!({"job":response["job"],"timeout_s":2}))
                .await
                .unwrap();
            assert_eq!(result["state"], "cancelled");
        }
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

#[tokio::test]
async fn key_press_respects_player_ownership_and_rejects_stale_tokens() {
    let tmp = tempfile::tempdir().unwrap();
    let daemon = TestDaemon::start_with_providers(tmp.path(), vec![miko_widgets::provider()])
        .await
        .unwrap();
    let player = Client::connect(&daemon.paths.socket(), "player")
        .await
        .unwrap();
    let cli = Client::connect(&daemon.paths.socket(), "cli")
        .await
        .unwrap();
    player
        .call("device.create", json!({"name":"P","image":"35"}))
        .await
        .unwrap();
    let lease = player
        .call("input.acquire", json!({"device":"P"}))
        .await
        .unwrap();
    let token = lease["token"].as_str().unwrap();
    assert_eq!(
        cli.call("key.press", json!({"device":"P","key":"home"}))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    assert_eq!(
        cli.call(
            "key.press",
            json!({"device":"P","key":"home","input_lease":token})
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::Busy
    );
    // The owning bridge can route a tool key through the daemon using its
    // token; the fake stopped device fails after the ownership check.
    assert_eq!(
        player
            .call(
                "key.press",
                json!({"device":"P","key":"home","input_lease":token})
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    player
        .call("input.release", json!({"device":"P","token":token}))
        .await
        .unwrap();
    let next = cli
        .call("input.acquire", json!({"device":"P"}))
        .await
        .unwrap();
    assert_eq!(
        player
            .call("input.release", json!({"device":"P","token":token}))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    cli.call("input.release", json!({"device":"P","token":next["token"]}))
        .await
        .unwrap();
    assert_eq!(
        cli.call("key.press", json!({"device":"P","key":"invalid-key!"}))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Usage
    );
    let after_error = player
        .call("input.acquire", json!({"device":"P"}))
        .await
        .unwrap();
    player
        .call(
            "input.release",
            json!({"device":"P","token":after_error["token"]}),
        )
        .await
        .unwrap();
    daemon.stop().await;
}

#[tokio::test]
async fn disconnected_owner_is_fenced_until_device_is_stopped() {
    let tmp = tempfile::tempdir().unwrap();
    let daemon = TestDaemon::start_with_providers(tmp.path(), vec![miko_widgets::provider()])
        .await
        .unwrap();
    let player = Client::connect(&daemon.paths.socket(), "player")
        .await
        .unwrap();
    let cli = Client::connect(&daemon.paths.socket(), "cli")
        .await
        .unwrap();
    player
        .call("device.create", json!({"name":"P","image":"35"}))
        .await
        .unwrap();
    player
        .call("input.acquire", json!({"device":"P"}))
        .await
        .unwrap();
    drop(player);
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let error = cli
                .call("input.acquire", json!({"device":"P"}))
                .await
                .unwrap_err();
            if error.message.contains("disconnected") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    cli.call("device.stop", json!({"device":"P"}))
        .await
        .unwrap();
    let lease = cli
        .call("input.acquire", json!({"device":"P"}))
        .await
        .unwrap();
    cli.call(
        "input.release",
        json!({"device":"P","token":lease["token"]}),
    )
    .await
    .unwrap();
    daemon.stop().await;
}

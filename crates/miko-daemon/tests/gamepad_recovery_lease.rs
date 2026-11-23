//! Gamepad recovery gives up input and never preempts another owner.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use async_trait::async_trait;
use miko_core::MikoResult;
use miko_core::provider::{CallCtx, CommandProvider, CommandSpec, DeviceParams};
use miko_daemon::testing::TestDaemon;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

struct FakePad {
    paused: Semaphore,
    resume: Semaphore,
    resumed: Semaphore,
    finish: Semaphore,
}

#[async_trait]
impl CommandProvider for FakePad {
    fn name(&self) -> &str {
        "fake-pad"
    }
    fn commands(&self) -> Vec<CommandSpec> {
        vec![CommandSpec::new::<DeviceParams>("input.gamepad.connect", "fake").long_running()]
    }
    async fn call(&self, ctx: &CallCtx, _: &str, _: Value) -> MikoResult<Value> {
        let lease = ctx.input_lease.as_ref().unwrap();
        lease.suspend()?;
        self.paused.add_permits(1);
        self.resume.acquire().await.unwrap().forget();
        let resumed = lease.resume().is_ok();
        self.resumed.add_permits(1);
        self.finish.acquire().await.unwrap().forget();
        Ok(json!({"resumed": resumed}))
    }
}

async fn wait(signal: &Semaphore) {
    tokio::time::timeout(Duration::from_secs(3), signal.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
}

#[tokio::test]
async fn recovery_releases_and_reacquires_without_stealing_input() {
    let tmp = tempfile::tempdir().unwrap();
    let pad = Arc::new(FakePad {
        paused: Semaphore::new(0),
        resume: Semaphore::new(0),
        resumed: Semaphore::new(0),
        finish: Semaphore::new(0),
    });
    let daemon = TestDaemon::start_with_providers(tmp.path(), vec![pad.clone()])
        .await
        .unwrap();
    let mut first = common::Conn::open(&daemon.paths.socket()).await;
    let mut second = common::Conn::open(&daemon.paths.socket()).await;
    for c in [&mut first, &mut second] {
        c.ok(
            "hello",
            json!({"version":miko_daemon::VERSION,"client":"test"}),
        )
        .await;
    }
    first
        .ok("device.create", json!({"name":"P","image":"35"}))
        .await;

    let job = first
        .ok("input.gamepad.connect", json!({"device":"P"}))
        .await;
    wait(&pad.paused).await;
    let manual = second.ok("input.acquire", json!({"device":"P"})).await;
    pad.resume.add_permits(1);
    wait(&pad.resumed).await;
    pad.finish.add_permits(1);
    let finished = first.wait_job(&job["job"]).await;
    assert_eq!(finished["result"]["resumed"], false);
    second
        .ok(
            "input.release",
            json!({"device":"P","token":manual["token"]}),
        )
        .await;

    let job = first
        .ok("input.gamepad.connect", json!({"device":"P"}))
        .await;
    wait(&pad.paused).await;
    pad.resume.add_permits(1);
    wait(&pad.resumed).await;
    let blocked = second.call("input.acquire", json!({"device":"P"})).await;
    assert!(blocked.get("error").is_some());
    pad.finish.add_permits(1);
    let finished = first.wait_job(&job["job"]).await;
    assert_eq!(finished["result"]["resumed"], true);
    let manual = second.ok("input.acquire", json!({"device":"P"})).await;
    second
        .ok(
            "input.release",
            json!({"device":"P","token":manual["token"]}),
        )
        .await;
    let job = first
        .ok("input.gamepad.connect", json!({"device":"P"}))
        .await;
    wait(&pad.paused).await;
    drop(first);
    tokio::time::sleep(Duration::from_millis(100)).await;
    pad.resume.add_permits(1);
    wait(&pad.resumed).await;
    let blocked = second.call("input.acquire", json!({"device":"P"})).await;
    assert!(blocked.get("error").is_some());
    pad.finish.add_permits(1);
    let finished = second.wait_job(&job["job"]).await;
    assert_eq!(finished["result"]["resumed"], true);
    let manual = second.ok("input.acquire", json!({"device":"P"})).await;
    second
        .ok(
            "input.release",
            json!({"device":"P","token":manual["token"]}),
        )
        .await;
    daemon.stop().await;
}

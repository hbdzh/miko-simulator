//! Batch installation cancellation around the package-manager commit point.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;

use common::Harness;
use serde_json::json;
use std::time::Duration;

async fn wait_for(path: &std::path::Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[cfg(unix)]
fn install_fake_adb(
    h: &Harness,
    hold_push: bool,
    hold_install: bool,
    success: bool,
    cleanup_failure: bool,
) {
    use std::os::unix::fs::PermissionsExt;
    let root = h.tmp.path();
    let adb = root.join("studio/platform-tools/adb");
    let script = format!(
        r##"#!/bin/sh
while [ "$1" = "-s" ]; do shift 2; done
if [ "$1" = "push" ]; then
  touch '{push}'
  if [ '{hold_push}' = true ]; then while [ ! -f '{release}' ]; do sleep 0.02; done; fi
  exit 0
fi
if [ "$1" = "shell" ] && [ "$2" = "pm" ]; then
  touch '{commit}'
  if [ '{hold_install}' = true ]; then while [ ! -f '{release}' ]; do sleep 0.02; done; fi
  if [ '{success}' = true ]; then echo Success; exit 0; fi
  echo 'error: device offline' >&2; exit 1
fi
if [ "$1" = "shell" ]; then touch '{cleanup}'; if [ '{cleanup_failure}' = true ]; then exit 1; fi; exit 0; fi
exit 0
"##,
        push = root.join("push").display(),
        commit = root.join("commit").display(),
        cleanup = root.join("cleanup").display(),
        release = root.join("release").display()
    );
    std::fs::write(&adb, script).unwrap();
    std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_stops_staging_and_cleans_the_temporary_apk() {
    let h = Harness::start().await;
    let mut c = h.client().await;
    c.ok("device.create", json!({"name":"S","image":"35"}))
        .await;
    c.run("device.start", json!({"device":"S"})).await;
    install_fake_adb(&h, true, false, true, false);
    let apk = h.tmp.path().join("app.apk");
    std::fs::write(&apk, b"fake apk").unwrap();
    let job = c
        .ok("file.install_task", json!({"device":"S","apk":apk}))
        .await;
    wait_for(&h.tmp.path().join("push")).await;
    c.ok("jobs.cancel", json!({"job":job["job"]})).await;
    let result = c.wait_job(&job["job"]).await;
    assert_eq!(result["state"], "cancelled", "{result}");
    assert!(h.tmp.path().join("cleanup").exists());
    assert!(!h.tmp.path().join("commit").exists());
    h.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn cancelled_staging_reports_cleanup_failure() {
    let h = Harness::start().await;
    let mut c = h.client().await;
    c.ok("device.create", json!({"name":"S","image":"35"}))
        .await;
    c.run("device.start", json!({"device":"S"})).await;
    install_fake_adb(&h, true, false, true, true);
    let apk = h.tmp.path().join("app.apk");
    std::fs::write(&apk, b"fake apk").unwrap();
    let job = c
        .ok("file.install_task", json!({"device":"S","apk":apk}))
        .await;
    wait_for(&h.tmp.path().join("push")).await;
    c.ok("jobs.cancel", json!({"job":job["job"]})).await;
    let result = c.wait_job(&job["job"]).await;
    assert_eq!(result["state"], "failed", "{result}");
    assert_eq!(result["error"]["data"]["cleanup_failed"], true);
    h.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_after_install_commit_reports_observed_outcome() {
    for success in [true, false] {
        let h = Harness::start().await;
        let mut c = h.client().await;
        c.ok("device.create", json!({"name":"S","image":"35"}))
            .await;
        c.run("device.start", json!({"device":"S"})).await;
        install_fake_adb(&h, false, true, success, false);
        let apk = h.tmp.path().join("app.apk");
        std::fs::write(&apk, b"fake apk").unwrap();
        let job = c
            .ok("file.install_task", json!({"device":"S","apk":apk}))
            .await;
        wait_for(&h.tmp.path().join("commit")).await;
        c.ok("jobs.cancel", json!({"job":job["job"]})).await;
        std::fs::write(h.tmp.path().join("release"), b"").unwrap();
        let result = c.wait_job(&job["job"]).await;
        assert_eq!(
            result["state"],
            if success { "succeeded" } else { "failed" },
            "{result}"
        );
        if !success {
            assert!(
                result["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("installation result unknown")
            );
        }
        assert_eq!(h.tmp.path().join("cleanup").exists(), success);
        if !success {
            assert!(result["error"]["data"]["staged_apk"].as_str().is_some());
        }
        h.stop().await;
    }
}

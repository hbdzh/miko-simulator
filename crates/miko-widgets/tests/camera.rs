//! Camera file settings persist and reach the launch backend.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use miko_client::Client;
use miko_daemon::testing::TestDaemon;
use serde_json::json;

#[tokio::test]
async fn camera_file_paths_survive_settings_and_reach_launch() {
    let tmp = tempfile::tempdir().unwrap();
    let daemon = TestDaemon::start_with_providers(tmp.path(), vec![miko_widgets::provider()])
        .await
        .unwrap();
    let client = Client::connect(&daemon.paths.socket(), "test")
        .await
        .unwrap();
    client
        .call("device.create", json!({"name":"P", "image":"35"}))
        .await
        .unwrap();
    let file = tmp.path().join("camera image.png");
    std::fs::write(&file, b"image").unwrap();
    let path = file.to_str().unwrap();
    let set = client
        .call(
            "camera.set",
            json!({"device":"P", "back":"image", "back_file":path, "front":"default"}),
        )
        .await
        .unwrap();
    assert_eq!(set["back"], format!("imagefile:{path}"));
    assert_eq!(set["front"], "default");
    let video = tmp.path().join("camera video.mp4");
    std::fs::write(&video, b"video").unwrap();
    let front = client
        .call(
            "camera.set",
            json!({"device":"P", "front":"video", "front_file":video}),
        )
        .await
        .unwrap();
    assert_eq!(front["front"], format!("videofile:{}", video.display()));
    let get = client
        .call("camera.get", json!({"device":"P"}))
        .await
        .unwrap();
    assert_eq!(get["back"], set["back"]);
    client
        .run("device.start", json!({"device":"P"}), |_| {})
        .await
        .unwrap();
    let launches = daemon.backend.launches();
    let source = launches.last().unwrap().1.camera_back.as_deref().unwrap();
    #[cfg(unix)]
    {
        let staged = source.strip_prefix("imagefile:").unwrap();
        assert!(!staged.contains(' '));
        assert_eq!(std::fs::read_link(staged).unwrap(), file);
        let staged_front = launches
            .last()
            .unwrap()
            .1
            .camera_front
            .as_deref()
            .unwrap()
            .strip_prefix("videofile:")
            .unwrap();
        assert_eq!(std::fs::read_link(staged_front).unwrap(), video);
    }
    #[cfg(not(unix))]
    assert_eq!(source, format!("imagefile:{path}"));
    client
        .call("device.stop", json!({"device":"P"}))
        .await
        .unwrap();
    client
        .call("device.clone", json!({"device":"P", "name":"Copy"}))
        .await
        .unwrap();
    let copy = client
        .call("camera.get", json!({"device":"Copy"}))
        .await
        .unwrap();
    assert_eq!(copy["back"], set["back"]);
    assert_eq!(copy["front"], front["front"]);
    std::fs::remove_file(file).unwrap();
    let error = client
        .run("device.start", json!({"device":"P"}), |_| {})
        .await
        .unwrap_err();
    assert!(error.message.contains("camera file is missing"), "{error}");
    daemon.stop().await;
}

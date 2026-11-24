//! File camera configuration on a temporary real emulator.
#![cfg(feature = "emulator")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use serde_json::json;
use std::time::{Duration, Instant};

async fn assert_camera_is_red(env: &common::Env) {
    let info: miko_core::ConnectInfo =
        serde_json::from_value(env.call("device.connect_info", json!({})).await).unwrap();
    let adb = |args: &[&str]| {
        std::process::Command::new(&info.adb_path)
            .arg("-s")
            .arg(&info.adb_serial)
            .args(args)
            .output()
            .unwrap()
    };
    adb(&["shell", "pm clear com.android.camera2"]);
    adb(&[
        "shell",
        "pm grant com.android.camera2 android.permission.CAMERA",
    ]);
    adb(&[
        "shell",
        "pm grant com.android.camera2 android.permission.ACCESS_FINE_LOCATION",
    ]);
    adb(&[
        "shell",
        "pm grant com.android.camera2 android.permission.ACCESS_COARSE_LOCATION",
    ]);
    adb(&[
        "shell",
        "am start -a android.media.action.STILL_IMAGE_CAMERA",
    ]);
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        let output = adb(&["exec-out", "screencap", "-p"]);
        if output.status.success() {
            if let Some(path) = std::env::var_os("MIKO_CAMERA_DEBUG_SCREEN") {
                std::fs::write(path, &output.stdout).unwrap();
            }
            let mut decoder = png::Decoder::new(std::io::Cursor::new(&output.stdout));
            decoder
                .set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
            if let Ok(mut reader) = decoder.read_info() {
                let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
                if let Ok(frame) = reader.next_frame(&mut bytes) {
                    let channels = frame.color_type.samples();
                    let x = (frame.width / 2) as usize;
                    let y = (frame.height / 2) as usize;
                    let at = (y * frame.width as usize + x) * channels;
                    if channels >= 3 && bytes[at] > 180 && bytes[at + 1] < 70 && bytes[at + 2] < 70
                    {
                        return;
                    }
                }
            }
        }
        // The first-launch button moves with display size.
        let ui = adb(&[
            "shell",
            "uiautomator dump /sdcard/miko-camera-ui.xml >/dev/null && cat /sdcard/miko-camera-ui.xml",
        ]);
        let xml = String::from_utf8_lossy(&ui.stdout);
        if let Some(node) = xml
            .split("<node ")
            .find(|node| node.contains("com.android.camera2:id/confirm_button"))
            && let Some(bounds) = node
                .split("bounds=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
        {
            let numbers: Vec<u32> = bounds
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|n| n.parse().ok())
                .collect();
            if let [x1, y1, x2, y2] = numbers.as_slice() {
                adb(&[
                    "shell",
                    "input",
                    "tap",
                    &((x1 + x2) / 2).to_string(),
                    &((y1 + y2) / 2).to_string(),
                ]);
            }
        }
        if Instant::now() >= deadline {
            let log = adb(&[
                "logcat",
                "-d",
                "-s",
                "AndroidRuntime:E",
                "CameraService:E",
                "CameraActivity:E",
            ]);
            panic!(
                "camera preview did not show the red test image: {}",
                String::from_utf8_lossy(&log.stdout)
            );
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn file_camera_starts_and_persists() {
    let env = common::Env::boot("widgets-camera-file").await;
    let path = env.tmp.path().join("test image.png");
    {
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = png::Encoder::new(file, 640, 480);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255, 0, 0].repeat(640 * 480))
            .unwrap();
    }
    let set = env
        .call("camera.set", json!({"back":"image", "back_file":path}))
        .await;
    assert_eq!(set["back"], format!("imagefile:{}", path.display()));
    env.call("device.stop", json!({})).await;
    env.run("device.start", json!({"cold_boot":true})).await;
    let saved = env.call("camera.get", json!({})).await;
    assert_eq!(saved["back"], set["back"]);
    assert_camera_is_red(&env).await;
    env.call("device.stop", json!({})).await;
    env.run("device.start", json!({})).await;
    let saved = env.call("camera.get", json!({})).await;
    assert_eq!(saved["back"], set["back"]);
    assert_camera_is_red(&env).await;
    env.stop().await;
}

//! Typed device tools: screenshot, touch, keys, UI dump. They reach the
//! emulator directly (gRPC + adb) using `device.connect_info`, like the player.

use std::time::Duration;

use miko_adb::Adb;
use miko_core::{ConnectInfo, ErrorCode, MikoError, MikoResult};
use miko_proto::Controller;
use miko_proto::control::{
    ImageFormat, KeyboardEvent, image_format::ImgFormat, keyboard_event::KeyEventType,
};

/// Screenshots are fitted in a box of this size (plan: PNG ≤ 1024 px).
pub const SHOT_BOX: u32 = 1024;

pub struct DeviceIo {
    pub info: ConnectInfo,
    pub adb: Adb,
}

impl DeviceIo {
    pub fn new(info: ConnectInfo) -> Self {
        let adb = Adb::new(&info.adb_path)
            .device(&info.adb_serial)
            .with_timeout(Duration::from_secs(60));
        Self { info, adb }
    }

    async fn grpc(&self) -> MikoResult<Controller> {
        Ok(
            miko_backend::grpc::connect_port(self.info.grpc_port, &self.info.grpc_token)
                .await?
                .0,
        )
    }

    /// The current screen size in device pixels, rotation applied.
    pub async fn screen_size(&self) -> MikoResult<(u32, u32)> {
        let wm = self.adb.shell("wm size").await?;
        let (w, h) = parse_wm_size(&wm).ok_or_else(|| {
            MikoError::unexpected(format!("can't read the screen size from {wm:?}"))
        })?;
        let display = self.adb.shell("dumpsys display").await?;
        let rotated = display_orientation(&display)
            .ok_or_else(|| MikoError::unexpected("cannot read display orientation"))?
            % 2
            == 1;
        Ok(if rotated { (h, w) } else { (w, h) })
    }

    /// PNG fitted in `SHOT_BOX`; returns (png, width, height).
    pub async fn screenshot(&self) -> MikoResult<(Vec<u8>, u32, u32)> {
        let mut c = self.grpc().await?;
        // ADR-0001: ask for width *and* height, or the emulator ignores the size.
        let fmt = ImageFormat {
            format: ImgFormat::Png as i32,
            width: SHOT_BOX,
            height: SHOT_BOX,
            ..Default::default()
        };
        let img = c
            .get_screenshot(fmt)
            .await
            .map_err(|s| miko_backend::grpc::status_error("screenshot", s))?
            .into_inner();
        if img.image.is_empty() {
            return Err(MikoError::unexpected(
                "the display is off (empty screenshot); press_key Power to wake it",
            ));
        }
        let (w, h) = png_size(&img.image).unwrap_or((0, 0));
        Ok((img.image, w, h))
    }

    pub async fn tap(&self, x: u32, y: u32) -> MikoResult<()> {
        self.adb
            .shell(&format!("input tap {x} {y}"))
            .await
            .map(|_| ())
    }

    pub async fn swipe(
        &self,
        from: (u32, u32),
        to: (u32, u32),
        ms: u32,
        cancel: tokio::sync::watch::Receiver<bool>,
    ) -> MikoResult<()> {
        swipe_events(from, to, ms, cancel, |action, point| async move {
            self.adb
                .shell(&format!(
                    "input motionevent {action} {} {}",
                    point.0, point.1
                ))
                .await
                .map(|_| ())
        })
        .await
    }

    /// Types into the focused field with `input text`. (gRPC `sendKey` text
    /// was flaky here: in the agent tests it sometimes dismissed the Settings
    /// search screen it was typing into; `input text` never did.)
    pub async fn type_text(&self, text: &str) -> MikoResult<()> {
        if text.is_empty() {
            return Ok(());
        }
        validate_input_text(text)?;
        self.adb.shell(&input_text_command(text)).await.map(|_| ())
    }

    /// A key by its web name (`GoHome`, `GoBack`, `AppSwitch`, `Enter`, `Power`, …).
    pub async fn press_key(&self, key: &str) -> MikoResult<()> {
        let mut c = self.grpc().await?;
        let ev = KeyboardEvent {
            key: key.to_string(),
            event_type: KeyEventType::Keypress as i32,
            ..Default::default()
        };
        c.send_key(ev)
            .await
            .map_err(|s| miko_backend::grpc::status_error("press_key", s))?;
        Ok(())
    }

    /// The UI hierarchy as uiautomator XML.
    pub async fn ui_dump(&self) -> MikoResult<String> {
        let out = self
            .adb
            .shell("uiautomator dump /sdcard/miko_ui.xml >/dev/null && cat /sdcard/miko_ui.xml")
            .await?;
        let start = out.find("<?xml").or_else(|| out.find("<hierarchy"));
        match start {
            Some(i) => Ok(out[i..].trim().to_string()),
            None => Err(MikoError::new(
                ErrorCode::Unexpected,
                format!("uiautomator gave no hierarchy: {}", out.trim()),
            )),
        }
    }
}

fn display_orientation(output: &str) -> Option<u32> {
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("mCurrentOrientation=")?
            .parse::<u32>()
            .ok()
            .filter(|value| *value < 4)
    })
}

async fn swipe_events<F, Fut>(
    from: (u32, u32),
    to: (u32, u32),
    ms: u32,
    mut cancel: tokio::sync::watch::Receiver<bool>,
    mut send: F,
) -> MikoResult<()>
where
    F: FnMut(&'static str, (u32, u32)) -> Fut,
    Fut: std::future::Future<Output = MikoResult<()>>,
{
    if *cancel.borrow() {
        return Ok(());
    }
    let mut point = from;
    // A failed DOWN may still have reached Android, so always attempt UP.
    let mut result = send("DOWN", point).await;
    let started = tokio::time::Instant::now();
    while result.is_ok() && !*cancel.borrow() {
        if started.elapsed() >= Duration::from_millis(u64::from(ms)) {
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(16)) => {},
            changed = cancel.changed() => { if changed.is_err() || *cancel.borrow() { break; } }
        }
        if *cancel.borrow() {
            break;
        }
        let fraction = (started.elapsed().as_secs_f64() / (f64::from(ms) / 1000.0)).min(1.0);
        point = (
            (f64::from(from.0) + (f64::from(to.0) - f64::from(from.0)) * fraction).round() as u32,
            (f64::from(from.1) + (f64::from(to.1) - f64::from(from.1)) * fraction).round() as u32,
        );
        result = send("MOVE", point).await;
    }
    if result.is_ok() && !*cancel.borrow() && point != to {
        point = to;
        result = send("MOVE", point).await;
    }
    let up = send("UP", point).await;
    result.and(up)
}

pub fn validate_input_text(text: &str) -> MikoResult<()> {
    if text.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
        Ok(())
    } else {
        Err(MikoError::usage(
            "type_text supports printable ASCII; paste other text with command_run clipboard commands",
        ))
    }
}

/// `input text '<text>'` for the device shell: spaces become `%s` (how
/// `input text` spells them) and the argument is single-quoted.
pub fn input_text_command(text: &str) -> String {
    let arg = text.replace(' ', "%s").replace('\'', "'\\''");
    format!("input text '{arg}'")
}

/// `Physical size: 1080x2400` / `Override size: 720x1600` (override wins).
pub fn parse_wm_size(text: &str) -> Option<(u32, u32)> {
    let pick = |label: &str| {
        text.lines().find_map(|l| {
            let rest = l.trim().strip_prefix(label)?.trim();
            let (w, h) = rest.split_once('x')?;
            Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
        })
    };
    pick("Override size:").or_else(|| pick("Physical size:"))
}

/// Width and height from a PNG's IHDR chunk.
pub fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.get(..8)? != b"\x89PNG\r\n\x1a\n" || png.get(12..16)? != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(png.get(16..20)?.try_into().ok()?);
    let h = u32::from_be_bytes(png.get(20..24)?.try_into().ok()?);
    Some((w, h))
}

/// Scale from screenshot space to device pixels for a screen of `w`×`h`
/// fitted in `SHOT_BOX`.
pub fn shot_scale(w: u32, h: u32) -> f64 {
    let fit = (f64::from(SHOT_BOX) / f64::from(w.max(1)))
        .min(f64::from(SHOT_BOX) / f64::from(h.max(1)))
        .min(1.0);
    1.0 / fit
}

/// Maps a point given in screenshot space to device pixels, clamped.
pub fn to_device(p: (f64, f64), screen: (u32, u32)) -> (u32, u32) {
    let s = shot_scale(screen.0, screen.1);
    let clamp =
        |v: f64, max: u32| (v * s).round().clamp(0.0, f64::from(max.saturating_sub(1))) as u32;
    (clamp(p.0, screen.0), clamp(p.1, screen.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_logical_display_orientation() {
        assert_eq!(display_orientation("  mCurrentOrientation=1\n"), Some(1));
        assert_eq!(display_orientation("mCurrentOrientation=4"), None);
    }

    #[tokio::test]
    async fn cancelled_swipe_releases_before_returning_and_reports_failed_up() {
        for fail_up in [false, true] {
            let (cancel, receiver) = tokio::sync::watch::channel(false);
            let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let captured = events.clone();
            let result = swipe_events(
                (10, 20),
                (100, 200),
                1000,
                receiver,
                move |action, point| {
                    captured.lock().unwrap().push((action, point));
                    if action == "DOWN" {
                        cancel.send(true).unwrap();
                    }
                    async move {
                        if action == "UP" && fail_up {
                            Err(MikoError::unexpected("up failed"))
                        } else {
                            Ok(())
                        }
                    }
                },
            )
            .await;
            assert_eq!(result.is_err(), fail_up);
            assert_eq!(
                *events.lock().unwrap(),
                vec![("DOWN", (10, 20)), ("UP", (10, 20))]
            );
        }
    }

    #[test]
    fn input_text_is_shell_safe() {
        assert_eq!(input_text_command("battery"), "input text 'battery'");
        assert_eq!(input_text_command("hi there"), "input text 'hi%sthere'");
        assert_eq!(
            input_text_command("it's; rm -rf / & $(x)"),
            "input text 'it'\\''s;%srm%s-rf%s/%s&%s$(x)'"
        );
    }

    #[test]
    fn wm_size() {
        assert_eq!(
            parse_wm_size("Physical size: 1080x2400\n"),
            Some((1080, 2400))
        );
        assert_eq!(
            parse_wm_size("Physical size: 1080x2400\nOverride size: 720x1600"),
            Some((720, 1600))
        );
        assert_eq!(parse_wm_size("nope"), None);
    }

    #[test]
    fn png_header() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(461u32.to_be_bytes());
        png.extend(1024u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((461, 1024)));
        assert_eq!(png_size(b"GIF89a"), None);
    }

    #[test]
    fn screenshot_space_maps_to_device_pixels() {
        // 1080×2400 fits 1024 tall: 460.8 wide, scale 2.34375.
        assert!((shot_scale(1080, 2400) - 2.34375).abs() < 1e-9);
        assert_eq!(to_device((230.0, 512.0), (1080, 2400)), (539, 1200));
        assert_eq!(
            to_device((5000.0, -3.0), (1080, 2400)),
            (1079, 0),
            "clamped"
        );
        // Small screens aren't upscaled.
        assert_eq!(shot_scale(720, 1000), 1.0);
    }

    #[tokio::test]
    async fn completed_swipe_reaches_requested_endpoint() {
        let (_sender, receiver) = tokio::sync::watch::channel(false);
        let events = std::sync::Mutex::new(Vec::new());
        swipe_events((10, 20), (80, 90), 1, receiver, |action, point| {
            events.lock().unwrap().push((action, point));
            async { Ok(()) }
        })
        .await
        .unwrap();
        let events = events.into_inner().unwrap();
        assert_eq!(events.last(), Some(&("UP", (80, 90))));
        assert!(events.contains(&("MOVE", (80, 90))));
    }
}

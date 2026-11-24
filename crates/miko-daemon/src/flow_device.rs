//! Device I/O for workflow jobs. All injected downs are tracked for release.
use miko_adb::Adb;
use miko_automation::{
    Event, FlowAction, Image, Locator, Point, ScriptStore, find_template_cancellable, find_ui,
};
use miko_core::{Cancel, ConnectInfo, MikoError, MikoResult};
use miko_proto::{Controller, control as pb};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::time::Instant;

#[derive(Clone)]
enum Held {
    Touch(i32, i32, i32),
    Key(i32, i32, String),
}
struct MatchCancellation(Arc<AtomicBool>);
impl Drop for MatchCancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}
fn fatal_input(mut error: MikoError) -> MikoError {
    let mut data = error.data.take().unwrap_or_else(|| serde_json::json!({}));
    if let Some(map) = data.as_object_mut() {
        map.insert("flow_fatal".into(), serde_json::json!(true));
    }
    error.data = Some(data);
    error
}

fn finish_action(
    result: MikoResult<()>,
    release: MikoResult<()>,
    attempted: bool,
) -> MikoResult<()> {
    let result = if attempted {
        result.map_err(fatal_input)
    } else {
        result
    };
    release.map_err(fatal_input).and(result)
}
pub struct FlowDevice {
    adb: Adb,
    controller: Controller,
    data: PathBuf,
    held: Vec<Held>,
    panel: Option<(u32, u32, u32)>,
    input_attempted: bool,
}
impl FlowDevice {
    pub async fn new(info: ConnectInfo, data: PathBuf) -> MikoResult<Self> {
        let controller = miko_backend::grpc::connect_port(info.grpc_port, &info.grpc_token)
            .await?
            .0;
        let adb = Adb::new(&info.adb_path)
            .device(&info.adb_serial)
            .with_timeout(Duration::from_secs(60));
        Ok(Self {
            adb,
            controller,
            data,
            held: vec![],
            panel: None,
            input_attempted: false,
        })
    }
    async fn dimensions(&self) -> MikoResult<(u32, u32, u32)> {
        let output = self.adb.shell("wm size").await?;
        let parse = |label: &str| {
            output.lines().find_map(|line| {
                let (w, h) = line.trim().strip_prefix(label)?.trim().split_once('x')?;
                Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
            })
        };
        let (w, h) = parse("Override size:")
            .or_else(|| parse("Physical size:"))
            .ok_or_else(|| MikoError::unexpected("cannot read display size"))?;
        let orientation = self.adb.shell("dumpsys display").await?;
        let rotation = display_orientation(&orientation)
            .ok_or_else(|| MikoError::unexpected("cannot read display orientation"))?;
        Ok((w, h, rotation))
    }
    fn position(p: Point, (w, h, r): (u32, u32, u32)) -> MikoResult<(i32, i32)> {
        if w == 0 || h == 0 {
            return Err(MikoError::unexpected("empty display"));
        }
        let (sw, sh) = if r % 2 == 0 { (w, h) } else { (h, w) };
        let x = (p.x * f64::from(sw)).min(f64::from(sw - 1)) as i32;
        let y = (p.y * f64::from(sh)).min(f64::from(sh - 1)) as i32;
        let (right, bottom) = (w as i32 - 1, h as i32 - 1);
        Ok(match r {
            1 => (right - y, x),
            2 => (right - x, bottom - y),
            3 => (y, bottom - x),
            _ => (x, y),
        })
    }
    async fn touch_at(&mut self, id: i32, x: i32, y: i32, down: bool) -> MikoResult<()> {
        if down {
            if let Some(held) = self
                .held
                .iter_mut()
                .find(|h| matches!(h,Held::Touch(i,_,_) if *i == id))
            {
                *held = Held::Touch(id, x, y);
            } else {
                self.held.push(Held::Touch(id, x, y));
            }
        }
        let event = pb::TouchEvent {
            touches: vec![pb::Touch {
                x,
                y,
                identifier: id,
                pressure: if down { 100 } else { 0 },
                touch_major: if down { 5 } else { 0 },
                expiration: pb::touch::EventExpiration::NeverExpire as i32,
                ..Default::default()
            }],
            display: 0,
        };
        self.input_attempted = true;
        self.controller
            .send_touch(event)
            .await
            .map_err(|e| fatal_input(miko_backend::grpc::status_error("flow touch", e)))?;
        if !down {
            self.held
                .retain(|h| !matches!(h,Held::Touch(i,_,_) if *i == id));
        }
        Ok(())
    }
    async fn touch(&mut self, id: i32, p: Point, down: bool) -> MikoResult<()> {
        if down && !self.held.iter().any(|h| matches!(h, Held::Touch(..))) {
            self.panel = Some(self.dimensions().await?);
        }
        let held = if !down {
            self.held.iter().find_map(|h| {
                if let Held::Touch(i, x, y) = h {
                    if *i == id { Some((*x, *y)) } else { None }
                } else {
                    None
                }
            })
        } else {
            None
        };
        let (x, y) = if let Some(point) = held {
            point
        } else {
            let panel = match self.panel {
                Some(panel) => panel,
                None => self.dimensions().await?,
            };
            Self::position(p, panel)?
        };
        self.touch_at(id, x, y, down).await
    }
    async fn key(&mut self, code_type: i32, code: i32, key: String, down: bool) -> MikoResult<()> {
        if down
            && !self
                .held
                .iter()
                .any(|h| matches!(h,Held::Key(t,c,k) if *t == code_type && *c == code && *k == key))
        {
            self.held.push(Held::Key(code_type, code, key.clone()));
        }
        let event = pb::KeyboardEvent {
            code_type,
            key_code: code,
            key: key.clone(),
            event_type: if down {
                pb::keyboard_event::KeyEventType::Keydown as i32
            } else {
                pb::keyboard_event::KeyEventType::Keyup as i32
            },
            ..Default::default()
        };
        self.input_attempted = true;
        self.controller
            .send_key(event)
            .await
            .map_err(|e| fatal_input(miko_backend::grpc::status_error("flow key", e)))?;
        if !down {
            self.held.retain(
                |h| !matches!(h,Held::Key(t,c,k) if *t == code_type && *c == code && *k == key),
            );
        }
        Ok(())
    }
    pub async fn release(&mut self) -> MikoResult<()> {
        let held = self.held.clone();
        let mut failure = None;
        for item in held.into_iter().rev() {
            let result = tokio::time::timeout(Duration::from_secs(1), async {
                match item {
                    Held::Touch(id, x, y) => self.touch_at(id, x, y, false).await,
                    Held::Key(t, c, k) => self.key(t, c, k, false).await,
                }
            })
            .await
            .map_err(|_| MikoError::unexpected("input release timed out"))
            .and_then(|result| result);
            if let Err(error) = result
                && failure.is_none()
            {
                failure = Some(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
    async fn text(&mut self, text: &str) -> MikoResult<()> {
        self.input_attempted = true;
        self.controller
            .set_clipboard(pb::ClipData {
                text: text.to_owned(),
            })
            .await
            .map_err(|e| fatal_input(miko_backend::grpc::status_error("flow clipboard", e)))?;
        let evdev = pb::keyboard_event::KeyCodeType::Evdev as i32;
        for (code, down) in [(29, true), (47, true), (47, false), (29, false)] {
            self.key(evdev, code, String::new(), down).await?;
        }
        Ok(())
    }
    async fn keypress(&mut self, key: &str) -> MikoResult<()> {
        let code = match key {
            "mute" => Some(113),
            "volume_down" => Some(114),
            "volume_up" => Some(115),
            "power" => Some(116),
            "menu" => Some(139),
            "back" => Some(158),
            "recents" | "app_switch" => Some(580),
            "play_pause" => Some(164),
            "next" => Some(163),
            "previous" => Some(165),
            _ => None,
        };
        let (kind, code, name) = if let Some(code) = code {
            (
                pb::keyboard_event::KeyCodeType::Evdev as i32,
                code,
                String::new(),
            )
        } else {
            let key = match key {
                "home" => "GoHome",
                "enter" | "dpad_center" => "Enter",
                "delete" => "Backspace",
                "dpad_up" => "ArrowUp",
                "dpad_down" => "ArrowDown",
                "dpad_left" => "ArrowLeft",
                "dpad_right" => "ArrowRight",
                other => other,
            };
            (
                pb::keyboard_event::KeyCodeType::Usb as i32,
                0,
                key.to_owned(),
            )
        };
        self.key(kind, code, name.clone(), true).await?;
        self.key(kind, code, name, false).await
    }
    async fn sleep(ms: u64, cancel: &Cancel) -> MikoResult<()> {
        let until = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < until {
            cancel.check()?;
            tokio::time::sleep_until(until.min(Instant::now() + Duration::from_millis(20))).await;
        }
        cancel.check()
    }
    async fn script(&mut self, id: uuid::Uuid, cancel: &Cancel) -> MikoResult<()> {
        let script = ScriptStore::new(&self.data).resolve_playback(&id)?;
        let mut previous = 0;
        for item in script.events {
            wait_script_interval(&mut previous, item.t, cancel).await?;
            let usb = pb::keyboard_event::KeyCodeType::Usb as i32;
            match item.event {
                Event::Touch { x, y, down } => self.touch(1, Point { x, y }, down).await?,
                Event::MappedTouch { id, x, y, down } => {
                    self.touch(id, Point { x, y }, down).await?
                }
                Event::Key { key, down } => self.key(usb, 0, key, down).await?,
                Event::Text { text } | Event::Paste { text } => self.text(&text).await?,
                Event::ToolKey { key } => self.keypress(&key).await?,
                Event::ScriptCall { .. } => {
                    return Err(MikoError::unexpected("unresolved script call"));
                }
            }
        }
        Ok(())
    }
    pub async fn action(&mut self, action: &FlowAction, cancel: &Cancel) -> MikoResult<()> {
        self.input_attempted = false;
        let result = {
            let task = self.action_inner(action, cancel);
            tokio::pin!(task);
            loop {
                tokio::select! {
                    result = &mut task => break result,
                    () = tokio::time::sleep(Duration::from_millis(20)) => {
                        if cancel.is_cancelled() { break Err(miko_core::cancel::cancelled()); }
                    }
                }
            }
        };
        let attempted = self.input_attempted;
        let release = self.release().await;
        finish_action(result, release, attempted)
    }

    pub async fn reconnect(&mut self, info: &ConnectInfo) -> MikoResult<()> {
        let controller = miko_backend::grpc::connect_port(info.grpc_port, &info.grpc_token)
            .await?
            .0;
        self.controller = controller;
        self.adb = Adb::new(&info.adb_path)
            .device(&info.adb_serial)
            .with_timeout(Duration::from_secs(60));
        self.panel = None;
        Ok(())
    }
    async fn action_inner(&mut self, action: &FlowAction, cancel: &Cancel) -> MikoResult<()> {
        cancel.check()?;
        match action {
            FlowAction::Tap { point } => {
                self.touch(1, *point, true).await?;
                self.touch(1, *point, false).await
            }
            FlowAction::Swipe {
                from,
                to,
                duration_ms,
            } => {
                self.touch(1, *from, true).await?;
                let start = Instant::now();
                loop {
                    cancel.check()?;
                    let fraction =
                        (start.elapsed().as_secs_f64() / (*duration_ms as f64 / 1000.0)).min(1.0);
                    let p = Point {
                        x: from.x + (to.x - from.x) * fraction,
                        y: from.y + (to.y - from.y) * fraction,
                    };
                    self.touch(1, p, true).await?;
                    if fraction >= 1.0 {
                        break;
                    }
                    Self::sleep(16, cancel).await?;
                }
                self.touch(1, *to, false).await
            }
            FlowAction::Text { text } => self.text(text).await,
            FlowAction::Key { key } => self.keypress(key).await,
            FlowAction::Script { id } => self.script(*id, cancel).await,
            FlowAction::TapElement { .. } => {
                Err(MikoError::usage("element must be resolved before tapping"))
            }
        }
    }
    async fn screenshot(&mut self, format: pb::image_format::ImgFormat) -> MikoResult<pb::Image> {
        let img = self
            .controller
            .get_screenshot(pb::ImageFormat {
                format: format as i32,
                ..Default::default()
            })
            .await
            .map_err(|e| miko_backend::grpc::status_error("flow screenshot", e))?
            .into_inner();
        if img.image.is_empty() {
            return Err(MikoError::unexpected("empty screenshot"));
        }
        Ok(img)
    }
    pub async fn screenshot_png(&mut self) -> MikoResult<Vec<u8>> {
        Ok(self
            .screenshot(pb::image_format::ImgFormat::Png)
            .await?
            .image)
    }
    pub async fn find(&mut self, locator: &Locator) -> MikoResult<Option<Point>> {
        match locator {
            Locator::Ui { text, resource_id } => {
                let xml=self.adb.shell("uiautomator dump /sdcard/miko_ui.xml >/dev/null && cat /sdcard/miko_ui.xml").await?;
                let start = xml
                    .find("<?xml")
                    .or_else(|| xml.find("<hierarchy"))
                    .ok_or_else(|| MikoError::unexpected("UI dump returned no hierarchy"))?;
                let (w, h, r) = self.dimensions().await?;
                let (w, h) = if r % 2 == 0 { (w, h) } else { (h, w) };
                find_ui(&xml[start..], text.as_deref(), resource_id.as_deref(), w, h)
            }
            Locator::Template { path, threshold } => {
                let img = self
                    .screenshot(pb::image_format::ImgFormat::Rgba8888)
                    .await?;
                let format = img
                    .format
                    .ok_or_else(|| MikoError::unexpected("screenshot has no format"))?;
                let screen = Image {
                    width: format.width,
                    height: format.height,
                    pixels: img.image,
                };
                let data = tokio::fs::read(path)
                    .await
                    .map_err(|e| MikoError::usage(format!("reading template: {e}")))?;
                let template = decode_png(&data)?;
                let threshold = *threshold;
                let cancelled = Arc::new(AtomicBool::new(false));
                let _guard = MatchCancellation(cancelled.clone());
                tokio::task::spawn_blocking(move || {
                    find_template_cancellable(&screen, &template, threshold, || {
                        cancelled.load(Ordering::Relaxed)
                    })
                })
                .await
                .map_err(|e| MikoError::unexpected(format!("template matching: {e}")))?
            }
        }
    }
}

async fn wait_script_interval(
    previous: &mut u64,
    timestamp: u64,
    cancel: &Cancel,
) -> MikoResult<()> {
    let interval = timestamp.saturating_sub(*previous);
    *previous = timestamp;
    FlowDevice::sleep(interval, cancel).await
}

pub(crate) fn display_orientation(output: &str) -> Option<u32> {
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("mCurrentOrientation=")?
            .parse::<u32>()
            .ok()
            .filter(|rotation| *rotation < 4)
    })
}
fn decode_png(bytes: &[u8]) -> MikoResult<Image> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| MikoError::usage(format!("invalid template PNG: {e}")))?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| MikoError::usage("template PNG is too large"))?;
    let mut buf = vec![0; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| MikoError::usage(format!("invalid template PNG: {e}")))?;
    let bytes = &buf[..info.buffer_size()];
    let pixels = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => bytes
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => bytes.iter().flat_map(|v| [*v, *v, *v, 255]).collect(),
        png::ColorType::GrayscaleAlpha => bytes
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        _ => return Err(MikoError::usage("unsupported template PNG")),
    };
    Ok(Image {
        width: info.width,
        height: info.height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn script_interval_is_not_shortened_by_slow_input() {
        let mut previous = 0;
        let cancel = Cancel::new();
        wait_script_interval(&mut previous, 0, &cancel)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await; // first input was slow
        let delivered = Instant::now();
        wait_script_interval(&mut previous, 80, &cancel)
            .await
            .unwrap();
        assert!(delivered.elapsed() >= Duration::from_millis(75));
    }

    #[test]
    fn reads_logical_display_orientation() {
        assert_eq!(
            display_orientation("Display:\n  mCurrentOrientation=1\n"),
            Some(1)
        );
        assert_eq!(display_orientation("mCurrentOrientation=4"), None);
        assert_eq!(display_orientation("unrelated=0"), None);
    }

    #[test]
    fn action_errors_remain_retryable_but_input_and_release_errors_stop() {
        let script_error =
            finish_action(Err(MikoError::not_found("script missing")), Ok(()), false).unwrap_err();
        assert_ne!(
            script_error.data.as_ref().and_then(|d| d.get("flow_fatal")),
            Some(&serde_json::json!(true))
        );
        let injection = finish_action(
            Err(fatal_input(MikoError::unexpected("touch failed"))),
            Ok(()),
            true,
        )
        .unwrap_err();
        assert_eq!(
            injection.data.as_ref().and_then(|d| d.get("flow_fatal")),
            Some(&serde_json::json!(true))
        );
        let release = finish_action(
            Err(MikoError::not_found("script missing")),
            Err(MikoError::unexpected("release failed")),
            false,
        )
        .unwrap_err();
        assert_eq!(release.message, "release failed");
        assert_eq!(
            release.data.as_ref().and_then(|d| d.get("flow_fatal")),
            Some(&serde_json::json!(true))
        );
        let partial =
            finish_action(Err(MikoError::sdk("device offline")), Ok(()), true).unwrap_err();
        assert_eq!(
            partial.data.as_ref().and_then(|d| d.get("flow_fatal")),
            Some(&serde_json::json!(true))
        );
    }
}

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use miko_adb::Adb;
use miko_widgets::gamepad::{confirm_removal, hid, uinput, wait_for_registration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use uuid::Uuid;

pub struct NativeGamepad {
    adb: Adb,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    uinput_mode: bool,
    name: String,
    pub session: Uuid,
    pub sequence: u64,
    pub last: Instant,
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn reports_are_ordered_and_stop_removes_the_virtual_device() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("adb");
        std::fs::write(
            &program,
            r##"#!/usr/bin/python3
import json, pathlib, sys
p = pathlib.Path(__file__).parent
marker = p / 'name'
if 'hid' in sys.argv:
    for line in sys.stdin:
        event = json.loads(line)
        if event['command'] == 'register': marker.write_text(event['name'])
        if event['command'] == 'report':
            with (p / 'reports').open('a') as f: f.write(json.dumps(event['report']) + '\n')
    marker.unlink(missing_ok=True)
elif marker.exists():
    print('Name: ' + marker.read_text())
else:
    print('No virtual input device')
"##,
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let info = miko_core::ConnectInfo {
            device: Uuid::new_v4(),
            grpc_endpoint: String::new(),
            grpc_port: 0,
            grpc_token: String::new(),
            adb_serial: "test".into(),
            adb_path: program,
            console_port: 0,
        };
        let mut pad = NativeGamepad::start(&info, Uuid::new_v4()).unwrap();
        pad.register().await.unwrap();
        let mut down = hid::Pad::default();
        down.apply(hid::PadEvent::Button(hid::Button::South, true));
        pad.report(5, &down.report()).await.unwrap();
        assert!(pad.report(7, &down.report()).await.is_err());
        pad.stop().await.unwrap();
        assert!(!dir.path().join("name").exists());
        let reports = std::fs::read_to_string(dir.path().join("reports")).unwrap();
        let lines: Vec<Vec<u8>> = reports
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.first().unwrap()[0], 1);
        assert_eq!(
            lines.last().unwrap().as_slice(),
            &hid::Pad::default().report()
        );
    }
}

impl NativeGamepad {
    pub fn start(info: &miko_core::ConnectInfo, session: Uuid) -> Result<Self> {
        let adb = Adb::new(info.adb_path.clone()).device(info.adb_serial.clone());
        let name = format!("miko-{}", Uuid::new_v4().simple());
        Ok(Self {
            adb,
            child: None,
            stdin: None,
            uinput_mode: false,
            name,
            session,
            sequence: 0,
            last: Instant::now(),
        })
    }

    pub async fn register(&mut self) -> Result<()> {
        let (status, output) = self.adb.shell_status("if command -v uinput >/dev/null && test -w /dev/uinput; then printf miko_uinput; fi").await?;
        self.uinput_mode = status == 0 && output.trim() == "miko_uinput";
        match self.register_current().await {
            Ok(()) => Ok(()),
            Err(error) if self.uinput_mode => {
                // Reuse the input lease only after Android confirms removal.
                self.stop()
                    .await
                    .context("uinput cleanup was not confirmed")?;
                self.child = None;
                self.uinput_mode = false;
                self.register_current()
                    .await
                    .context(format!("hid fallback after uinput failed: {error:#}"))
            }
            Err(error) => Err(error),
        }
    }

    async fn register_current(&mut self) -> Result<()> {
        let mut child = if self.uinput_mode {
            self.adb.spawn_with_stdio(&["shell", "uinput", "-"])?
        } else {
            self.adb.spawn_with_stdin(&["shell", "hid", "-"])?
        };
        self.stdin = child.stdin.take();
        if let Some(mut stdout) = child.stdout.take() {
            let session = self.session;
            tokio::spawn(async move {
                let mut decoder = uinput::FeedbackDecoder::default();
                let mut buffer = [0u8; 4096];
                while let Ok(count) = stdout.read(&mut buffer).await {
                    if count == 0 {
                        break;
                    }
                    match decoder.push(&buffer[..count]) {
                        Ok(events) => {
                            for vibrating in events {
                                eprintln!(
                                    "MIKO_GAMEPAD_FEEDBACK:{}",
                                    serde_json::json!({"session":session,"vibrating":vibrating})
                                );
                            }
                        }
                        Err(_) => break,
                    }
                }
                eprintln!(
                    "MIKO_GAMEPAD_FEEDBACK:{}",
                    serde_json::json!({"session":session,"vibrating":false})
                );
            });
        }
        self.child = Some(child);
        let register = if self.uinput_mode {
            uinput::register(&self.name)
        } else {
            hid::register_line(&self.name)
        };
        let mut line = serde_json::to_vec(&register)?;
        line.push(b'\n');
        tokio::time::timeout(
            Duration::from_secs(1),
            self.stdin
                .as_mut()
                .context("virtual controller input closed")?
                .write_all(&line),
        )
        .await??;
        wait_for_registration(
            &self.adb,
            self.child
                .as_mut()
                .context("virtual controller process unavailable")?,
            &self.name,
            &AtomicBool::new(false),
            &miko_core::Cancel::new(),
        )
        .await?;
        Ok(())
    }

    pub async fn report(&mut self, sequence: u64, report: &[u8; hid::REPORT_LEN]) -> Result<()> {
        if self.sequence != 0 && sequence != self.sequence + 1 {
            bail!("controller stream lost input");
        }
        let frame = if self.uinput_mode {
            uinput::report(report)
        } else {
            serde_json::json!({"id":1,"command":"report","report":report})
        };
        let mut line = serde_json::to_vec(&frame)?;
        line.push(b'\n');
        tokio::time::timeout(
            Duration::from_secs(1),
            self.stdin
                .as_mut()
                .context("virtual controller input closed")?
                .write_all(&line),
        )
        .await??;
        self.sequence = sequence;
        self.last = Instant::now();
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        let neutral = hid::Pad::default().report();
        let frame = if self.uinput_mode {
            uinput::report(&neutral)
        } else {
            hid::report_line(&hid::Pad::default())
        };
        let mut line = serde_json::to_vec(&frame)?;
        line.push(b'\n');
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = tokio::time::timeout(Duration::from_millis(100), stdin.write_all(&line)).await;
        }
        self.stdin.take();
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        let status = tokio::time::timeout(Duration::from_secs(2), child.wait()).await??;
        if !status.success() {
            bail!("virtual controller did not exit cleanly");
        }
        confirm_removal(&self.adb, &self.name, Duration::from_secs(5)).await?;
        Ok(())
    }
}

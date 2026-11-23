//! Operation-recording manager for the player bridge.
//!
//! The bridge records normalised player input (touch, mapped touch, keys, text,
//! paste and tool keys) with timestamps and replays it locally. Format, limits
//! and storage live in `miko_automation`; this module owns the bridge state
//! machine (record/pause/stop + the non-blocking playback tick), the stderr
//! `MIKO_SCRIPT:` notifications and the playback injection into the emulator.
//!
//! The playback engine is advanced by the single input owner: the FIFO input
//! worker (or the inline fallback) arms a ~5 ms timer only while a script is
//! playing, so an idle bridge has no periodic input timer and the frame stream
//! stays responsive (no blocking waits between events).

use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::time::{Duration, Instant};

use anyhow::Result;
use miko_automation::{
    DisplayInfo, Event, PlaybackMode, PlaybackOptions, Recorder, Script, ScriptStore,
};
use miko_client::Client;
use miko_core::{ErrorCode, MikoError};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::Command;
use crate::player_input::{GrpcInput, InputState};

/// Every script notification is one line on stderr with this prefix.
const PREFIX: &str = "MIKO_SCRIPT:";
/// Stable codes the Swift player can translate; the bridge sends English text.
const BUSY_CODE: &str = "script.busy";
const INVALID_CODE: &str = "script.invalid";
const NOT_FOUND_CODE: &str = "script.not_found";
const FAILED_CODE: &str = "script.failed";
const NOT_RECORDING_CODE: &str = "script.not_recording";
const NOT_PLAYING_CODE: &str = "script.not_playing";

fn notify(value: Value) {
    eprintln!("{PREFIX}{value}");
}

fn notify_error(code: &str, message: impl Into<String>) {
    notify(json!({"op":"error","code":code,"message":message.into()}));
}

/// The bridge's stable error code for a store/format failure.
fn error_code(error: &MikoError) -> &'static str {
    match error.code {
        ErrorCode::NotFound => NOT_FOUND_CODE,
        ErrorCode::Usage => INVALID_CODE,
        ErrorCode::Busy => BUSY_CODE,
        _ => FAILED_CODE,
    }
}

fn notify_store_error(error: &MikoError) {
    notify_error(error_code(error), error.message.clone());
}

/// Result of one script state-machine step. `events` are due playback inputs
/// for the caller to inject; `release` asks the caller to release held inputs.
#[derive(Default)]
pub struct ScriptOutcome {
    pub events: Vec<Event>,
    pub release: bool,
}

impl ScriptOutcome {
    fn release() -> Self {
        Self {
            events: Vec::new(),
            release: true,
        }
    }
}

/// One recorded command, if it is part of the v1 recording whitelist.
pub(crate) fn event_from_command(command: &Command) -> Option<Event> {
    match command {
        Command::Touch { x, y, down } => Some(Event::Touch {
            x: *x,
            y: *y,
            down: *down,
        }),
        Command::MappedTouch { id, x, y, down } => Some(Event::MappedTouch {
            id: *id,
            x: *x,
            y: *y,
            down: *down,
        }),
        Command::Key { key, down } => Some(Event::Key {
            key: key.clone(),
            down: *down,
        }),
        Command::Text { text } => Some(Event::Text { text: text.clone() }),
        Command::Paste { text } => Some(Event::Paste { text: text.clone() }),
        Command::ToolKey { key } => Some(Event::ToolKey { key: key.clone() }),
        _ => None,
    }
}

/// Cheap pre-check for the recorder: the same bounds the v1 validator applies,
/// so one invalid UI command cannot poison an otherwise valid recording.
fn event_fits_the_format(event: &Event) -> bool {
    match event {
        Event::Touch { x, y, .. } => {
            x.is_finite() && y.is_finite() && (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y)
        }
        Event::MappedTouch { id, x, y, .. } => {
            (miko_automation::MAPPED_ID_MIN..=miko_automation::MAPPED_ID_MAX).contains(id)
                && x.is_finite()
                && y.is_finite()
                && (0.0..=1.0).contains(x)
                && (0.0..=1.0).contains(y)
        }
        Event::Key { key, .. } | Event::ToolKey { key } => {
            !key.is_empty() && key.len() <= miko_automation::MAX_KEY_BYTES
        }
        Event::Text { text } | Event::Paste { text } => {
            text.len() <= miko_automation::MAX_TEXT_BYTES
        }
        Event::ScriptCall { .. } => false,
    }
}

/// Whether the command belongs to the operation-recording control surface.
/// Those are handled before any device input so recording never records itself.
pub fn is_script_command(command: &Command) -> bool {
    matches!(
        command,
        Command::ScriptRecordStart
            | Command::ScriptRecordPause
            | Command::ScriptRecordResume
            | Command::ScriptRecordStop { .. }
            | Command::ScriptRecordCancel
            | Command::ScriptList
            | Command::ScriptDelete { .. }
            | Command::ScriptRename { .. }
            | Command::ScriptActionsGet { .. }
            | Command::ScriptActionDelete { .. }
            | Command::ScriptActionWait { .. }
            | Command::ScriptActionMove { .. }
            | Command::ScriptActionGesture { .. }
            | Command::ScriptActionCall { .. }
            | Command::ScriptImport { .. }
            | Command::ScriptExport { .. }
            | Command::ScriptPlayStart { .. }
            | Command::ScriptPlayPause
            | Command::ScriptPlayResume
            | Command::ScriptPlayStop
    )
}

/// A new physical action can take ownership; a stale release cannot.
pub(crate) fn is_manual_takeover(command: &Command) -> bool {
    matches!(
        command,
        Command::Touch { down: true, .. }
            | Command::MappedTouch { down: true, .. }
            | Command::Key { down: true, .. }
            | Command::Text { .. }
            | Command::Paste { .. }
            | Command::Copy { .. }
            | Command::Cut { .. }
            | Command::SelectAll
            | Command::ToolKey { .. }
            | Command::Rotate { .. }
    )
}

/// Commands excluded from a locked synchronization follower.
pub(crate) fn is_direct_input(command: &Command) -> bool {
    matches!(
        command,
        Command::Touch { .. }
            | Command::MappedTouch { .. }
            | Command::Key { .. }
            | Command::Text { .. }
            | Command::Paste { .. }
            | Command::Copy { .. }
            | Command::Cut { .. }
            | Command::SelectAll
    )
}

/// Record/playback state owned by the bridge main loop.
pub struct ScriptManager {
    store: ScriptStore,
    device: String,
    display: DisplayInfo,
    recorder: Option<Recorder>,
    pending_record_count: Option<(Instant, usize)>,
    playback: Option<Playback>,
    editor: Option<(Uuid, Uuid, miko_automation::ActionDocument)>,
}

impl ScriptManager {
    pub fn new(data_dir: PathBuf, device: String, display_width: u32, display_height: u32) -> Self {
        Self {
            store: ScriptStore::new(data_dir),
            device,
            display: DisplayInfo {
                width: display_width,
                height: display_height,
            },
            recorder: None,
            pending_record_count: None,
            playback: None,
            editor: None,
        }
    }

    /// True while a script is playing (paused playback counts as active).
    pub fn is_playing(&self) -> bool {
        self.playback.is_some()
    }

    /// True while an operation recording is active (paused counts as active).
    pub fn is_recording(&self) -> bool {
        self.recorder.is_some()
    }

    /// Stop playback only for a new manual action, not an old release.
    pub fn take_manual_control(&mut self, command: &Command) -> bool {
        if self.playback.is_some() && is_manual_takeover(command) {
            self.stop_playback();
            true
        } else {
            false
        }
    }

    /// Records a whitelisted user command while a recording is running.
    pub fn record_user_command(&mut self, command: &Command, now: Instant) {
        let Some(event) = event_from_command(command) else {
            return;
        };
        self.record_event(event, now);
    }

    /// Records a toolbar tool key (handled outside the normal input dispatch).
    pub fn record_tool_key(&mut self, key: &str, now: Instant) {
        self.record_event(
            Event::ToolKey {
                key: key.to_owned(),
            },
            now,
        );
    }

    /// A failed device injection makes an in-progress recording unreliable.
    pub fn fail_recording(&mut self, message: &str) {
        if self.recorder.take().is_some() {
            self.pending_record_count = None;
            notify(json!({"op":"record", "state":"failed", "code":FAILED_CODE, "message":message}));
        }
    }

    fn record_event(&mut self, event: Event, now: Instant) {
        let Some(recorder) = self.recorder.as_mut() else {
            return;
        };
        if recorder.is_paused() || !event_fits_the_format(&event) {
            return;
        }
        if let Err(error) = recorder.record(event, now) {
            self.recorder = None;
            self.pending_record_count = None;
            notify(json!({
                "op":"record", "state":"failed",
                "code": error_code(&error), "message": error.message,
            }));
        } else {
            let count = recorder.event_count();
            let deadline = self
                .pending_record_count
                .map_or(now + Duration::from_millis(150), |(at, _)| at);
            self.pending_record_count = Some((deadline, count));
        }
    }

    /// Handles a script control command. Returns due events (none today) and a
    /// release request for pause/stop; notifications are emitted here.
    pub fn handle_command(&mut self, command: &Command, now: Instant) -> ScriptOutcome {
        match command {
            Command::ScriptRecordStart => self.start_recording(now),
            Command::ScriptRecordPause => self.pause_recording(now),
            Command::ScriptRecordResume => self.resume_recording(now),
            Command::ScriptRecordStop { name } => self.stop_recording(name, now),
            Command::ScriptRecordCancel => self.cancel_recording(),
            Command::ScriptList => {
                self.send_list();
                ScriptOutcome::default()
            }
            Command::ScriptDelete { id } => {
                let _ = self.with_id(id, |store, id| store.delete(&id));
                self.send_list();
                ScriptOutcome::default()
            }
            Command::ScriptRename { id, name } => {
                self.with_id(id, |store, id| store.rename(&id, name).map(|_| ()));
                self.send_list();
                ScriptOutcome::default()
            }
            Command::ScriptActionsGet { id } => {
                self.open_editor(id);
                ScriptOutcome::default()
            }
            Command::ScriptActionDelete {
                id,
                revision,
                index,
            } => {
                self.edit_script(id, revision, |script| script.delete_action(*index));
                ScriptOutcome::default()
            }
            Command::ScriptActionWait {
                id,
                revision,
                index,
                wait_ms,
            } => {
                self.edit_script(id, revision, |script| {
                    script.set_wait_before(*index, *wait_ms)
                });
                ScriptOutcome::default()
            }
            Command::ScriptActionMove {
                id,
                revision,
                from,
                to,
            } => {
                self.edit_script(id, revision, |script| script.move_action(*from, *to));
                ScriptOutcome::default()
            }
            Command::ScriptActionGesture {
                id,
                revision,
                index,
                x,
                y,
                end_x,
                end_y,
                duration_ms,
            } => {
                self.edit_script(id, revision, |script| {
                    script.edit_gesture(*index, *x, *y, *end_x, *end_y, *duration_ms)
                });
                ScriptOutcome::default()
            }
            Command::ScriptActionCall {
                id,
                revision,
                script,
            } => {
                if let Ok(target) = Uuid::parse_str(script) {
                    if let Err(error) = self.store.load(&target) {
                        notify_store_error(&error);
                    } else {
                        self.edit_script(id, revision, |document| document.append_call(target));
                    }
                } else {
                    notify_error(INVALID_CODE, "Invalid fragment id");
                }
                ScriptOutcome::default()
            }
            Command::ScriptImport { path } => {
                match self.store.import(path) {
                    Ok(script) => notify(json!({
                        "op":"list", "scripts": self.summaries(), "imported": script.summary(),
                    })),
                    Err(error) => notify_store_error(&error),
                }
                ScriptOutcome::default()
            }
            Command::ScriptExport { id, path } => {
                if let Ok(id) = self.parse_id(id) {
                    match self.store.export(&id, path) {
                        Ok(()) => notify(json!({"op":"list", "scripts": self.summaries()})),
                        Err(error) => notify_store_error(&error),
                    }
                }
                ScriptOutcome::default()
            }
            Command::ScriptPlayStart { id, options } => {
                self.start_playback(id, options.as_ref(), now)
            }
            Command::ScriptPlayPause => self.pause_playback(now),
            Command::ScriptPlayResume => self.resume_playback(now),
            Command::ScriptPlayStop => self.stop_playback(),
            _ => ScriptOutcome::default(),
        }
    }

    /// The next playback deadline for the input worker, or `None` when no
    /// script is playing. An idle (or paused) bridge arms no timer at all.
    pub fn next_tick(&self) -> Option<Instant> {
        let playback = self
            .playback
            .as_ref()
            .filter(|playback| !playback.is_paused())
            .map(|_| Instant::now() + Duration::from_millis(5));
        match (playback, self.pending_record_count.map(|(at, _)| at)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(at), None) | (None, Some(at)) => Some(at),
            (None, None) => None,
        }
    }

    /// Flush coalesced recording counts and advance playback.
    pub fn tick(&mut self, now: Instant) -> ScriptOutcome {
        if self.pending_record_count.is_some_and(|(at, _)| at <= now)
            && let Some((_, count)) = self.pending_record_count.take()
        {
            notify(json!({"op":"record", "state":"progress", "event_count":count}));
        }
        let Some(playback) = self.playback.as_mut() else {
            return ScriptOutcome::default();
        };
        playback.tick(now)
    }

    /// Complete a tick only after its input events have been injected.
    pub fn complete_tick(&mut self, error: Option<&str>) {
        if let Some(error) = error {
            if let Some(playback) = self.playback.take() {
                playback.notify_state("failed", Some(error));
            }
        } else if self.playback.as_ref().is_some_and(Playback::is_finished)
            && let Some(playback) = self.playback.take()
        {
            playback.notify_state("finished", None);
        }
    }

    pub fn executing_event(&mut self, offset: usize) {
        if let Some(playback) = self.playback.as_mut()
            && let Some(index) = playback.pending_positions.get(offset).copied()
            && playback.current_action != Some(index)
        {
            playback.current_action = Some(index);
            playback.notify_state("progress", None);
        }
    }

    // MARK: recording

    fn start_recording(&mut self, now: Instant) -> ScriptOutcome {
        if self.recorder.is_some() || self.playback.is_some() {
            notify_error(
                BUSY_CODE,
                "An operation recording or playback is already running",
            );
            return ScriptOutcome::default();
        }
        self.recorder = Some(Recorder::new(now));
        self.pending_record_count = None;
        notify(json!({"op":"record","state":"recording", "event_count":0}));
        ScriptOutcome::default()
    }

    fn pause_recording(&mut self, now: Instant) -> ScriptOutcome {
        match self.recorder.as_mut() {
            Some(recorder) => {
                recorder.pause(now);
                notify(json!({"op":"record","state":"paused"}));
            }
            None => notify_error(NOT_RECORDING_CODE, "No operation recording is running"),
        }
        ScriptOutcome::default()
    }

    fn resume_recording(&mut self, now: Instant) -> ScriptOutcome {
        match self.recorder.as_mut() {
            Some(recorder) => {
                recorder.resume(now);
                notify(json!({"op":"record","state":"recording"}));
            }
            None => notify_error(NOT_RECORDING_CODE, "No operation recording is running"),
        }
        ScriptOutcome::default()
    }

    fn stop_recording(&mut self, name: &str, now: Instant) -> ScriptOutcome {
        self.pending_record_count = None;
        let Some(recorder) = self.recorder.take() else {
            notify_error(NOT_RECORDING_CODE, "No operation recording is running");
            return ScriptOutcome::default();
        };
        let result = recorder.finish(
            name,
            self.device.clone(),
            self.display,
            PlaybackOptions::default(),
            now,
        );
        match result {
            Ok(script) => match self.store.save(&script) {
                Ok(()) => notify(json!({
                    "op":"record", "state":"saved",
                    "name": script.name, "script": script.summary(),
                })),
                Err(error) => notify(json!({
                    "op":"record", "state":"failed",
                    "code": error_code(&error), "message": error.message,
                })),
            },
            Err(error) => notify(json!({
                "op":"record", "state":"failed",
                "code": error_code(&error), "message": error.message,
            })),
        }
        // The recording may still hold a touch or key on the device: the
        // bridge must release it even when finishing or saving failed.
        ScriptOutcome::release()
    }

    fn cancel_recording(&mut self) -> ScriptOutcome {
        self.pending_record_count = None;
        if self.recorder.take().is_none() {
            notify_error(NOT_RECORDING_CODE, "No operation recording is running");
            return ScriptOutcome::default();
        }
        notify(json!({"op":"record","state":"stopped"}));
        // Same as stop: cancelling while an input is held must release it.
        ScriptOutcome::release()
    }

    // MARK: library

    fn summaries(&self) -> Vec<miko_automation::ScriptSummary> {
        self.store.list().unwrap_or_default()
    }

    fn send_list(&self) {
        match self.store.list() {
            Ok(scripts) => notify(json!({"op":"list","scripts":scripts})),
            Err(error) => notify_store_error(&error),
        }
    }

    fn open_editor(&mut self, id: &str) {
        let Ok(id) = self.parse_id(id) else { return };
        match self.store.load_actions(&id) {
            Ok(script) => {
                self.editor = Some((id, Uuid::new_v4(), script));
                self.send_editor();
            }
            Err(error) => notify_store_error(&error),
        }
    }

    fn edit_script(
        &mut self,
        id: &str,
        revision: &str,
        edit: impl FnOnce(
            &miko_automation::ActionDocument,
        ) -> miko_core::MikoResult<miko_automation::ActionDocument>,
    ) {
        let Ok(id) = self.parse_id(id) else { return };
        let Some((opened_id, opened_revision, original)) = self.editor.as_ref() else {
            notify_error(INVALID_CODE, "Open the script before editing it");
            return;
        };
        if *opened_id != id || opened_revision.to_string() != revision {
            notify_error(
                INVALID_CODE,
                "The open script changed; reopen it before editing",
            );
            return;
        }
        let result = self.store.load_actions(&id).and_then(|current| {
            if &current != original {
                return Err(MikoError::usage(
                    "The script changed elsewhere; reopen it before editing",
                ));
            }
            let updated = edit(&current)?;
            self.store.save_actions(&updated)?;
            Ok(updated)
        });
        match result {
            Ok(updated) => {
                self.editor = Some((id, Uuid::new_v4(), updated));
                self.send_editor();
                self.send_list();
            }
            Err(error) => notify_store_error(&error),
        }
    }

    fn send_editor(&self) {
        let Some((id, revision, script)) = &self.editor else {
            return;
        };
        let actions = &script.actions;
        let mut rows = Vec::with_capacity(actions.len());
        let mut prior_end = 0;
        let mut group_start = None;
        let mut group_end = 0;
        for (index, action) in actions.iter().enumerate() {
            let Some((start, end)) = action.time_bounds() else {
                continue;
            };
            if group_start != Some(start) {
                if group_start.is_some() {
                    prior_end = prior_end.max(group_end);
                }
                group_start = Some(start);
                group_end = end;
            } else {
                group_end = group_end.max(end);
            }
            let (kind, detail) = match action {
                miko_automation::Action::Gesture { samples, .. } => {
                    let moved = samples.first().is_some_and(|first| {
                        samples.iter().any(|sample| {
                            sample.value.x != first.value.x || sample.value.y != first.value.y
                        })
                    });
                    ("gesture", if moved { "swipe" } else { "tap" }.to_owned())
                }
                miko_automation::Action::Key { key, .. } => ("key", key.clone()),
                miko_automation::Action::Text { .. } => ("text", String::new()),
                miko_automation::Action::Paste { .. } => ("paste", String::new()),
                miko_automation::Action::ToolKey { key, .. } => ("tool_key", key.clone()),
                miko_automation::Action::ScriptCall { script, .. } => {
                    ("script_call", script.to_string())
                }
            };
            let point = match action {
                miko_automation::Action::Gesture { samples, .. } => samples
                    .first()
                    .map(|sample| json!({"x":sample.value.x,"y":sample.value.y})),
                _ => None,
            };
            let endpoint = match action {
                miko_automation::Action::Gesture { samples, .. } => samples
                    .last()
                    .map(|sample| json!({"x":sample.value.x,"y":sample.value.y})),
                _ => None,
            };
            rows.push(json!({"index":index,"kind":kind,"detail":detail,"start_ms":start,"end_ms":end,"wait_ms":start.checked_sub(prior_end),"point":point,"endpoint":endpoint}));
        }
        notify(
            json!({"op":"actions","id":id,"revision":revision,"name":script.name(),"actions":rows}),
        );
    }

    fn parse_id(&self, id: &str) -> Result<Uuid, ()> {
        match Uuid::parse_str(id) {
            Ok(id) => Ok(id),
            Err(_) => {
                notify_error(INVALID_CODE, "The script id is not a valid UUID");
                Err(())
            }
        }
    }

    fn with_id(
        &self,
        id: &str,
        operation: impl FnOnce(&ScriptStore, Uuid) -> miko_core::MikoResult<()>,
    ) -> Option<()> {
        let id = self.parse_id(id).ok()?;
        match operation(&self.store, id) {
            Ok(()) => Some(()),
            Err(error) => {
                notify_store_error(&error);
                None
            }
        }
    }

    // MARK: playback

    fn start_playback(&mut self, id: &str, options: Option<&Value>, now: Instant) -> ScriptOutcome {
        if self.recorder.is_some() || self.playback.is_some() {
            notify_error(
                BUSY_CODE,
                "An operation recording or playback is already running",
            );
            return ScriptOutcome::default();
        }
        let Ok(id) = self.parse_id(id) else {
            return ScriptOutcome::default();
        };
        let (script, positions) = match self.store.resolve_with_positions(&id) {
            Ok(result) => result,
            Err(error) => {
                notify_store_error(&error);
                return ScriptOutcome::default();
            }
        };
        let options = match options {
            Some(value) => match serde_json::from_value::<PlaybackOptions>(value.clone()) {
                Ok(options) => options,
                Err(error) => {
                    notify_error(INVALID_CODE, format!("Invalid playback options: {error}"));
                    return ScriptOutcome::default();
                }
            },
            None => script.playback,
        };
        if let Err(error) = options.validate() {
            notify(json!({
                "op":"play", "state":"failed",
                "code": error_code(&error), "message": error.message,
            }));
            return ScriptOutcome::default();
        }
        let mut playback = Playback::new(script, options, now);
        playback.positions = positions;
        playback.notify_state("playing", None);
        self.playback = Some(playback);
        ScriptOutcome::release()
    }

    fn pause_playback(&mut self, now: Instant) -> ScriptOutcome {
        let Some(playback) = self.playback.as_mut() else {
            notify_error(NOT_PLAYING_CODE, "No script playback is running");
            return ScriptOutcome::default();
        };
        playback.pause(now);
        playback.notify_state("paused", None);
        ScriptOutcome::release()
    }

    fn resume_playback(&mut self, now: Instant) -> ScriptOutcome {
        let Some(playback) = self.playback.as_mut() else {
            notify_error(NOT_PLAYING_CODE, "No script playback is running");
            return ScriptOutcome::default();
        };
        playback.resume(now);
        playback.notify_state("playing", None);
        ScriptOutcome::default()
    }

    fn stop_playback(&mut self) -> ScriptOutcome {
        let Some(playback) = self.playback.take() else {
            notify_error(NOT_PLAYING_CODE, "No script playback is running");
            return ScriptOutcome::default();
        };
        playback.notify_state("stopped", None);
        ScriptOutcome::release()
    }
}

/// One playback run: a loaded script plus the options for this run.
struct Playback {
    script: Script,
    options: PlaybackOptions,
    /// Next event index inside the current loop.
    index: usize,
    positions: Vec<usize>,
    pending_positions: Vec<usize>,
    current_action: Option<usize>,
    /// 1-based loop number used in notifications.
    loop_number: u32,
    /// Completed loops.
    loops_done: u32,
    /// Playback clock in event-time milliseconds (advanced by `speed`).
    virtual_ms: f64,
    last_tick: Instant,
    started_at: Instant,
    paused_at: Option<Instant>,
    paused_total: Duration,
    /// Set while waiting `interval_ms` between loops (real time).
    interval_until: Option<Instant>,
    finished: bool,
}

impl Playback {
    fn new(script: Script, options: PlaybackOptions, now: Instant) -> Self {
        Self {
            script,
            options,
            index: 0,
            positions: Vec::new(),
            pending_positions: Vec::new(),
            current_action: None,
            loop_number: 1,
            loops_done: 0,
            virtual_ms: 0.0,
            last_tick: now,
            started_at: now,
            paused_at: None,
            paused_total: Duration::ZERO,
            interval_until: None,
            finished: false,
        }
    }

    fn is_finished(&self) -> bool {
        self.finished
    }

    fn count(&self) -> u32 {
        self.options.count.max(1)
    }

    fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }

    fn pause(&mut self, now: Instant) {
        if self.paused_at.is_none() {
            self.paused_at = Some(now);
        }
    }

    fn resume(&mut self, now: Instant) {
        if let Some(paused_at) = self.paused_at.take() {
            let paused = now.saturating_duration_since(paused_at);
            self.paused_total += paused;
            if let Some(until) = self.interval_until {
                self.interval_until = Some(until + paused);
            }
            self.last_tick = now;
        }
    }

    fn tick(&mut self, now: Instant) -> ScriptOutcome {
        if self.finished || self.is_paused() {
            self.last_tick = now;
            return ScriptOutcome::default();
        }
        // Real-time loop interval: no event clock advance while waiting.
        if let Some(until) = self.interval_until {
            if now < until {
                self.last_tick = now;
                return ScriptOutcome::default();
            }
            self.interval_until = None;
            self.begin_loop(now);
            self.notify_state("playing", None);
        }
        let delta = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        self.virtual_ms += delta.as_secs_f64() * 1000.0 * self.options.speed;

        let mut events = Vec::new();
        self.pending_positions.clear();
        while let Some(timed) = self.script.events.get(self.index) {
            if (timed.t as f64) > self.virtual_ms {
                break;
            }
            events.push(timed.event.clone());
            self.pending_positions.push(
                self.positions
                    .get(self.index)
                    .copied()
                    .unwrap_or(self.index),
            );
            self.index += 1;
        }

        // Duration mode stops mid-run once the real active time is reached.
        if self.options.mode == PlaybackMode::Duration {
            let active = now
                .saturating_duration_since(self.started_at)
                .saturating_sub(self.paused_total);
            if active.as_millis() >= u128::from(self.options.duration_ms) {
                return self.finish(events);
            }
        }

        if self.index < self.script.events.len() {
            return ScriptOutcome {
                events,
                release: false,
            };
        }

        // The current loop ended.
        self.loops_done += 1;
        let more = match self.options.mode {
            PlaybackMode::Count => self.loops_done < self.count(),
            PlaybackMode::Infinite | PlaybackMode::Duration => true,
        };
        if !more {
            return self.finish(events);
        }
        if self.options.interval_ms > 0 {
            self.interval_until = Some(now + Duration::from_millis(self.options.interval_ms));
        } else {
            self.begin_loop(now);
            self.notify_state("playing", None);
        }
        ScriptOutcome {
            events,
            release: false,
        }
    }

    fn begin_loop(&mut self, now: Instant) {
        self.index = 0;
        self.loop_number = self.loop_number.saturating_add(1);
        self.virtual_ms = 0.0;
        self.last_tick = now;
    }

    fn finish(&mut self, mut events: Vec<Event>) -> ScriptOutcome {
        self.finished = true;
        self.virtual_ms = 0.0;
        events.shrink_to_fit();
        ScriptOutcome {
            events,
            release: true,
        }
    }

    fn notify_state(&self, state: &str, message: Option<&str>) {
        let mut value = json!({
            "op": "play",
            "state": state,
            "name": self.script.name,
            "id": self.script.id,
            "action_index": self.current_action,
            "loop": self.loop_number,
            "speed": self.options.speed,
            "interval_ms": self.options.interval_ms,
        });
        match self.options.mode {
            PlaybackMode::Count => {
                value["count"] = json!(self.count());
                value["total"] = json!(self.count());
            }
            PlaybackMode::Duration => {
                value["duration_ms"] = json!(self.options.duration_ms);
            }
            PlaybackMode::Infinite => {
                value["count"] = json!(0);
            }
        }
        if let Some(message) = message {
            value["message"] = json!(message);
        }
        notify(value);
    }
}

/// Injects one recorded event through the bridge's single input owner,
/// sharing held-input bookkeeping and release semantics with direct input.
pub async fn execute_event(
    state: &mut InputState<GrpcInput>,
    client: &Client,
    device: &str,
    panel: (u32, u32),
    rotation: &AtomicU32,
    event: &Event,
    input_lease: Option<&str>,
) -> Result<()> {
    let command = match event {
        Event::Touch { x, y, down } => Command::Touch {
            x: *x,
            y: *y,
            down: *down,
        },
        Event::MappedTouch { id, x, y, down } => Command::MappedTouch {
            id: *id,
            x: *x,
            y: *y,
            down: *down,
        },
        Event::Key { key, down } => Command::Key {
            key: key.clone(),
            down: *down,
        },
        Event::Text { text } => Command::Text { text: text.clone() },
        Event::Paste { text } => Command::Paste { text: text.clone() },
        Event::ToolKey { key } => {
            if [
                "back",
                "home",
                "recents",
                "volume_up",
                "volume_down",
                "power",
            ]
            .contains(&key.as_str())
            {
                client
                    .call(
                        "key.press",
                        json!({"device": device, "key": key, "input_lease": input_lease}),
                    )
                    .await?;
            }
            return Ok(());
        }
        Event::ScriptCall { .. } => anyhow::bail!("unresolved script fragment"),
    };
    state.command(command, panel, rotation).await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use miko_automation::{DisplayInfo, Event, PlaybackMode, PlaybackOptions, Script};

    fn sample_script(events: Vec<miko_automation::TimedEvent>) -> Script {
        Script {
            format: miko_automation::FORMAT.to_owned(),
            version: miko_automation::VERSION,
            id: Uuid::nil(),
            name: "test".to_owned(),
            created_at: chrono::Utc::now(),
            device: "Phone".to_owned(),
            display: DisplayInfo {
                width: 576,
                height: 1280,
            },
            playback: PlaybackOptions::default(),
            events,
        }
    }

    fn timed(t: u64, event: Event) -> miko_automation::TimedEvent {
        miko_automation::TimedEvent { t, event }
    }

    #[test]
    fn playback_emits_events_per_tick_and_finishes_after_the_last_loop() {
        let events = vec![
            timed(
                0,
                Event::Touch {
                    x: 0.5,
                    y: 0.4,
                    down: true,
                },
            ),
            timed(
                80,
                Event::Touch {
                    x: 0.5,
                    y: 0.4,
                    down: false,
                },
            ),
        ];
        let script = sample_script(events);
        let options = PlaybackOptions {
            mode: PlaybackMode::Count,
            count: 2,
            ..PlaybackOptions::default()
        };
        let start = Instant::now();
        let mut playback = Playback::new(script, options, start);

        let first = playback.tick(start);
        assert_eq!(first.events.len(), 1, "the t=0 event fires immediately");
        assert!(!first.release);

        let second = playback.tick(start + Duration::from_millis(100));
        assert_eq!(second.events.len(), 1);
        assert!(
            !second.release,
            "the first loop ends but count mode continues"
        );
        assert_eq!(playback.loop_number, 2);

        let third = playback.tick(start + Duration::from_millis(200));
        assert_eq!(third.events.len(), 2, "the second loop replays both events");
        assert!(third.release, "the second and last loop finished");
        assert!(playback.is_finished());
    }

    #[test]
    fn playback_speed_scales_the_event_clock() {
        let script = sample_script(vec![timed(
            1000,
            Event::Text {
                text: "x".to_owned(),
            },
        )]);
        let options = PlaybackOptions {
            speed: 4.0,
            ..PlaybackOptions::default()
        };
        let start = Instant::now();
        let mut playback = Playback::new(script, options, start);
        assert!(
            playback
                .tick(start + Duration::from_millis(200))
                .events
                .is_empty()
        );
        let outcome = playback.tick(start + Duration::from_millis(300));
        assert_eq!(
            outcome.events.len(),
            1,
            "4x speed reaches t=1000 after 250 ms"
        );
    }

    #[test]
    fn playback_interval_waits_in_real_time_between_loops() {
        let script = sample_script(vec![timed(
            0,
            Event::Text {
                text: "x".to_owned(),
            },
        )]);
        let options = PlaybackOptions {
            mode: PlaybackMode::Count,
            count: 2,
            interval_ms: 50,
            ..PlaybackOptions::default()
        };
        let start = Instant::now();
        let mut playback = Playback::new(script, options, start);
        assert_eq!(playback.tick(start).events.len(), 1, "first loop event");
        assert!(
            playback
                .tick(start + Duration::from_millis(10))
                .events
                .is_empty()
        );
        assert!(
            playback
                .tick(start + Duration::from_millis(40))
                .events
                .is_empty()
        );
        let second_loop = playback.tick(start + Duration::from_millis(70));
        assert_eq!(
            second_loop.events.len(),
            1,
            "second loop after the interval"
        );
        assert!(second_loop.release);
    }

    #[test]
    fn playback_pause_freezes_the_event_clock() {
        let script = sample_script(vec![
            timed(
                0,
                Event::Touch {
                    x: 0.5,
                    y: 0.5,
                    down: true,
                },
            ),
            timed(
                100,
                Event::Touch {
                    x: 0.5,
                    y: 0.5,
                    down: false,
                },
            ),
        ]);
        let start = Instant::now();
        let mut playback = Playback::new(script, PlaybackOptions::default(), start);
        assert_eq!(playback.tick(start).events.len(), 1);
        playback.pause(start + Duration::from_millis(10));
        assert!(
            playback
                .tick(start + Duration::from_millis(500))
                .events
                .is_empty()
        );
        playback.resume(start + Duration::from_millis(500));
        let outcome = playback.tick(start + Duration::from_millis(600));
        assert_eq!(outcome.events.len(), 1, "the paused time is skipped");
        assert!(outcome.release);
    }

    #[test]
    fn playback_duration_mode_stops_mid_script() {
        let script = sample_script(vec![
            timed(
                0,
                Event::Text {
                    text: "a".to_owned(),
                },
            ),
            timed(
                10_000,
                Event::Text {
                    text: "b".to_owned(),
                },
            ),
        ]);
        let options = PlaybackOptions {
            mode: PlaybackMode::Duration,
            duration_ms: 100,
            ..PlaybackOptions::default()
        };
        let start = Instant::now();
        let mut playback = Playback::new(script, options, start);
        assert_eq!(playback.tick(start).events.len(), 1);
        let outcome = playback.tick(start + Duration::from_millis(150));
        assert!(outcome.release, "duration reached and playback finished");
        assert!(playback.is_finished());
    }

    fn recording_manager() -> (tempfile::TempDir, ScriptManager) {
        let temp = tempfile::tempdir().expect("temp dir");
        let manager = ScriptManager::new(temp.path().to_path_buf(), "dev".to_owned(), 576, 1280);
        (temp, manager)
    }

    #[test]
    fn composite_playback_reports_the_parent_action_for_fragment_events() {
        let (_temp, mut manager) = recording_manager();
        let mut parent = sample_script(vec![timed(
            0,
            Event::Text {
                text: "parent".into(),
            },
        )]);
        let mut child = sample_script(vec![timed(
            0,
            Event::Text {
                text: "child".into(),
            },
        )]);
        parent.id = Uuid::new_v4();
        child.id = Uuid::new_v4();
        manager.store.save(&parent).unwrap();
        manager.store.save(&child).unwrap();
        let document = manager
            .store
            .load_actions(&parent.id)
            .unwrap()
            .append_call(child.id)
            .unwrap();
        manager.store.save_actions(&document).unwrap();
        let now = Instant::now();
        manager.handle_command(
            &Command::ScriptPlayStart {
                id: parent.id.to_string(),
                options: None,
            },
            now,
        );
        let outcome = manager.tick(now);
        assert_eq!(outcome.events.len(), 2);
        manager.executing_event(0);
        assert_eq!(manager.playback.as_ref().unwrap().current_action, Some(0));
        manager.executing_event(1);
        assert_eq!(manager.playback.as_ref().unwrap().current_action, Some(1));
        manager.complete_tick(None);
        assert!(!manager.is_playing());
    }

    #[test]
    fn input_failure_discards_an_incomplete_recording() {
        let (_temp, mut manager) = recording_manager();
        let now = Instant::now();
        manager.handle_command(&Command::ScriptRecordStart, now);
        manager.record_user_command(
            &Command::Key {
                key: "A".into(),
                down: true,
            },
            now,
        );
        manager.fail_recording("device input failed");
        assert!(!manager.is_recording());
        assert!(manager.pending_record_count.is_none());
        assert!(manager.store.list().expect("library").is_empty());
    }

    #[test]
    fn editor_rejects_stale_requests_and_external_changes() {
        let (_temp, mut manager) = recording_manager();
        let script = sample_script(vec![
            timed(
                10,
                Event::Text {
                    text: "first".into(),
                },
            ),
            timed(
                20,
                Event::Text {
                    text: "second".into(),
                },
            ),
        ]);
        manager.store.save(&script).expect("save");
        let id = script.id.to_string();
        manager.open_editor(&id);
        let revision = manager.editor.as_ref().expect("open").1.to_string();
        manager.edit_script(&id, &revision, |script| script.delete_action(0));
        assert_eq!(
            manager.store.load(&script.id).expect("load").events.len(),
            1
        );
        manager.edit_script(&id, &revision, |script| script.delete_action(0));
        assert_eq!(
            manager
                .store
                .load(&script.id)
                .expect("stale edit did not delete")
                .events
                .len(),
            1
        );

        let revision = manager.editor.as_ref().expect("open").1.to_string();
        let mut external = manager.store.load(&script.id).expect("load");
        external.name = "changed elsewhere".into();
        manager.store.save(&external).expect("external save");
        manager.edit_script(&id, &revision, |script| script.set_wait_before(0, 200));
        assert_eq!(
            manager
                .store
                .load(&script.id)
                .expect("external change retained"),
            external
        );
    }

    #[test]
    fn playback_waits_for_injection_result_before_finishing() {
        let (_temp, mut manager) = recording_manager();
        let now = Instant::now();
        let script = sample_script(vec![timed(
            0,
            Event::Key {
                key: "ENTER".to_owned(),
                down: true,
            },
        )]);
        manager.playback = Some(Playback::new(
            script.clone(),
            PlaybackOptions::default(),
            now,
        ));
        let outcome = manager.tick(now);
        assert!(outcome.release);
        assert!(manager.is_playing());
        manager.complete_tick(Some("injection failed"));
        assert!(!manager.is_playing());

        manager.playback = Some(Playback::new(script, PlaybackOptions::default(), now));
        manager.tick(now);
        assert!(manager.is_playing());
        manager.complete_tick(None);
        assert!(!manager.is_playing());
    }

    #[test]
    fn recording_can_be_saved_listed_and_played_with_requested_options() {
        let (_temp, mut manager) = recording_manager();
        let start = Instant::now();
        manager.handle_command(&Command::ScriptRecordStart, start);
        manager.record_user_command(
            &Command::Key {
                key: "ENTER".to_owned(),
                down: true,
            },
            start,
        );
        manager.record_user_command(
            &Command::Key {
                key: "ENTER".to_owned(),
                down: false,
            },
            start + Duration::from_millis(20),
        );
        assert_eq!(
            manager.pending_record_count.map(|(_, count)| count),
            Some(2)
        );
        assert!(manager.next_tick().is_some());
        manager.tick(start + Duration::from_millis(200));
        assert!(manager.pending_record_count.is_none());
        manager.handle_command(
            &Command::ScriptRecordStop {
                name: "Recording".to_owned(),
            },
            start + Duration::from_millis(210),
        );
        let scripts = manager.store.list().expect("saved recording can be listed");
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].event_count, 2);
        let options =
            json!({"mode":"count", "count":2, "duration_ms":0, "interval_ms":300, "speed":2.0});
        let start_outcome = manager.handle_command(
            &Command::ScriptPlayStart {
                id: scripts[0].id.to_string(),
                options: Some(options),
            },
            start + Duration::from_secs(1),
        );
        assert!(
            start_outcome.release,
            "playback must release previous manual input"
        );
        let playback = manager
            .playback
            .as_ref()
            .expect("saved recording starts playback");
        assert_eq!(playback.options.count, 2);
        assert_eq!(playback.options.interval_ms, 300);
        assert_eq!(playback.options.speed, 2.0);
        assert_eq!(manager.tick(start + Duration::from_secs(1)).events.len(), 1);
        assert!(
            manager
                .handle_command(&Command::ScriptPlayStop, start + Duration::from_secs(1))
                .release
        );
    }

    #[test]
    fn stopping_a_recording_releases_held_device_inputs() {
        let (_temp, mut manager) = recording_manager();
        let start = Instant::now();
        manager.handle_command(&Command::ScriptRecordStart, start);
        manager.record_user_command(
            &Command::Key {
                key: "ENTER".to_owned(),
                down: true,
            },
            start + Duration::from_millis(10),
        );
        assert_eq!(
            manager.recorder.as_ref().map(|r| r.event_count()),
            Some(1),
            "the key down is being recorded"
        );
        let outcome = manager.handle_command(
            &Command::ScriptRecordStop {
                name: "daily".to_owned(),
            },
            start + Duration::from_millis(20),
        );
        assert!(outcome.release, "stop must release held inputs");
        assert!(manager.recorder.is_none());
    }

    #[test]
    fn failed_recording_stop_still_releases_held_device_inputs() {
        let (_temp, mut manager) = recording_manager();
        let start = Instant::now();
        manager.handle_command(&Command::ScriptRecordStart, start);
        manager.record_user_command(
            &Command::Touch {
                x: 0.5,
                y: 0.5,
                down: true,
            },
            start + Duration::from_millis(10),
        );
        // An empty name fails validation inside Recorder::finish.
        let outcome = manager.handle_command(
            &Command::ScriptRecordStop {
                name: String::new(),
            },
            start + Duration::from_millis(20),
        );
        assert!(
            outcome.release,
            "a failed stop must still release held inputs"
        );
    }

    #[test]
    fn cancelling_a_recording_releases_held_device_inputs() {
        let (_temp, mut manager) = recording_manager();
        let start = Instant::now();
        manager.handle_command(&Command::ScriptRecordStart, start);
        manager.record_user_command(
            &Command::Key {
                key: "W".to_owned(),
                down: true,
            },
            start + Duration::from_millis(10),
        );
        let outcome = manager.handle_command(
            &Command::ScriptRecordCancel,
            start + Duration::from_millis(20),
        );
        assert!(outcome.release, "cancel must release held inputs");
        assert!(manager.recorder.is_none());
    }

    #[test]
    fn cancelling_without_a_recording_does_not_release_anything() {
        let (_temp, mut manager) = recording_manager();
        let outcome = manager.handle_command(&Command::ScriptRecordCancel, Instant::now());
        assert!(!outcome.release);
    }

    #[test]
    fn recording_whitelist_matches_the_documented_events() {
        assert!(
            event_from_command(&Command::Touch {
                x: 0.5,
                y: 0.5,
                down: true
            })
            .is_some()
        );
        assert!(
            event_from_command(&Command::MappedTouch {
                id: 4,
                x: 0.1,
                y: 0.2,
                down: false
            })
            .is_some()
        );
        assert!(
            event_from_command(&Command::Key {
                key: "ENTER".to_owned(),
                down: true
            })
            .is_some()
        );
        assert!(
            event_from_command(&Command::Text {
                text: "hi".to_owned()
            })
            .is_some()
        );
        assert!(
            event_from_command(&Command::Paste {
                text: "hi".to_owned()
            })
            .is_some()
        );
        assert!(
            event_from_command(&Command::ToolKey {
                key: "back".to_owned()
            })
            .is_some()
        );
        assert!(event_from_command(&Command::Rotate { direction: 1 }).is_none());
        assert!(event_from_command(&Command::Copy { request_id: 1 }).is_none());
        assert!(
            event_from_command(&Command::Screenshot {
                path: "a".to_owned()
            })
            .is_none()
        );
    }

    #[test]
    fn record_precheck_matches_the_format_bounds() {
        assert!(event_fits_the_format(&Event::Touch {
            x: 0.5,
            y: 0.5,
            down: true
        }));
        assert!(!event_fits_the_format(&Event::Touch {
            x: 1.5,
            y: 0.5,
            down: true
        }));
        assert!(event_fits_the_format(&Event::MappedTouch {
            id: 3,
            x: 0.0,
            y: 1.0,
            down: true
        }));
        assert!(!event_fits_the_format(&Event::MappedTouch {
            id: 2,
            x: 0.5,
            y: 0.5,
            down: true
        }));
        assert!(event_fits_the_format(&Event::Key {
            key: "ENTER".to_owned(),
            down: true
        }));
        assert!(!event_fits_the_format(&Event::Key {
            key: String::new(),
            down: true
        }));
        assert!(!event_fits_the_format(&Event::ToolKey {
            key: String::new()
        }));
        assert!(event_fits_the_format(&Event::Text {
            text: "hi".to_owned()
        }));
    }

    #[test]
    fn manual_press_takes_control_but_stale_release_does_not() {
        let release = Command::Touch {
            x: 0.5,
            y: 0.5,
            down: false,
        };
        let press = Command::Touch {
            x: 0.5,
            y: 0.5,
            down: true,
        };
        let temp = tempfile::tempdir().expect("temp dir");
        let mut manager =
            ScriptManager::new(temp.path().to_path_buf(), "dev".to_owned(), 576, 1280);
        assert!(!manager.take_manual_control(&press));
        manager.playback = Some(Playback::new(
            sample_script(vec![timed(
                0,
                Event::Text {
                    text: "x".to_owned(),
                },
            )]),
            PlaybackOptions::default(),
            Instant::now(),
        ));
        assert!(!manager.take_manual_control(&release));
        assert!(manager.is_playing());
        assert!(manager.take_manual_control(&press));
        assert!(!manager.is_playing());
    }
}

//! Persistent script library under `<Miko data>/scripts/`.
//!
//! One pretty-printed JSON file per script, named by id (`<uuid>.json`), so a
//! script is portable: import/export moves the same file. Writes are atomic
//! (temp file + rename) so a crash never leaves a truncated script, and
//! `list()` skips files that are not valid scripts instead of failing the
//! whole library.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use miko_core::{ErrorCode, MikoError, MikoResult, ResultExt};
use uuid::Uuid;

use crate::model::{Script, ScriptSummary, VERSION};
use crate::validate::{validate, validate_name};

/// Largest script file the store reads or imports, in bytes.
pub const MAX_SCRIPT_BYTES: u64 = 64 * 1024 * 1024;

/// Editable action list and script metadata. Raw input events are expanded
/// only when validating or playing the recording.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionDocument {
    metadata: Script,
    pub actions: Vec<crate::Action>,
}

impl ActionDocument {
    pub fn name(&self) -> &str {
        &self.metadata.name
    }

    pub fn to_script(&self) -> MikoResult<Script> {
        crate::script_with_actions(&self.metadata, &self.actions)
    }

    pub fn delete_action(&self, index: usize) -> MikoResult<Self> {
        let mut result = self.clone();
        result.actions = crate::actions::remove_action(&self.actions, index)?;
        result.to_script()?;
        Ok(result)
    }

    pub fn set_wait_before(&self, index: usize, wait_ms: u64) -> MikoResult<Self> {
        let mut result = self.clone();
        result.actions = crate::actions::set_action_wait(&self.actions, index, wait_ms)?;
        result.to_script()?;
        Ok(result)
    }

    pub fn move_action(&self, from: usize, to: usize) -> MikoResult<Self> {
        let mut result = self.clone();
        result.actions = crate::actions::move_action(&self.actions, from, to)?;
        result.to_script()?;
        Ok(result)
    }

    pub fn edit_gesture(
        &self,
        index: usize,
        x: f64,
        y: f64,
        end_x: f64,
        end_y: f64,
        duration_ms: u64,
    ) -> MikoResult<Self> {
        let mut result = self.clone();
        result.actions =
            crate::actions::edit_gesture(&self.actions, index, x, y, end_x, end_y, duration_ms)?;
        result.to_script()?;
        Ok(result)
    }

    pub fn append_call(&self, script: Uuid) -> MikoResult<Self> {
        let mut result = self.clone();
        let t = result
            .actions
            .iter()
            .filter_map(|a| a.time_bounds().map(|(_, end)| end))
            .max()
            .unwrap_or(0);
        let order = result
            .actions
            .iter()
            .map(crate::Action::max_order)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| MikoError::usage("too many actions"))?;
        result
            .actions
            .push(crate::Action::ScriptCall { t, order, script });
        result.to_script()?;
        Ok(result)
    }
}

/// Unique suffix for the temporary file of an atomic write.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The script library rooted at one Miko data directory.
#[derive(Debug, Clone)]
pub struct ScriptStore {
    dir: PathBuf,
}

impl ScriptStore {
    /// Uses `<data_dir>/scripts` as the library directory (it is created on
    /// the first save, and a missing directory simply lists as empty).
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            dir: data_dir.as_ref().join("scripts"),
        }
    }

    /// The library directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Lists valid scripts, newest first. Corrupt or foreign files are skipped.
    pub fn list(&self) -> MikoResult<Vec<ScriptSummary>> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(MikoError::unexpected(format!(
                    "listing {}: {error}",
                    self.dir.display()
                )));
            }
        };
        let mut scripts = Vec::new();
        for entry in entries {
            let entry = entry.ctx(
                ErrorCode::Unexpected,
                format!("listing {}", self.dir.display()),
            )?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json")
                || !path.is_file()
            {
                continue;
            }
            // A corrupt script must not hide the rest of the library.
            if let Ok(script) = self.read_script(&path) {
                scripts.push(script.summary());
            }
        }
        scripts.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(scripts)
    }

    /// Loads and validates one script.
    pub fn load(&self, id: &Uuid) -> MikoResult<Script> {
        self.read_script(&self.path_for(id))
    }

    /// Resolves fragment calls into a playback timeline. Every device uses its
    /// own playback and a depth limit prevents recursive calls from looping.
    pub fn resolve_playback(&self, id: &Uuid) -> MikoResult<Script> {
        self.resolve_at_depth(id, 0).map(|(script, _)| script)
    }

    pub fn resolve_with_positions(&self, id: &Uuid) -> MikoResult<(Script, Vec<usize>)> {
        self.resolve_at_depth(id, 0)
    }

    fn resolve_at_depth(&self, id: &Uuid, depth: usize) -> MikoResult<(Script, Vec<usize>)> {
        if depth > 10 {
            return Err(MikoError::usage("script call depth exceeds 10"));
        }
        let document = self.load_actions(id)?;
        for action in &document.actions {
            if let crate::Action::ScriptCall { t, .. } = action
                && document.actions.iter().any(|other| {
                    other
                        .time_bounds()
                        .is_some_and(|(start, end)| start < *t && end > *t)
                })
            {
                return Err(MikoError::usage(
                    "cannot call a script during overlapping input",
                ));
            }
        }
        let mut script = document.to_script()?;
        let mut keys: Vec<_> = document
            .actions
            .iter()
            .enumerate()
            .flat_map(|(index, action)| {
                action
                    .event_keys()
                    .into_iter()
                    .map(move |(time, order)| (time, order, index))
            })
            .collect();
        keys.sort_by_key(|(time, order, _)| (*time, *order));
        let mut events = Vec::new();
        let mut positions = Vec::new();
        let mut extra = 0u64;
        let mut held = std::collections::HashSet::new();
        for (timed, (_, _, action_index)) in script.events.iter().zip(keys) {
            let at = timed
                .t
                .checked_add(extra)
                .ok_or_else(|| MikoError::usage("script duration is too long"))?;
            if let crate::Event::ScriptCall { script: child } = timed.event {
                if !held.is_empty() {
                    return Err(MikoError::usage(format!(
                        "action {} cannot call a script while input is held",
                        action_index + 1
                    )));
                }
                let (child, _) =
                    self.resolve_at_depth(&child, depth + 1)
                        .map_err(|mut error| {
                            error.message = format!(
                                "action {} fragment {}: {}",
                                action_index + 1,
                                child,
                                error.message
                            );
                            error
                        })?;
                for event in child.events {
                    let t = at
                        .checked_add(event.t)
                        .ok_or_else(|| MikoError::usage("script duration is too long"))?;
                    events.push(crate::TimedEvent {
                        t,
                        event: event.event,
                    });
                    positions.push(action_index);
                }
                extra = events
                    .last()
                    .map_or(extra, |last| last.t.saturating_sub(timed.t));
            } else {
                let change = match &timed.event {
                    crate::Event::Touch { down, .. } => Some(("touch".to_owned(), *down)),
                    crate::Event::MappedTouch { id, down, .. } => {
                        Some((format!("touch:{id}"), *down))
                    }
                    crate::Event::Key { key, down } => Some((format!("key:{key}"), *down)),
                    _ => None,
                };
                if let Some((key, down)) = change {
                    if down {
                        held.insert(key);
                    } else {
                        held.remove(&key);
                    }
                }
                events.push(crate::TimedEvent {
                    t: at,
                    event: timed.event.clone(),
                });
                positions.push(action_index);
            }
            if events.len() > crate::MAX_EVENTS {
                return Err(MikoError::usage("combined script contains too many events"));
            }
        }
        if depth > 0 && !held.is_empty() {
            return Err(MikoError::usage(
                "script fragment ends with input still held",
            ));
        }
        script.events = events;
        validate(&script)?;
        Ok((script, positions))
    }

    /// Loads the canonical editable actions, converting a legacy timeline once.
    pub fn load_actions(&self, id: &Uuid) -> MikoResult<ActionDocument> {
        let path = self.path_for(id);
        let size = std::fs::metadata(&path)
            .ctx(ErrorCode::NotFound, format!("reading {}", path.display()))?
            .len();
        if size > MAX_SCRIPT_BYTES {
            return Err(MikoError::usage(format!(
                "{} is larger than the {MAX_SCRIPT_BYTES} byte limit",
                path.display()
            )));
        }
        let bytes = std::fs::read(&path)
            .ctx(ErrorCode::Unexpected, format!("reading {}", path.display()))?;
        decode_document(&bytes, &path)
    }

    /// Validates and atomically saves an editable action document.
    pub fn save_actions(&self, document: &ActionDocument) -> MikoResult<()> {
        let bytes = encode_document(document)?;
        let path = self.path_for(&document.metadata.id);
        if path.exists() {
            self.read_script(&path)?;
        }
        std::fs::create_dir_all(&self.dir).ctx(
            ErrorCode::Unexpected,
            format!("creating {}", self.dir.display()),
        )?;
        write_atomic(&path, &bytes)
    }

    /// Validates and writes a script atomically.
    pub fn save(&self, script: &Script) -> MikoResult<()> {
        // Conversion validates before any library file is touched.
        let bytes = encode(script)?;
        let path = self.path_for(&script.id);
        // Do not replace a file whose format we cannot understand. In
        // particular, an older build must not overwrite a newer script.
        if path.exists() {
            self.read_script(&path)?;
        }
        std::fs::create_dir_all(&self.dir).ctx(
            ErrorCode::Unexpected,
            format!("creating {}", self.dir.display()),
        )?;
        write_atomic(&path, &bytes)
    }

    /// Deletes one script; missing ids are a not-found error.
    pub fn delete(&self, id: &Uuid) -> MikoResult<()> {
        let path = self.path_for(id);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(MikoError::not_found(format!("script {id} not found")))
            }
            Err(error) => Err(MikoError::unexpected(format!(
                "deleting {}: {error}",
                path.display()
            ))),
        }
    }

    /// Renames one script and returns the updated copy.
    pub fn rename(&self, id: &Uuid, name: &str) -> MikoResult<Script> {
        validate_name(name)?;
        let mut document = self.load_actions(id)?;
        document.metadata.name = name.to_owned();
        self.save_actions(&document)?;
        document.to_script()
    }

    /// Imports an exported script file under a fresh id and saves it.
    ///
    /// The original name, timestamps and timeline are preserved; only the id
    /// changes so an import can never overwrite an existing script.
    pub fn import(&self, path: impl AsRef<Path>) -> MikoResult<Script> {
        let path = path.as_ref();
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(MikoError::not_found(format!(
                    "no script file at {}",
                    path.display()
                )));
            }
            Err(error) => {
                return Err(MikoError::unexpected(format!(
                    "reading {}: {error}",
                    path.display()
                )));
            }
        };
        if !metadata.is_file() {
            return Err(MikoError::usage(format!(
                "{} is not a script file",
                path.display()
            )));
        }
        if metadata.len() > MAX_SCRIPT_BYTES {
            return Err(MikoError::usage(format!(
                "the script file is larger than {MAX_SCRIPT_BYTES} bytes"
            )));
        }
        let bytes = std::fs::read(path)
            .ctx(ErrorCode::Unexpected, format!("reading {}", path.display()))?;
        let mut document = decode_document(&bytes, path)?;
        document.metadata.id = Uuid::new_v4();
        self.save_actions(&document)?;
        document.to_script()
    }

    /// Writes one stored script to `path` (an export; the library is unchanged).
    pub fn export(&self, id: &Uuid, path: impl AsRef<Path>) -> MikoResult<()> {
        let path = path.as_ref();
        let document = self.load_actions(id)?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.is_dir()
        {
            return Err(MikoError::usage(format!(
                "the export destination directory does not exist: {}",
                parent.display()
            )));
        }
        let bytes = encode_document(&document)?;
        std::fs::write(path, bytes)
            .map_err(|error| MikoError::usage(format!("cannot write {}: {error}", path.display())))
    }

    fn path_for(&self, id: &Uuid) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn read_script(&self, path: &Path) -> MikoResult<Script> {
        match std::fs::metadata(path) {
            Ok(metadata) if metadata.len() > MAX_SCRIPT_BYTES => {
                return Err(MikoError::usage(format!(
                    "{} is larger than the {MAX_SCRIPT_BYTES} byte limit",
                    path.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(MikoError::not_found(format!(
                    "no script file at {}",
                    path.display()
                )));
            }
            Err(error) => {
                return Err(MikoError::unexpected(format!(
                    "reading {}: {error}",
                    path.display()
                )));
            }
        }
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(MikoError::not_found(format!(
                    "no script file at {}",
                    path.display()
                )));
            }
            Err(error) => {
                return Err(MikoError::unexpected(format!(
                    "reading {}: {error}",
                    path.display()
                )));
            }
        };
        decode(&bytes, path)
    }
}

fn decode_document(bytes: &[u8], path: &Path) -> MikoResult<ActionDocument> {
    let mut metadata = decode(bytes, path)?;
    let json: serde_json::Value = serde_json::from_slice(bytes)
        .ctx(ErrorCode::Usage, format!("reading {}", path.display()))?;
    let mut actions: Vec<crate::Action> = if json["version"].as_u64().is_some_and(|v| v >= 3) {
        serde_json::from_value(json["actions"].clone())
            .ctx(ErrorCode::Usage, "invalid script actions")?
    } else {
        crate::actions_from_script(&metadata)?
    };
    actions.sort_by_key(|action| action.time_bounds().map(|bounds| bounds.0));
    metadata.events = Vec::new();
    Ok(ActionDocument { metadata, actions })
}

fn encode_document(document: &ActionDocument) -> MikoResult<Vec<u8>> {
    document.to_script()?;
    let mut value = serde_json::to_value(&document.metadata)
        .ctx(ErrorCode::Unexpected, "encoding the script")?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| MikoError::unexpected("script is not an object"))?;
    object.remove("events");
    object.insert(
        "version".into(),
        serde_json::json!(if document
            .actions
            .iter()
            .any(|a| matches!(a, crate::Action::ScriptCall { .. }))
        {
            4
        } else {
            3
        }),
    );
    object.insert(
        "actions".into(),
        serde_json::to_value(&document.actions).ctx(ErrorCode::Unexpected, "encoding actions")?,
    );
    let mut bytes =
        serde_json::to_vec_pretty(&value).ctx(ErrorCode::Unexpected, "encoding the script")?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Serialises a script as pretty JSON with a trailing newline.
fn encode(script: &Script) -> MikoResult<Vec<u8>> {
    let actions = crate::actions_from_script(script)?;
    let mut document =
        serde_json::to_value(script).ctx(ErrorCode::Unexpected, "encoding the script")?;
    let object = document
        .as_object_mut()
        .ok_or_else(|| MikoError::unexpected("script is not an object"))?;
    object.remove("events");
    object.insert(
        "version".into(),
        serde_json::json!(if actions
            .iter()
            .any(|a| matches!(a, crate::Action::ScriptCall { .. }))
        {
            4
        } else {
            3
        }),
    );
    object.insert(
        "actions".into(),
        serde_json::to_value(actions).ctx(ErrorCode::Unexpected, "encoding actions")?,
    );
    let mut bytes =
        serde_json::to_vec_pretty(&document).ctx(ErrorCode::Unexpected, "encoding the script")?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Parses and validates a script, reporting `usage` errors with the file name.
fn decode(bytes: &[u8], path: &Path) -> MikoResult<Script> {
    // Read the header and retain the untyped body in one pass: future versions
    // may use event types or fields this build cannot deserialize.
    #[derive(serde::Deserialize)]
    struct Document {
        format: String,
        version: u64,
        #[serde(flatten)]
        body: serde_json::Map<String, serde_json::Value>,
    }
    let Document {
        format,
        version,
        mut body,
    } = serde_json::from_slice(bytes).map_err(|error| {
        MikoError::usage(format!(
            "{} is not a valid operation recording: {error}",
            path.display()
        ))
    })?;
    if format == crate::model::FORMAT && version > u64::from(VERSION) {
        return Err(MikoError::usage(format!(
            "{}: operation-recording version {} requires a newer version of miko; please upgrade",
            path.display(),
            version
        )));
    }
    body.insert("format".into(), format.into());
    body.insert("version".into(), version.into());
    let actions = if version == 3 || version == 4 {
        if body.contains_key("events") {
            return Err(MikoError::usage(
                "version 3 scripts must not contain an event timeline",
            ));
        }
        let actions = body
            .remove("actions")
            .ok_or_else(|| MikoError::usage("version 3 script has no actions"))?;
        let actions: Vec<crate::Action> = serde_json::from_value(actions)
            .map_err(|error| MikoError::usage(format!("invalid script actions: {error}")))?;
        if version == 3
            && actions
                .iter()
                .any(|a| matches!(a, crate::Action::ScriptCall { .. }))
        {
            return Err(MikoError::usage("script calls require version 4"));
        }
        body.insert("version".into(), serde_json::json!(VERSION));
        body.insert("events".into(), serde_json::json!([]));
        Some(actions)
    } else if version == 2 {
        if body.contains_key("actions") {
            return Err(MikoError::usage(
                "version 2 scripts must not contain actions",
            ));
        }
        body.insert("version".into(), serde_json::json!(VERSION));
        None
    } else {
        return Err(MikoError::usage(format!(
            "unsupported operation-recording version {}",
            version
        )));
    };
    let script: Script =
        serde_json::from_value(serde_json::Value::Object(body)).map_err(|error| {
            MikoError::usage(format!(
                "{} is not a valid operation recording: {error}",
                path.display()
            ))
        })?;
    if let Some(actions) = actions {
        // Expanding actions also validates the resulting timeline.
        crate::script_with_actions(&script, &actions)
    } else {
        validate(&script)
            .map_err(|error| MikoError::usage(format!("{}: {}", path.display(), error.message)))?;
        Ok(script)
    }
}

/// Writes `bytes` to `path` atomically: a temp file in the same directory is
/// renamed into place, so a crash never leaves a half-written script.
fn write_atomic(path: &Path, bytes: &[u8]) -> MikoResult<()> {
    let dir = path.parent().ok_or_else(|| {
        MikoError::unexpected(format!("{} has no parent directory", path.display()))
    })?;
    std::fs::create_dir_all(dir)
        .ctx(ErrorCode::Unexpected, format!("creating {}", dir.display()))?;
    let suffix = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(format!(
        ".miko-automation-{}-{suffix}.tmp",
        std::process::id()
    ));
    std::fs::write(&temp, bytes)
        .ctx(ErrorCode::Unexpected, format!("writing {}", temp.display()))?;
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(MikoError::unexpected(format!(
            "writing {}: {error}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Event, PlaybackMode, PlaybackOptions, VERSION};
    use crate::test_support::fixture;
    use chrono::DateTime;
    use tempfile::TempDir;

    fn script(name: &str, created_at: i64) -> Script {
        let mut script = fixture();
        script.id = Uuid::new_v4();
        script.name = name.to_owned();
        script.created_at = DateTime::from_timestamp_millis(created_at).expect("timestamp");
        script
    }

    fn store() -> (TempDir, ScriptStore) {
        let dir = TempDir::new().expect("temp dir");
        let store = ScriptStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn composed_scripts_expand_in_order_and_limit_recursion() {
        let (_dir, store) = store();
        let parent = script("parent", 1_700_000_000_000);
        let child = script("child", 1_700_000_000_001);
        store.save(&parent).unwrap();
        store.save(&child).unwrap();
        let composite = store
            .load_actions(&parent.id)
            .unwrap()
            .append_call(child.id)
            .unwrap();
        store.save_actions(&composite).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(store.path_for(&parent.id)).unwrap()).unwrap();
        assert_eq!(value["version"], 4);
        let (resolved, positions) = store.resolve_with_positions(&parent.id).unwrap();
        assert_eq!(
            resolved.events.len(),
            parent.events.len() + child.events.len()
        );
        assert_eq!(positions.len(), resolved.events.len());
        assert_eq!(positions.last(), Some(&(composite.actions.len() - 1)));
        assert!(
            resolved
                .events
                .windows(2)
                .all(|pair| pair[0].t <= pair[1].t)
        );
        let cyclic = store
            .load_actions(&child.id)
            .unwrap()
            .append_call(parent.id)
            .unwrap();
        store.save_actions(&cyclic).unwrap();
        assert!(
            store
                .resolve_playback(&parent.id)
                .unwrap_err()
                .message
                .contains("depth exceeds 10")
        );
    }

    #[test]
    fn editing_preserves_stored_action_boundaries() {
        let (_dir, store) = store();
        let script = script("actions", 1_700_000_000_000);
        store.save(&script).expect("save");
        let mut document = store.load_actions(&script.id).expect("load actions");
        document.actions = vec![
            crate::Action::Key {
                key: "A".into(),
                samples: vec![crate::ActionSample {
                    t: 0,
                    order: 0,
                    value: true,
                }],
            },
            crate::Action::Key {
                key: "A".into(),
                samples: vec![
                    crate::ActionSample {
                        t: 10,
                        order: 1,
                        value: true,
                    },
                    crate::ActionSample {
                        t: 20,
                        order: 2,
                        value: false,
                    },
                ],
            },
            crate::Action::Text {
                t: 30,
                order: 3,
                text: "done".into(),
            },
        ];
        store.save_actions(&document).expect("save actions");
        let loaded = store.load_actions(&script.id).expect("reload actions");
        assert_eq!(loaded.actions, document.actions);
        store.rename(&script.id, "renamed").expect("rename");
        let export = store.dir().join("export.json");
        store.export(&script.id, &export).expect("export");
        let imported = store.import(&export).expect("import");
        assert_eq!(
            store
                .load_actions(&imported.id)
                .expect("imported actions")
                .actions,
            document.actions
        );
        let edited = loaded.delete_action(2).expect("delete text");
        store.save_actions(&edited).expect("save edit");
        assert_eq!(
            store.load_actions(&script.id).expect("reload edit").actions,
            edited.actions
        );
    }

    #[test]
    fn save_load_and_summarise_round_trip() {
        let (_dir, store) = store();
        let script = script("first", 1_700_000_000_000);
        store.save(&script).expect("save");
        let loaded = store.load(&script.id).expect("load");
        assert_eq!(loaded, script);
        let summaries = store.list().expect("list");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0], script.summary());
        assert_eq!(summaries[0].duration_ms, script.duration_ms());
        assert!(store.dir().join(format!("{}.json", script.id)).is_file());
        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(store.path_for(&script.id)).expect("read file"))
                .expect("JSON");
        assert_eq!(stored["version"], 3);
        assert!(stored.get("actions").is_some());
        assert!(stored.get("events").is_none());
    }

    #[test]
    fn imports_legacy_v2_without_modifying_the_source() {
        let (dir, store) = store();
        let mut original = script("legacy", 1_700_000_000_000);
        original.events = vec![
            crate::TimedEvent {
                t: 0,
                event: Event::Touch {
                    x: 0.1,
                    y: 0.2,
                    down: true,
                },
            },
            crate::TimedEvent {
                t: 0,
                event: Event::MappedTouch {
                    id: 3,
                    x: 0.8,
                    y: 0.7,
                    down: true,
                },
            },
            crate::TimedEvent {
                t: 12,
                event: Event::Touch {
                    x: 0.3,
                    y: 0.4,
                    down: true,
                },
            },
            crate::TimedEvent {
                t: 18,
                event: Event::MappedTouch {
                    id: 3,
                    x: 0.6,
                    y: 0.5,
                    down: false,
                },
            },
            crate::TimedEvent {
                t: 18,
                event: Event::Touch {
                    x: 0.3,
                    y: 0.4,
                    down: false,
                },
            },
        ];
        let mut legacy = serde_json::to_value(&original).expect("encode");
        legacy["version"] = serde_json::json!(2);
        let bytes = serde_json::to_vec(&legacy).expect("legacy JSON");
        let path = dir.path().join("legacy.json");
        std::fs::write(&path, &bytes).expect("write source");
        let imported = store.import(&path).expect("import");
        assert_eq!(imported.events, original.events);
        assert_eq!(
            store.load(&imported.id).expect("load v3").events,
            original.events
        );
        assert_eq!(std::fs::read(path).expect("read source"), bytes);
        let stored: serde_json::Value = serde_json::from_slice(
            &std::fs::read(store.path_for(&imported.id)).expect("read stored"),
        )
        .expect("JSON");
        assert!(stored.get("actions").is_some());
        assert!(stored.get("events").is_none());
    }

    #[test]
    fn unknown_action_fields_are_not_silently_discarded() {
        let (_dir, store) = store();
        let script = script("unknown fields", 1_700_000_000_000);
        store.save(&script).expect("save");
        let path = store.path_for(&script.id);
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
        value["actions"][0]["future_field"] = serde_json::json!(42);
        let bytes = serde_json::to_vec(&value).expect("json");
        std::fs::write(&path, &bytes).expect("write");
        assert!(store.load(&script.id).is_err());
        assert!(store.save(&script).is_err());
        assert_eq!(std::fs::read(path).expect("read"), bytes);
    }

    #[test]
    fn list_sorts_newest_first_and_skips_corrupt_files() {
        let (_dir, store) = store();
        let older = script("older", 1_700_000_000_000);
        let newer = script("newer", 1_700_000_100_000);
        store.save(&older).expect("save older");
        store.save(&newer).expect("save newer");
        std::fs::write(store.dir().join("broken.json"), b"{not json").expect("write corrupt");
        std::fs::write(store.dir().join("foreign.json"), b"{\"hi\":1}").expect("write foreign");
        std::fs::write(store.dir().join("notes.txt"), b"not a script").expect("write text");
        let summaries = store.list().expect("list");
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].name, "newer");
        assert_eq!(summaries[1].name, "older");
    }

    #[test]
    fn missing_library_lists_empty_and_load_is_not_found() {
        let (_dir, store) = store();
        assert!(store.list().expect("list").is_empty());
        let error = store.load(&Uuid::new_v4()).expect_err("missing script");
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    #[test]
    fn rename_updates_the_file_and_delete_removes_it() {
        let (_dir, store) = store();
        let script = script("before", 1_700_000_000_000);
        store.save(&script).expect("save");
        let renamed = store.rename(&script.id, "after").expect("rename");
        assert_eq!(renamed.name, "after");
        assert_eq!(store.load(&script.id).expect("load").name, "after");

        store.delete(&script.id).expect("delete");
        let error = store.load(&script.id).expect_err("deleted");
        assert_eq!(error.code, ErrorCode::NotFound);
        let error = store.delete(&script.id).expect_err("deleted twice");
        assert_eq!(error.code, ErrorCode::NotFound);

        let error = store.rename(&script.id, "again").expect_err("missing");
        assert_eq!(error.code, ErrorCode::NotFound);
        let error = store.rename(&Uuid::new_v4(), "  ").expect_err("empty name");
        assert_eq!(error.code, ErrorCode::Usage);
    }

    #[test]
    fn save_validates_before_touching_the_library() {
        let (_dir, store) = store();
        let mut invalid = script("invalid", 1_700_000_000_000);
        invalid.version = VERSION + 1;
        let error = store.save(&invalid).expect_err("invalid script");
        assert_eq!(error.code, ErrorCode::Usage);
        assert!(store.list().expect("list").is_empty());
    }

    #[test]
    fn future_version_is_reported_before_body_decoding_and_never_overwritten() {
        let (_dir, store) = store();
        let current = script("current", 1_700_000_000_000);
        std::fs::create_dir_all(store.dir()).expect("create library");
        let path = store.path_for(&current.id);
        let future = format!(
            r#"{{"format":"miko.operation-recording","version":{},"events":[{{"type":"future_action"}}]}}"#,
            VERSION + 1
        );
        std::fs::write(&path, &future).expect("write future script");

        let error = store.load(&current.id).expect_err("future load");
        assert!(error.message.contains("upgrade"), "{}", error.message);
        let error = store.save(&current).expect_err("future overwrite");
        assert!(error.message.contains("upgrade"), "{}", error.message);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read original"),
            future
        );
        let error = store.import(&path).expect_err("future import");
        assert!(error.message.contains("upgrade"), "{}", error.message);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read original"),
            future
        );
    }

    #[test]
    fn import_copies_under_a_new_id_and_export_reproduces_it() {
        let (_source_dir, source_store) = store();
        let original = script("imported", 1_700_000_000_000);
        source_store.save(&original).expect("save source");
        let export_path = source_store.dir().join("exported.json");
        source_store
            .export(&original.id, &export_path)
            .expect("export");

        let (_dir, store) = store();
        let imported = store.import(&export_path).expect("import");
        assert_ne!(imported.id, original.id);
        assert_eq!(imported.name, original.name);
        assert_eq!(imported.created_at, original.created_at);
        assert_eq!(imported.events, original.events);
        assert_eq!(store.load(&imported.id).expect("load imported"), imported);
        assert_eq!(store.list().expect("list").len(), 1);

        // Exporting the imported copy and importing it again yields another id.
        let second_path = store.dir().join("second.json");
        store
            .export(&imported.id, &second_path)
            .expect("export imported");
        let reimported = store.import(&second_path).expect("re-import");
        assert_ne!(reimported.id, imported.id);
        assert_eq!(reimported.events, imported.events);
    }

    #[test]
    fn import_and_export_errors_use_usage_and_not_found() {
        let (_dir, store) = store();
        let missing = store
            .import(store.dir().join("nope.json"))
            .expect_err("missing import");
        assert_eq!(missing.code, ErrorCode::NotFound);

        let broken = TempDir::new().expect("temp dir");
        let broken_path = broken.path().join("broken.json");
        std::fs::write(&broken_path, b"{not json").expect("write");
        let error = store.import(&broken_path).expect_err("bad import");
        assert_eq!(error.code, ErrorCode::Usage);

        let invalid = TempDir::new().expect("temp dir");
        let invalid_path = invalid.path().join("invalid.json");
        let mut bad = script("bad", 1_700_000_000_000);
        bad.events.clear();
        std::fs::write(&invalid_path, serde_json::to_vec(&bad).expect("encode")).expect("write");
        let error = store.import(&invalid_path).expect_err("invalid import");
        assert_eq!(error.code, ErrorCode::Usage);

        let oversized_dir = TempDir::new().expect("temp dir");
        let oversized_path = oversized_dir.path().join("oversized.json");
        let file = std::fs::File::create(&oversized_path).expect("create sparse file");
        file.set_len(MAX_SCRIPT_BYTES + 1)
            .expect("size sparse file");
        drop(file);
        let error = store.import(&oversized_path).expect_err("oversized import");
        assert_eq!(error.code, ErrorCode::Usage);
        assert!(error.message.contains("larger"), "{}", error.message);

        let error = store
            .export(&Uuid::new_v4(), invalid.path().join("out.json"))
            .expect_err("missing export id");
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    #[test]
    fn infinite_playback_round_trips_through_the_store() {
        let (_dir, store) = store();
        let mut script = script("playback", 1_700_000_000_000);
        script.playback = PlaybackOptions {
            mode: PlaybackMode::Infinite,
            count: 0,
            duration_ms: 0,
            interval_ms: 250,
            speed: 2.0,
        };
        script.events = vec![crate::model::TimedEvent {
            t: 0,
            event: Event::ToolKey {
                key: "back".to_owned(),
            },
        }];
        store.save(&script).expect("save");
        let loaded = store.load(&script.id).expect("load");
        assert_eq!(loaded.playback.mode, PlaybackMode::Infinite);
        assert!((loaded.playback.speed - 2.0).abs() < f64::EPSILON);
        assert_eq!(loaded.playback.interval_ms, 250);
    }
}

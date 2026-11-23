//! Input synchronizer leader/follower state for the player bridge (D45).
//!
//! A leader bridge emits every whitelisted player input as one stderr line:
//! `MIKO_SYNC:{"seq":N,"event":{...}}`.
//! A follower bridge receives `sync_mode` and `sync_input` stdin commands,
//! adapts coordinates to its own screen via `miko_sync`, and injects events
//! through `script::execute_event` (sharing held-input release semantics).

use miko_automation::Event;
use miko_sync::{Adaptation, DisplaySize, SeqCheck, SeqTracker, SyncOptions, forward};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;

use crate::Command;
use crate::script::{event_from_command, is_direct_input};

/// Every leader input line on stderr uses this prefix.
pub const SYNC_PREFIX: &str = "MIKO_SYNC:";
/// Status/error notifications for sync mode transitions.
pub const SYNC_STATUS_PREFIX: &str = "MIKO_SYNC_STATUS:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncRole {
    #[default]
    Off,
    Leader,
    Follower,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum InputId {
    Touch,
    Mapped(i32),
    Key(String),
}

fn held_event(event: &Event) -> Option<(InputId, bool)> {
    match event {
        Event::Touch { down, .. } => Some((InputId::Touch, *down)),
        Event::MappedTouch { id, down, .. } => Some((InputId::Mapped(*id), *down)),
        Event::Key { key, down } => Some((InputId::Key(key.clone()), *down)),
        _ => None,
    }
}

/// A remote gesture rejected before its first input must be dropped through
/// its final release, otherwise a later move could begin a partial gesture.
#[derive(Debug, Default)]
pub(crate) struct RejectedRemote {
    held: HashSet<InputId>,
}
impl RejectedRemote {
    pub fn reject(&mut self, event: &Event) {
        if let Some((id, true)) = held_event(event) {
            self.held.insert(id);
        }
    }
    pub fn should_skip(&mut self, event: &Event) -> bool {
        if self.held.is_empty() {
            return false;
        }
        if let Some((id, down)) = held_event(event) {
            if down {
                self.held.insert(id);
            } else {
                self.held.remove(&id);
            }
        }
        true
    }
    pub fn clear(&mut self) {
        self.held.clear();
    }
}

/// Serial input owner for an unlocked follower. Once local input interrupts a
/// forwarded gesture, the rest of that remote gesture is discarded through its
/// last release; an incoming move must never recreate a released touch.
#[derive(Debug, Default)]
pub(crate) struct FollowerInputArbiter {
    local: HashSet<InputId>,
    remote: HashSet<InputId>,
    remote_owned: bool,
    suppress_remote: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LocalDecision {
    pub accept: bool,
    pub release_remote: bool,
}

impl FollowerInputArbiter {
    pub fn local(&mut self, command: &Command) -> LocalDecision {
        let event = event_from_command(command);
        let held = event.as_ref().and_then(held_event);
        if let Some((id, false)) = &held
            && !self.local.contains(id)
        {
            return LocalDecision {
                accept: false,
                release_remote: false,
            };
        }
        let release_remote = self.remote_owned && crate::script::is_manual_takeover(command);
        if release_remote {
            self.remote_owned = false;
            self.suppress_remote = !self.remote.is_empty();
        }
        if let Some((id, down)) = held {
            if down {
                self.local.insert(id);
            } else {
                self.local.remove(&id);
            }
        }
        LocalDecision {
            accept: true,
            release_remote,
        }
    }

    pub fn remote(&mut self, event: &Event) -> bool {
        if let Some((id, down)) = held_event(event) {
            if down {
                self.remote.insert(id);
            } else {
                self.remote.remove(&id);
            }
        }
        if !self.local.is_empty() {
            self.suppress_remote = !self.remote.is_empty();
            return false;
        }
        if self.suppress_remote {
            if self.remote.is_empty() {
                self.suppress_remote = false;
            }
            return false;
        }
        self.remote_owned = !self.remote.is_empty();
        true
    }

    /// A leader release must not release an independently held local input.
    pub fn remote_release(&mut self) -> bool {
        let release = self.remote_owned;
        self.remote.clear();
        self.remote_owned = false;
        self.suppress_remote = false;
        release
    }
}

#[derive(Debug)]
pub struct BridgeSync {
    role: SyncRole,
    lock: bool,
    leader: Option<String>,
    leader_display: Option<DisplaySize>,
    follower_display: DisplaySize,
    options: SyncOptions,
    next_seq: u64,
    seq_tracker: SeqTracker,
}

impl BridgeSync {
    pub fn new(display_width: u32, display_height: u32) -> Self {
        Self {
            role: SyncRole::Off,
            lock: true,
            leader: None,
            leader_display: None,
            follower_display: DisplaySize {
                width: display_width,
                height: display_height,
            },
            options: SyncOptions::default(),
            next_seq: 0,
            seq_tracker: SeqTracker::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.role != SyncRole::Off
    }

    pub fn is_follower(&self) -> bool {
        self.role == SyncRole::Follower
    }

    pub fn is_locked_follower(&self) -> bool {
        self.role == SyncRole::Follower && self.lock
    }

    pub fn is_unlocked_follower(&self) -> bool {
        self.role == SyncRole::Follower && !self.lock
    }

    /// True when direct UI input must be ignored because this bridge is a
    /// locked follower.
    pub fn ignores_user_input(&self, command: &Command) -> bool {
        self.is_locked_follower()
            && (is_direct_input(command)
                || matches!(
                    command,
                    Command::ToolKey { .. } | Command::MappingHold { active: true, .. }
                ))
    }

    /// Applies a `sync_mode` command. Returns `true` if held inputs should be
    /// released (e.g. leaving active sync, switching roles, or switching leader).
    #[allow(clippy::too_many_arguments)]
    pub fn set_mode(
        &mut self,
        role: SyncRole,
        lock: bool,
        leader: Option<String>,
        leader_display: Option<DisplaySize>,
        adaptation: Adaptation,
        sync_keyboard: bool,
        sync_text: bool,
        sync_tool_keys: bool,
    ) -> bool {
        let role_changed = self.role != role;
        let leader_changed =
            role == SyncRole::Follower && leader.is_some() && self.leader != leader;
        let lock_changed = role == SyncRole::Follower && lock != self.lock;
        let keyboard_filter_changed = role == SyncRole::Follower
            && self.role == SyncRole::Follower
            && self.options.sync_keyboard != sync_keyboard;
        self.role = role;
        self.lock = lock;
        self.leader = if role == SyncRole::Off { None } else { leader };
        self.leader_display = leader_display;
        self.options = SyncOptions {
            adaptation,
            sync_keyboard,
            sync_text,
            sync_tool_keys,
            lock_followers: lock,
            jitter_ms: 0,
        };
        if role == SyncRole::Off || (role == SyncRole::Leader && role_changed) {
            self.next_seq = 0;
        }
        if role_changed || leader_changed {
            self.seq_tracker = SeqTracker::new();
        }
        role_changed || leader_changed || lock_changed || keyboard_filter_changed
    }

    /// Formats a `MIKO_SYNC:...` stderr line if this bridge is the leader and
    /// `command` is a valid whitelisted input.
    pub fn format_leader_command(&mut self, command: &Command) -> Option<String> {
        if self.role != SyncRole::Leader {
            return None;
        }
        let event = event_from_command(command)?;
        self.format_leader_event(&event)
    }

    /// Formats a `MIKO_SYNC:...` stderr line for a tool key if leading.
    pub fn format_leader_tool_key(&mut self, key: &str) -> Option<String> {
        if self.role != SyncRole::Leader {
            return None;
        }
        let event = Event::ToolKey {
            key: key.to_owned(),
        };
        self.format_leader_event(&event)
    }

    pub fn format_leader_event(&mut self, event: &Event) -> Option<String> {
        if self.role != SyncRole::Leader || !event.fits_format() {
            return None;
        }
        self.next_seq = self.next_seq.saturating_add(1);
        Some(format!(
            "{SYNC_PREFIX}{}",
            json!({ "seq": self.next_seq, "event": event })
        ))
    }

    /// Emits `MIKO_SYNC:...` to stderr for a pre-built event (such as script
    /// playback on the leader). Does nothing unless this bridge is the leader.
    pub fn emit_leader_event(&mut self, event: &Event) {
        if let Some(line) = self.format_leader_event(event) {
            eprintln!("{line}");
        }
    }

    /// Emits a `MIKO_SYNC_STATUS:{"state":"release"}` line when leading so
    /// followers also release held inputs when leader playback stops/pauses.
    pub fn emit_leader_release(&self) {
        if self.role == SyncRole::Leader {
            eprintln!("{SYNC_STATUS_PREFIX}{}", json!({ "state": "release" }));
        }
    }

    /// Emits `MIKO_SYNC:...` to stderr when leading.
    pub fn emit_leader_command(&mut self, command: &Command) {
        if let Some(line) = self.format_leader_command(command) {
            eprintln!("{line}");
        }
    }

    /// Emits `MIKO_SYNC:...` for a tool key when leading.
    pub fn emit_leader_tool_key(&mut self, key: &str) {
        if let Some(line) = self.format_leader_tool_key(key) {
            eprintln!("{line}");
        }
    }

    /// Validates, checks ordering, and adapts an incoming `sync_input` event
    /// for injection on a follower. Never re-emits as leader output.
    pub fn accept_follower_input(
        &mut self,
        seq: Option<u64>,
        event: &Event,
        rotation: u32,
    ) -> Option<Event> {
        if self.role != SyncRole::Follower {
            return None;
        }
        if let Some(seq) = seq
            && matches!(self.seq_tracker.observe(seq), SeqCheck::Stale)
        {
            return None;
        }
        let follower_display = if rotation.is_multiple_of(2) {
            self.follower_display
        } else {
            DisplaySize {
                width: self.follower_display.height,
                height: self.follower_display.width,
            }
        };
        forward(
            event,
            &self.options,
            self.leader_display,
            Some(follower_display),
        )
    }
}

pub fn notify_sync_busy(message: &str) {
    eprintln!(
        "{SYNC_STATUS_PREFIX}{}",
        json!({ "state": "busy", "code": "script.busy", "message": message })
    );
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn touch(down: bool) -> Event {
        Event::Touch {
            x: 0.5,
            y: 0.5,
            down,
        }
    }
    fn local_touch(down: bool) -> Command {
        Command::Touch {
            x: 0.2,
            y: 0.2,
            down,
        }
    }

    #[test]
    fn local_takeover_releases_remote_and_discards_its_remaining_gesture() {
        let mut arbiter = FollowerInputArbiter::default();
        assert!(arbiter.remote(&touch(true)));
        assert_eq!(
            arbiter.local(&local_touch(true)),
            LocalDecision {
                accept: true,
                release_remote: true
            }
        );
        assert!(
            !arbiter.remote(&touch(true)),
            "a remote move cannot re-press during local input"
        );
        assert_eq!(
            arbiter.local(&local_touch(false)),
            LocalDecision {
                accept: true,
                release_remote: false
            }
        );
        assert!(
            !arbiter.remote(&touch(true)),
            "remote input stays suppressed until its release"
        );
        assert!(!arbiter.remote(&touch(false)));
        assert!(
            arbiter.remote(&touch(true)),
            "a new remote gesture may start after neutral"
        );
    }

    #[test]
    fn rejected_remote_gesture_is_dropped_until_every_contact_is_up() {
        let mut rejected = RejectedRemote::default();
        let mapped = |down| Event::MappedTouch {
            id: 3,
            x: 0.2,
            y: 0.3,
            down,
        };
        rejected.reject(&touch(true));
        assert!(rejected.should_skip(&touch(true)));
        assert!(rejected.should_skip(&mapped(true)));
        assert!(rejected.should_skip(&touch(false)));
        assert!(rejected.should_skip(&mapped(false)));
        assert!(!rejected.should_skip(&touch(true)));
    }

    #[test]
    fn remote_gesture_can_resume_after_it_ends_during_local_control() {
        let mut arbiter = FollowerInputArbiter::default();
        assert!(arbiter.remote(&touch(true)));
        assert!(arbiter.local(&local_touch(true)).release_remote);
        assert!(!arbiter.remote(&touch(true)));
        assert!(!arbiter.remote(&touch(false)));
        assert!(arbiter.local(&local_touch(false)).accept);
        assert!(arbiter.remote(&touch(true)));
        assert!(arbiter.remote(&touch(false)));
    }

    #[test]
    fn remote_release_and_stale_local_up_do_not_release_the_other_owner() {
        let mut arbiter = FollowerInputArbiter::default();
        assert!(arbiter.remote(&touch(true)));
        assert!(!arbiter.local(&local_touch(false)).accept);
        assert!(arbiter.local(&local_touch(true)).release_remote);
        assert!(
            !arbiter.remote_release(),
            "leader release cannot lift local touch"
        );
        assert!(arbiter.local(&local_touch(false)).accept);
        assert!(arbiter.remote(&touch(true)));
        assert!(
            arbiter.remote_release(),
            "remote-held input still needs release"
        );
    }

    #[test]
    fn suppression_waits_for_all_remote_contacts_and_keys() {
        let mut arbiter = FollowerInputArbiter::default();
        let mapped = |down| Event::MappedTouch {
            id: 3,
            x: 0.2,
            y: 0.2,
            down,
        };
        let key = |down| Event::Key {
            key: "A".into(),
            down,
        };
        assert!(arbiter.remote(&touch(true)));
        assert!(arbiter.remote(&mapped(true)));
        assert!(arbiter.remote(&key(true)));
        assert!(arbiter.local(&local_touch(true)).release_remote);
        assert!(arbiter.local(&local_touch(false)).accept);
        assert!(!arbiter.remote(&touch(false)));
        assert!(!arbiter.remote(&mapped(false)));
        assert!(!arbiter.remote(&key(false)));
        assert!(arbiter.remote(&touch(true)));
    }

    #[test]
    fn unlocking_or_disabling_keyboard_releases_the_previous_input() {
        let mut sync = BridgeSync::new(1080, 2400);
        assert!(sync.set_mode(
            SyncRole::Follower,
            true,
            Some("L".into()),
            None,
            Adaptation::Stretch,
            true,
            true,
            true
        ));
        assert!(sync.set_mode(
            SyncRole::Follower,
            false,
            Some("L".into()),
            None,
            Adaptation::Stretch,
            true,
            true,
            true
        ));
        assert!(sync.set_mode(
            SyncRole::Follower,
            false,
            Some("L".into()),
            None,
            Adaptation::Stretch,
            false,
            true,
            true
        ));
    }

    #[test]
    fn leader_emits_monotonic_seq_lines_and_skips_non_input() {
        let mut sync = BridgeSync::new(1080, 2400);
        sync.set_mode(
            SyncRole::Leader,
            true,
            Some("L".into()),
            None,
            Adaptation::Stretch,
            true,
            true,
            true,
        );
        let first = sync
            .format_leader_command(&Command::Touch {
                x: 0.25,
                y: 0.5,
                down: true,
            })
            .unwrap();
        assert!(first.starts_with(SYNC_PREFIX));
        let v1: serde_json::Value =
            serde_json::from_str(first.strip_prefix(SYNC_PREFIX).unwrap()).unwrap();
        assert_eq!(v1["seq"], 1);
        assert_eq!(v1["event"]["type"], "touch");

        let second = sync.format_leader_tool_key("back").unwrap();
        let v2: serde_json::Value =
            serde_json::from_str(second.strip_prefix(SYNC_PREFIX).unwrap()).unwrap();
        assert_eq!(v2["seq"], 2);
        assert_eq!(v2["event"], json!({ "type": "tool_key", "key": "back" }));

        assert!(
            sync.format_leader_command(&Command::StopRecording)
                .is_none()
        );
        // Line stays well under the 4 MB JSON/NDJSON line limit.
        assert!(first.len() < 1024);
    }

    #[test]
    fn follower_never_echoes_and_ignores_locked_user_input() {
        let mut sync = BridgeSync::new(2000, 1000);
        assert!(sync.set_mode(
            SyncRole::Follower,
            true,
            Some("L".into()),
            Some(DisplaySize {
                width: 1000,
                height: 2000
            }),
            Adaptation::Letterbox,
            true,
            true,
            true,
        ));
        assert!(sync.ignores_user_input(&Command::Touch {
            x: 0.5,
            y: 0.5,
            down: true
        }));
        // Follower never emits leader lines (loop guard).
        assert!(
            sync.format_leader_command(&Command::Touch {
                x: 0.5,
                y: 0.5,
                down: true
            })
            .is_none()
        );

        let adapted = sync
            .accept_follower_input(
                Some(1),
                &Event::Touch {
                    x: 0.0,
                    y: 0.5,
                    down: true,
                },
                0,
            )
            .unwrap();
        match adapted {
            Event::Touch { x, y, down } => {
                assert!((x - 0.375).abs() < 1e-9);
                assert!((y - 0.5).abs() < 1e-9);
                assert!(down);
            }
            other => panic!("{other:?}"),
        }
        // Stale seq is dropped.
        assert!(
            sync.accept_follower_input(
                Some(1),
                &Event::Touch {
                    x: 0.5,
                    y: 0.5,
                    down: false
                },
                0,
            )
            .is_none()
        );
    }

    #[test]
    fn turning_sync_off_requests_release() {
        let mut sync = BridgeSync::new(1080, 2400);
        assert!(sync.set_mode(
            SyncRole::Follower,
            true,
            Some("L".into()),
            None,
            Adaptation::Stretch,
            true,
            true,
            true
        ));
        assert!(sync.set_mode(
            SyncRole::Off,
            true,
            None,
            None,
            Adaptation::Stretch,
            true,
            true,
            true
        ));
        assert!(!sync.set_mode(
            SyncRole::Off,
            true,
            None,
            None,
            Adaptation::Stretch,
            true,
            true,
            true
        ));
    }

    #[test]
    fn unlocked_follower_allows_direct_user_input() {
        let mut sync = BridgeSync::new(1080, 2400);
        sync.set_mode(
            SyncRole::Follower,
            false,
            Some("L".into()),
            None,
            Adaptation::Stretch,
            true,
            true,
            true,
        );
        assert!(!sync.ignores_user_input(&Command::Touch {
            x: 0.5,
            y: 0.5,
            down: true
        }));
        assert!(
            sync.set_mode(
                SyncRole::Follower,
                true,
                Some("L".into()),
                None,
                Adaptation::Stretch,
                true,
                true,
                true
            ),
            "locking a follower must release its direct input"
        );
        assert!(sync.ignores_user_input(&Command::Touch {
            x: 0.5,
            y: 0.5,
            down: true
        }));
        assert!(sync.ignores_user_input(&Command::ToolKey { key: "back".into() }));
        assert!(sync.ignores_user_input(&Command::MappingHold {
            id: 1,
            active: true
        }));
        assert!(!sync.ignores_user_input(&Command::MappingHold {
            id: 1,
            active: false
        }));
    }

    #[test]
    fn leader_change_resets_seq_and_requests_release_on_remaining_follower() {
        let mut sync = BridgeSync::new(1080, 2400);
        assert!(sync.set_mode(
            SyncRole::Follower,
            true,
            Some("A".into()),
            None,
            Adaptation::Stretch,
            true,
            true,
            true,
        ));
        assert!(
            sync.accept_follower_input(
                Some(20),
                &Event::Touch {
                    x: 0.5,
                    y: 0.5,
                    down: true,
                },
                0,
            )
            .is_some()
        );

        // Same leader update does not release or reset seq.
        assert!(!sync.set_mode(
            SyncRole::Follower,
            true,
            Some("A".into()),
            None,
            Adaptation::Stretch,
            true,
            true,
            true,
        ));
        assert!(
            sync.accept_follower_input(
                Some(1),
                &Event::Touch {
                    x: 0.5,
                    y: 0.5,
                    down: true,
                },
                0,
            )
            .is_none()
        );

        // Switching to leader B must release held inputs and accept seq=1.
        assert!(sync.set_mode(
            SyncRole::Follower,
            true,
            Some("B".into()),
            None,
            Adaptation::Stretch,
            true,
            true,
            true,
        ));
        assert!(
            sync.accept_follower_input(
                Some(1),
                &Event::Touch {
                    x: 0.2,
                    y: 0.3,
                    down: true,
                },
                0,
            )
            .is_some()
        );
    }

    #[test]
    fn rotated_follower_swaps_display_dimensions_for_letterbox() {
        let mut sync = BridgeSync::new(1000, 2000);
        sync.set_mode(
            SyncRole::Follower,
            true,
            Some("L".into()),
            Some(DisplaySize {
                width: 1000,
                height: 2000,
            }),
            Adaptation::Letterbox,
            true,
            true,
            true,
        );
        // Quarter turn 1 makes the follower 2000x1000 landscape, so portrait
        // x=0.0 maps inside the horizontally centred content at 0.375.
        let adapted = sync
            .accept_follower_input(
                Some(1),
                &Event::Touch {
                    x: 0.0,
                    y: 0.5,
                    down: true,
                },
                1,
            )
            .unwrap();
        match adapted {
            Event::Touch { x, y, .. } => {
                assert!((x - 0.375).abs() < 1e-9);
                assert!((y - 0.5).abs() < 1e-9);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn leader_event_emits_only_when_leading_and_within_format() {
        let mut sync = BridgeSync::new(1080, 2400);
        let touch = Event::Touch {
            x: 0.4,
            y: 0.6,
            down: true,
        };
        assert!(sync.format_leader_event(&touch).is_none());

        sync.set_mode(
            SyncRole::Follower,
            true,
            Some("L".into()),
            None,
            Adaptation::Stretch,
            true,
            true,
            true,
        );
        assert!(sync.is_follower());
        assert!(sync.format_leader_event(&touch).is_none());

        sync.set_mode(
            SyncRole::Leader,
            true,
            Some("L".into()),
            None,
            Adaptation::Stretch,
            true,
            true,
            true,
        );
        assert!(!sync.is_follower());
        let line = sync.format_leader_event(&touch).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(line.strip_prefix(SYNC_PREFIX).unwrap()).unwrap();
        assert_eq!(v["seq"], 1);
        assert_eq!(v["event"]["type"], "touch");
        assert!(
            sync.format_leader_event(&Event::Touch {
                x: 1.5,
                y: 0.5,
                down: true,
            })
            .is_none()
        );
    }
}

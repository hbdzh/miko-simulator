//! Lossless conversion between legacy event timelines and editable actions.
//!
//! Times and stable ordinals are retained for every sample: two concurrent
//! contacts and keyboard chords must replay in precisely their original order.

use std::collections::HashMap;

use miko_core::{MikoError, MikoResult};
use serde::{Deserialize, Serialize};

use crate::{Event, Script, TimedEvent, validate};

/// One sample within a gesture or key action. `order` disambiguates events
/// that have the same timestamp, including events belonging to other actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionSample<T> {
    pub t: u64,
    pub order: usize,
    pub value: T,
}

/// Position and state of a contact at one point in a gesture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactSample {
    pub x: f64,
    pub y: f64,
    pub down: bool,
}

/// A semantic action; overlapping gestures and keys are represented separately.
/// An incomplete legacy press or release is retained instead of silently repaired.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Gesture {
        id: Option<i32>,
        samples: Vec<ActionSample<ContactSample>>,
    },
    Key {
        key: String,
        samples: Vec<ActionSample<bool>>,
    },
    Text {
        t: u64,
        order: usize,
        text: String,
    },
    Paste {
        t: u64,
        order: usize,
        text: String,
    },
    ToolKey {
        t: u64,
        order: usize,
        key: String,
    },
    ScriptCall {
        t: u64,
        order: usize,
        script: uuid::Uuid,
    },
}

impl Action {
    pub(crate) fn event_keys(&self) -> Vec<(u64, usize)> {
        match self {
            Self::Gesture { samples, .. } => samples.iter().map(|s| (s.t, s.order)).collect(),
            Self::Key { samples, .. } => samples.iter().map(|s| (s.t, s.order)).collect(),
            Self::Text { t, order, .. }
            | Self::Paste { t, order, .. }
            | Self::ToolKey { t, order, .. }
            | Self::ScriptCall { t, order, .. } => vec![(*t, *order)],
        }
    }
    pub(crate) fn max_order(&self) -> usize {
        match self {
            Self::Gesture { samples, .. } => samples.iter().map(|s| s.order).max().unwrap_or(0),
            Self::Key { samples, .. } => samples.iter().map(|s| s.order).max().unwrap_or(0),
            Self::Text { order, .. }
            | Self::Paste { order, .. }
            | Self::ToolKey { order, .. }
            | Self::ScriptCall { order, .. } => *order,
        }
    }
    /// Earliest and latest sample time in this action.
    pub fn time_bounds(&self) -> Option<(u64, u64)> {
        fn bounds(times: impl Iterator<Item = u64>) -> Option<(u64, u64)> {
            times.fold(None, |range, t| {
                Some(match range {
                    Some((min, max)) => (u64::min(min, t), u64::max(max, t)),
                    None => (t, t),
                })
            })
        }
        match self {
            Self::Gesture { samples, .. } => bounds(samples.iter().map(|s| s.t)),
            Self::Key { samples, .. } => bounds(samples.iter().map(|s| s.t)),
            Self::Text { t, .. }
            | Self::Paste { t, .. }
            | Self::ToolKey { t, .. }
            | Self::ScriptCall { t, .. } => Some((*t, *t)),
        }
    }

    pub(crate) fn shift(&mut self, old_start: u64, new_start: u64) -> MikoResult<()> {
        fn shifted(t: &mut u64, old: u64, new: u64) -> MikoResult<()> {
            *t = t
                .checked_sub(old)
                .and_then(|offset| new.checked_add(offset))
                .ok_or_else(|| MikoError::usage("action time is out of range"))?;
            Ok(())
        }
        match self {
            Self::Gesture { samples, .. } => {
                for sample in samples {
                    shifted(&mut sample.t, old_start, new_start)?;
                }
            }
            Self::Key { samples, .. } => {
                for sample in samples {
                    shifted(&mut sample.t, old_start, new_start)?;
                }
            }
            Self::Text { t, .. }
            | Self::Paste { t, .. }
            | Self::ToolKey { t, .. }
            | Self::ScriptCall { t, .. } => shifted(t, old_start, new_start)?,
        }
        Ok(())
    }

    fn renumber(&mut self, orders: &HashMap<(u64, usize), usize>) {
        match self {
            Self::Gesture { samples, .. } => {
                for sample in samples {
                    if let Some(order) = orders.get(&(sample.t, sample.order)) {
                        sample.order = *order;
                    }
                }
            }
            Self::Key { samples, .. } => {
                for sample in samples {
                    if let Some(order) = orders.get(&(sample.t, sample.order)) {
                        sample.order = *order;
                    }
                }
            }
            Self::Text { t, order, .. }
            | Self::Paste { t, order, .. }
            | Self::ToolKey { t, order, .. }
            | Self::ScriptCall { t, order, .. } => {
                if let Some(next) = orders.get(&(*t, *order)) {
                    *order = *next;
                }
            }
        }
    }
}

/// Reorders overlapping actions as a unit, preserving their relative timing.
pub fn move_action(actions: &[Action], from: usize, to: usize) -> MikoResult<Vec<Action>> {
    if from >= actions.len() || to >= actions.len() {
        return Err(MikoError::usage("action not found"));
    }
    struct Group {
        first: usize,
        last: usize,
        start: u64,
        end: u64,
        wait: u64,
        actions: Vec<Action>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        let (start, end) = action
            .time_bounds()
            .ok_or_else(|| MikoError::usage("empty action"))?;
        if let Some(group) = groups.last_mut()
            && (start < group.end || start == group.start)
        {
            group.last = index;
            group.end = group.end.max(end);
            group.actions.push(action.clone());
            continue;
        }
        let prior = groups.last().map_or(0, |group| group.end);
        let wait = start
            .checked_sub(prior)
            .ok_or_else(|| MikoError::usage("actions are not ordered"))?;
        groups.push(Group {
            first: index,
            last: index,
            start,
            end,
            wait,
            actions: vec![action.clone()],
        });
    }
    let source = groups
        .iter()
        .position(|g| (g.first..=g.last).contains(&from))
        .ok_or_else(|| MikoError::usage("action not found"))?;
    let target = groups
        .iter()
        .position(|g| (g.first..=g.last).contains(&to))
        .ok_or_else(|| MikoError::usage("action not found"))?;
    if source == target {
        return Ok(actions.to_vec());
    }
    let group = groups.remove(source);
    groups.insert(target, group);
    let mut end = 0u64;
    let mut result = Vec::with_capacity(actions.len());
    let mut order = 0;
    for mut group in groups {
        let next = end
            .checked_add(group.wait)
            .ok_or_else(|| MikoError::usage("action time is out of range"))?;
        for action in &mut group.actions {
            action.shift(group.start, next)?;
        }
        let mut keys: Vec<_> = group.actions.iter().flat_map(Action::event_keys).collect();
        keys.sort_unstable();
        let mut orders = HashMap::new();
        for key in keys {
            orders.insert(key, order);
            order += 1;
        }
        for action in &mut group.actions {
            action.renumber(&orders);
        }
        end = next
            .checked_add(group.end - group.start)
            .ok_or_else(|| MikoError::usage("action time is out of range"))?;
        result.extend(group.actions);
    }
    Ok(result)
}

/// Translates a gesture and changes its duration, preserving its sampled path.
pub fn edit_gesture(
    actions: &[Action],
    index: usize,
    x: f64,
    y: f64,
    end_x: f64,
    end_y: f64,
    duration_ms: u64,
) -> MikoResult<Vec<Action>> {
    if [x, y, end_x, end_y]
        .into_iter()
        .any(|p| !p.is_finite() || !(0.0..=1.0).contains(&p))
    {
        return Err(MikoError::usage("coordinates must be between 0 and 1"));
    }
    let mut result = actions.to_vec();
    let (start, end) = result
        .get(index)
        .and_then(Action::time_bounds)
        .ok_or_else(|| MikoError::usage("action not found"))?;
    if duration_ms != end - start
        && result.iter().enumerate().any(|(i, action)| {
            i != index
                && action
                    .time_bounds()
                    .is_some_and(|(a, b)| a < end && b > start)
        })
    {
        return Err(MikoError::usage(
            "cannot edit duration of overlapping actions",
        ));
    }
    let Action::Gesture { samples, .. } = &mut result[index] else {
        return Err(MikoError::usage("action is not a gesture"));
    };
    let first = samples
        .first()
        .ok_or_else(|| MikoError::usage("empty gesture"))?
        .value
        .clone();
    let last = samples
        .last()
        .ok_or_else(|| MikoError::usage("empty gesture"))?
        .value
        .clone();
    let old_duration = end - start;
    if old_duration == 0 && duration_ms != 0 {
        return Err(MikoError::usage(
            "cannot change duration of an instantaneous gesture",
        ));
    }
    let new_end = start
        .checked_add(duration_ms)
        .ok_or_else(|| MikoError::usage("action time is out of range"))?;
    let count = samples.len();
    for (index, sample) in samples.iter_mut().enumerate() {
        let fraction = if old_duration == 0 {
            index as f64 / count.saturating_sub(1).max(1) as f64
        } else {
            (sample.t - start) as f64 / old_duration as f64
        };
        sample.value.x += x - first.x + (end_x - last.x - (x - first.x)) * fraction;
        sample.value.y += y - first.y + (end_y - last.y - (y - first.y)) * fraction;
        sample.t = if old_duration == 0 {
            start
        } else {
            start
                + ((sample.t - start) as u128 * u128::from(duration_ms) / u128::from(old_duration))
                    as u64
        };
    }
    for (i, action) in result.iter_mut().enumerate() {
        if i != index && action.time_bounds().is_some_and(|(a, _)| a >= end) {
            action.shift(end, new_end)?;
        }
    }
    Ok(result)
}

/// Adjusts the idle interval before an action without interrupting any
/// overlapping gesture or key. Actions starting together move together.
pub fn set_wait_before(script: &Script, index: usize, wait_ms: u64) -> MikoResult<Script> {
    let actions = set_action_wait(&actions_from_script(script)?, index, wait_ms)?;
    script_with_actions(script, &actions)
}

/// Adjusts an already aggregated action list without converting it back and forth.
pub fn set_action_wait(actions: &[Action], index: usize, wait_ms: u64) -> MikoResult<Vec<Action>> {
    let mut actions = actions.to_vec();
    let start = actions
        .get(index)
        .and_then(Action::time_bounds)
        .map(|bounds| bounds.0)
        .ok_or_else(|| MikoError::usage("action not found"))?;
    let mut previous_end = 0;
    for action in &actions {
        let (begin, end) = action
            .time_bounds()
            .ok_or_else(|| MikoError::usage("empty action"))?;
        if begin < start {
            if end > start {
                return Err(MikoError::usage(
                    "cannot change a wait inside overlapping actions",
                ));
            }
            previous_end = previous_end.max(end);
        }
    }
    let next = previous_end
        .checked_add(wait_ms)
        .ok_or_else(|| MikoError::usage("wait is too long"))?;
    for action in &mut actions {
        if action.time_bounds().is_some_and(|bounds| bounds.0 >= start) {
            action.shift(start, next)?;
        }
    }
    Ok(actions)
}

/// Deletes an action. A non-overlapping action's duration is removed from the
/// following timeline; overlapping input keeps its original timing.
pub fn delete_action(script: &Script, index: usize) -> MikoResult<Script> {
    let actions = remove_action(&actions_from_script(script)?, index)?;
    script_with_actions(script, &actions)
}

/// Deletes from an already aggregated action list.
pub fn remove_action(actions: &[Action], index: usize) -> MikoResult<Vec<Action>> {
    let mut actions = actions.to_vec();
    if index >= actions.len() {
        return Err(MikoError::usage("action not found"));
    }
    let (start, end) = actions[index]
        .time_bounds()
        .ok_or_else(|| MikoError::usage("empty action"))?;
    let overlaps = actions.iter().enumerate().any(|(other, action)| {
        other != index
            && action
                .time_bounds()
                .is_some_and(|(begin, finish)| begin < end && finish > start)
    });
    actions.remove(index);
    if !overlaps && end > start {
        for action in &mut actions {
            if action.time_bounds().is_some_and(|bounds| bounds.0 >= end) {
                action.shift(end, start)?;
            }
        }
    }
    Ok(actions)
}

#[derive(Debug, Hash, PartialEq, Eq)]
enum Held {
    Touch(Option<i32>),
    Key(String),
}

/// Converts a validated legacy recording without modifying it or its file.
pub fn actions_from_script(script: &Script) -> MikoResult<Vec<Action>> {
    validate(script)?;
    let mut actions = Vec::new();
    let mut active = HashMap::<Held, usize>::new();
    for (order, timed) in script.events.iter().enumerate() {
        match &timed.event {
            Event::Touch { x, y, down } => add_touch(
                &mut actions,
                &mut active,
                None,
                *x,
                *y,
                *down,
                timed.t,
                order,
            ),
            Event::MappedTouch { id, x, y, down } => add_touch(
                &mut actions,
                &mut active,
                Some(*id),
                *x,
                *y,
                *down,
                timed.t,
                order,
            ),
            Event::Key { key, down } => {
                let held = Held::Key(key.clone());
                let index = *active.entry(held).or_insert_with(|| {
                    actions.push(Action::Key {
                        key: key.clone(),
                        samples: Vec::new(),
                    });
                    actions.len() - 1
                });
                if let Some(Action::Key { samples, .. }) = actions.get_mut(index) {
                    samples.push(ActionSample {
                        t: timed.t,
                        order,
                        value: *down,
                    });
                }
                if !down {
                    active.remove(&Held::Key(key.clone()));
                }
            }
            Event::Text { text } => actions.push(Action::Text {
                t: timed.t,
                order,
                text: text.clone(),
            }),
            Event::Paste { text } => actions.push(Action::Paste {
                t: timed.t,
                order,
                text: text.clone(),
            }),
            Event::ToolKey { key } => actions.push(Action::ToolKey {
                t: timed.t,
                order,
                key: key.clone(),
            }),
            Event::ScriptCall { script } => actions.push(Action::ScriptCall {
                t: timed.t,
                order,
                script: *script,
            }),
        }
    }
    Ok(actions)
}

#[allow(clippy::too_many_arguments)]
fn add_touch(
    actions: &mut Vec<Action>,
    active: &mut HashMap<Held, usize>,
    id: Option<i32>,
    x: f64,
    y: f64,
    down: bool,
    t: u64,
    order: usize,
) {
    let index = *active.entry(Held::Touch(id)).or_insert_with(|| {
        actions.push(Action::Gesture {
            id,
            samples: Vec::new(),
        });
        actions.len() - 1
    });
    if let Some(Action::Gesture { samples, .. }) = actions.get_mut(index) {
        samples.push(ActionSample {
            t,
            order,
            value: ContactSample { x, y, down },
        });
    }
    if !down {
        active.remove(&Held::Touch(id));
    }
}

/// Builds a legacy playback timeline from actions, checking ordering and all
/// existing script limits before returning. The input script remains untouched.
pub fn script_with_actions(script: &Script, actions: &[Action]) -> MikoResult<Script> {
    let mut events = Vec::<(usize, TimedEvent)>::new();
    for action in actions {
        match action {
            Action::Gesture { id, samples } => {
                if samples.is_empty() {
                    return Err(MikoError::usage("gesture has no samples"));
                }
                for sample in samples {
                    let p = &sample.value;
                    let event = match id {
                        Some(id) => Event::MappedTouch {
                            id: *id,
                            x: p.x,
                            y: p.y,
                            down: p.down,
                        },
                        None => Event::Touch {
                            x: p.x,
                            y: p.y,
                            down: p.down,
                        },
                    };
                    events.push((sample.order, TimedEvent { t: sample.t, event }));
                }
            }
            Action::Key { key, samples } => {
                if samples.is_empty() {
                    return Err(MikoError::usage("key action has no samples"));
                }
                for sample in samples {
                    events.push((
                        sample.order,
                        TimedEvent {
                            t: sample.t,
                            event: Event::Key {
                                key: key.clone(),
                                down: sample.value,
                            },
                        },
                    ));
                }
            }
            Action::Text { t, order, text } => events.push((
                *order,
                TimedEvent {
                    t: *t,
                    event: Event::Text { text: text.clone() },
                },
            )),
            Action::Paste { t, order, text } => events.push((
                *order,
                TimedEvent {
                    t: *t,
                    event: Event::Paste { text: text.clone() },
                },
            )),
            Action::ToolKey { t, order, key } => events.push((
                *order,
                TimedEvent {
                    t: *t,
                    event: Event::ToolKey { key: key.clone() },
                },
            )),
            Action::ScriptCall { t, order, script } => events.push((
                *order,
                TimedEvent {
                    t: *t,
                    event: Event::ScriptCall { script: *script },
                },
            )),
        }
    }
    events.sort_by_key(|(order, event)| (event.t, *order));
    if events
        .windows(2)
        .any(|pair| pair[0].1.t == pair[1].1.t && pair[0].0 == pair[1].0)
    {
        return Err(MikoError::usage(
            "duplicate action sample order at the same timestamp",
        ));
    }
    let mut result = script.clone();
    result.events = events.into_iter().map(|(_, event)| event).collect();
    validate(&result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{fixture, timed};

    #[test]
    fn reorder_and_edit_preserve_durations_and_following_waits() {
        let mut script = fixture();
        script.events = vec![
            timed(
                10,
                Event::Touch {
                    x: 0.2,
                    y: 0.3,
                    down: true,
                },
            ),
            timed(
                30,
                Event::Touch {
                    x: 0.4,
                    y: 0.5,
                    down: false,
                },
            ),
            timed(
                50,
                Event::Key {
                    key: "A".into(),
                    down: true,
                },
            ),
            timed(
                60,
                Event::Key {
                    key: "A".into(),
                    down: false,
                },
            ),
        ];
        let actions = actions_from_script(&script).unwrap();
        let moved = move_action(&actions, 0, 1).unwrap();
        let replay = script_with_actions(&script, &moved).unwrap();
        assert!(matches!(replay.events[0].event, Event::Key { .. }));
        assert_eq!(
            replay.events.iter().map(|e| e.t).collect::<Vec<_>>(),
            [20, 30, 40, 60]
        );
        let edited = edit_gesture(&actions, 0, 0.1, 0.2, 0.6, 0.7, 40).unwrap();
        let replay = script_with_actions(&script, &edited).unwrap();
        assert_eq!(
            replay.events.iter().map(|e| e.t).collect::<Vec<_>>(),
            [10, 50, 70, 80]
        );
        assert!(
            matches!(replay.events[0].event, Event::Touch { x, y, .. } if (x-0.1).abs() < 1e-9 && (y-0.2).abs() < 1e-9)
        );
        assert!(
            matches!(replay.events[1].event, Event::Touch { x, y, .. } if (x-0.6).abs() < 1e-9 && (y-0.7).abs() < 1e-9)
        );
    }

    #[test]
    fn reorder_moves_overlapping_inputs_together() {
        let mut script = fixture();
        script.events = vec![
            timed(
                0,
                Event::Touch {
                    x: 0.2,
                    y: 0.3,
                    down: true,
                },
            ),
            timed(
                5,
                Event::Key {
                    key: "A".into(),
                    down: true,
                },
            ),
            timed(
                10,
                Event::Touch {
                    x: 0.2,
                    y: 0.3,
                    down: false,
                },
            ),
            timed(
                15,
                Event::Key {
                    key: "A".into(),
                    down: false,
                },
            ),
            timed(
                30,
                Event::Text {
                    text: "next".into(),
                },
            ),
        ];
        let actions = actions_from_script(&script).unwrap();
        let moved = move_action(&actions, 0, 2).unwrap();
        let replay = script_with_actions(&script, &moved).unwrap();
        assert!(matches!(replay.events[0].event, Event::Text { .. }));
        assert_eq!(
            replay.events[1..].iter().map(|e| e.t).collect::<Vec<_>>(),
            [15, 20, 25, 30]
        );
        assert_eq!(
            replay.events[1..]
                .iter()
                .map(|e| &e.event)
                .collect::<Vec<_>>(),
            script.events[..4]
                .iter()
                .map(|e| &e.event)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn overlapping_contacts_and_chords_round_trip_in_original_order() {
        let mut script = fixture();
        script.events = vec![
            timed(
                5,
                Event::Touch {
                    x: 0.1,
                    y: 0.2,
                    down: true,
                },
            ),
            timed(
                5,
                Event::MappedTouch {
                    id: 3,
                    x: 0.4,
                    y: 0.5,
                    down: true,
                },
            ),
            timed(
                5,
                Event::Key {
                    key: "A".into(),
                    down: true,
                },
            ),
            timed(
                10,
                Event::Touch {
                    x: 0.2,
                    y: 0.3,
                    down: true,
                },
            ),
            timed(
                10,
                Event::Text {
                    text: "hello".into(),
                },
            ),
            timed(
                20,
                Event::MappedTouch {
                    id: 3,
                    x: 0.5,
                    y: 0.6,
                    down: false,
                },
            ),
            timed(
                20,
                Event::Key {
                    key: "A".into(),
                    down: false,
                },
            ),
            timed(
                20,
                Event::Touch {
                    x: 0.2,
                    y: 0.3,
                    down: false,
                },
            ),
        ];
        let actions = actions_from_script(&script).expect("convert");
        assert_eq!(actions.len(), 4);
        assert_eq!(
            script_with_actions(&script, &actions).expect("rebuild"),
            script
        );
    }

    #[test]
    fn incomplete_legacy_inputs_are_preserved_and_invalid_edits_rejected() {
        let mut script = fixture();
        script.events = vec![
            timed(
                0,
                Event::Key {
                    key: "A".into(),
                    down: false,
                },
            ),
            timed(
                1,
                Event::Touch {
                    x: 0.2,
                    y: 0.3,
                    down: true,
                },
            ),
        ];
        let mut actions = actions_from_script(&script).expect("convert");
        assert_eq!(
            script_with_actions(&script, &actions).expect("rebuild"),
            script
        );
        if let Action::Gesture { samples, .. } = &mut actions[1] {
            samples[0].value.x = 2.0;
        }
        assert!(script_with_actions(&script, &actions).is_err());
        assert_eq!(
            script.events[1].event,
            Event::Touch {
                x: 0.2,
                y: 0.3,
                down: true
            }
        );
    }

    #[test]
    fn changing_wait_shifts_following_actions_without_changing_gestures() {
        let mut script = fixture();
        script.events = vec![
            timed(
                0,
                Event::Key {
                    key: "A".into(),
                    down: true,
                },
            ),
            timed(
                50,
                Event::Key {
                    key: "A".into(),
                    down: false,
                },
            ),
            timed(
                150,
                Event::Touch {
                    x: 0.1,
                    y: 0.2,
                    down: true,
                },
            ),
            timed(
                180,
                Event::Touch {
                    x: 0.3,
                    y: 0.4,
                    down: false,
                },
            ),
            timed(200, Event::Text { text: "x".into() }),
        ];
        let edited = set_wait_before(&script, 1, 300).expect("edit wait");
        assert_eq!(
            edited.events.iter().map(|e| e.t).collect::<Vec<_>>(),
            vec![0, 50, 350, 380, 400]
        );
        assert_eq!(
            edited.events.iter().map(|e| &e.event).collect::<Vec<_>>(),
            script.events.iter().map(|e| &e.event).collect::<Vec<_>>()
        );
        assert!(set_wait_before(&script, 1, crate::MAX_SCRIPT_MS).is_err());
    }

    #[test]
    fn changing_wait_inside_overlapping_inputs_is_rejected() {
        let mut script = fixture();
        script.events = vec![
            timed(
                0,
                Event::Key {
                    key: "A".into(),
                    down: true,
                },
            ),
            timed(
                10,
                Event::Touch {
                    x: 0.1,
                    y: 0.2,
                    down: true,
                },
            ),
            timed(
                20,
                Event::Touch {
                    x: 0.1,
                    y: 0.2,
                    down: false,
                },
            ),
            timed(
                30,
                Event::Key {
                    key: "A".into(),
                    down: false,
                },
            ),
        ];
        assert!(set_wait_before(&script, 1, 5).is_err());
        let edited = delete_action(&script, 1).expect("delete overlapping touch");
        assert_eq!(
            edited.events.iter().map(|e| e.t).collect::<Vec<_>>(),
            vec![0, 30]
        );
    }

    #[test]
    fn deleting_a_non_overlapping_action_compacts_its_duration() {
        let mut script = fixture();
        script.events = vec![
            timed(
                10,
                Event::Key {
                    key: "A".into(),
                    down: true,
                },
            ),
            timed(
                30,
                Event::Key {
                    key: "A".into(),
                    down: false,
                },
            ),
            timed(
                50,
                Event::Text {
                    text: "next".into(),
                },
            ),
        ];
        let edited = delete_action(&script, 0).expect("delete");
        assert_eq!(edited.events.len(), 1);
        assert_eq!(edited.events[0].t, 30);
        assert!(
            delete_action(&edited, 0).is_err(),
            "empty recordings are invalid"
        );
    }
}

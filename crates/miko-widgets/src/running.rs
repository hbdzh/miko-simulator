//! One job of a kind per device (a microphone injection, a game controller),
//! each with a stop flag that its stop command sets.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use miko_core::{MikoError, MikoResult};
use uuid::Uuid;

/// Stop flags of the jobs in progress, by device.
#[derive(Debug, Default)]
pub struct Running(Mutex<HashMap<Uuid, Entry>>);

#[derive(Debug)]
struct Entry {
    stop: Arc<AtomicBool>,
    ready: bool,
    recovering: bool,
    capture: bool,
    session: Option<Uuid>,
    controller: Option<usize>,
}

impl Running {
    pub fn get(&self, id: Uuid) -> Option<Arc<AtomicBool>> {
        self.0.lock().ok()?.get(&id).map(|entry| entry.stop.clone())
    }

    /// Marks a registered job ready only while it has not been stopped.
    pub fn mark_ready(&self, id: Uuid) -> bool {
        let Ok(mut entries) = self.0.lock() else {
            return false;
        };
        let Some(entry) = entries.get_mut(&id) else {
            return false;
        };
        if entry.stop.load(Ordering::Relaxed) {
            return false;
        }
        entry.ready = true;
        entry.recovering = false;
        true
    }

    pub fn clear_ready(&self, id: Uuid) {
        if let Ok(mut entries) = self.0.lock()
            && let Some(entry) = entries.get_mut(&id)
        {
            entry.ready = false;
        }
    }

    pub fn mark_recovering(&self, id: Uuid) {
        if let Ok(mut entries) = self.0.lock()
            && let Some(entry) = entries.get_mut(&id)
        {
            entry.ready = false;
            entry.recovering = !entry.stop.load(Ordering::Relaxed);
        }
    }

    /// Commit this job to reopening before a new binding can replace it.
    pub fn begin_reconnect(&self, job: &Registered<'_>) -> bool {
        let Ok(mut entries) = self.0.lock() else {
            return false;
        };
        let Some(entry) = entries.get_mut(&job.id) else {
            return false;
        };
        if !entry.recovering || !Arc::ptr_eq(&entry.stop, &job.flag) {
            return false;
        }
        entry.recovering = false;
        true
    }

    pub fn set_capture(&self, id: Uuid, session: Uuid) {
        if let Ok(mut entries) = self.0.lock()
            && let Some(entry) = entries.get_mut(&id)
        {
            entry.capture = true;
            entry.session = Some(session);
        }
    }

    pub fn set_native_controller(&self, id: Uuid, session: Uuid, controller: usize) {
        if let Ok(mut entries) = self.0.lock()
            && let Some(entry) = entries.get_mut(&id)
        {
            entry.session = Some(session);
            entry.controller = Some(controller);
        }
    }

    pub fn native_controller(&self, id: Uuid, session: Uuid) -> Option<usize> {
        let entries = self.0.lock().ok()?;
        let entry = entries.get(&id)?;
        (entry.ready
            && !entry.capture
            && !entry.stop.load(Ordering::Relaxed)
            && entry.session == Some(session))
        .then_some(entry.controller)
        .flatten()
    }

    pub fn stop_session(&self, id: Uuid, session: Uuid) -> bool {
        let Ok(mut entries) = self.0.lock() else {
            return false;
        };
        let Some(entry) = entries.get_mut(&id) else {
            return false;
        };
        if entry.session != Some(session) {
            return false;
        }
        entry.ready = false;
        entry.recovering = false;
        entry.stop.store(true, Ordering::Relaxed);
        true
    }

    pub fn status(&self, id: Uuid) -> (bool, bool, bool, Option<Uuid>) {
        self.0
            .lock()
            .ok()
            .and_then(|entries| {
                entries.get(&id).map(|entry| {
                    let active = !entry.stop.load(Ordering::Relaxed);
                    (
                        entry.ready && active,
                        entry.recovering && active,
                        entry.capture && active,
                        entry.session.filter(|_| active),
                    )
                })
            })
            .unwrap_or((false, false, false, None))
    }

    /// Registers a job on `id`; `busy` (with the message `busy` builds)
    /// while another one runs.
    pub fn start(&self, id: Uuid, busy: impl FnOnce() -> String) -> MikoResult<Registered<'_>> {
        self.start_inner(id, busy, false)
    }

    /// A new controller may replace a disconnected job waiting for recovery.
    pub fn start_replacing_recovery(
        &self,
        id: Uuid,
        busy: impl FnOnce() -> String,
    ) -> MikoResult<Registered<'_>> {
        self.start_inner(id, busy, true)
    }

    fn start_inner(
        &self,
        id: Uuid,
        busy: impl FnOnce() -> String,
        replace_recovery: bool,
    ) -> MikoResult<Registered<'_>> {
        let flag = Arc::new(AtomicBool::new(false));
        let mut m = self
            .0
            .lock()
            .map_err(|_| MikoError::unexpected("running jobs lock poisoned"))?;
        if let Some(previous) = m.get(&id) {
            if !replace_recovery || !previous.recovering {
                return Err(MikoError::busy(busy()));
            }
            previous.stop.store(true, Ordering::Relaxed);
        }
        m.insert(
            id,
            Entry {
                stop: flag.clone(),
                ready: false,
                recovering: false,
                capture: false,
                session: None,
                controller: None,
            },
        );
        Ok(Registered {
            map: self,
            id,
            flag,
        })
    }

    /// Asks the job on `id` to stop; false when none runs.
    pub fn stop(&self, id: Uuid) -> bool {
        match self.0.lock().ok().and_then(|mut entries| {
            entries.get_mut(&id).map(|entry| {
                entry.ready = false;
                entry.recovering = false;
                entry.stop.store(true, Ordering::Relaxed);
            })
        }) {
            Some(()) => true,
            None => false,
        }
    }
}

/// Removes its job from the map when dropped, however the job ends.
#[derive(Debug)]
pub struct Registered<'a> {
    map: &'a Running,
    id: Uuid,
    pub flag: Arc<AtomicBool>,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        if let Ok(mut m) = self.map.0.lock()
            && m.get(&self.id)
                .is_some_and(|entry| Arc::ptr_eq(&entry.stop, &self.flag))
        {
            m.remove(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_controller_replaces_only_a_waiting_recovery() {
        let jobs = Running::default();
        let id = Uuid::new_v4();
        let old = jobs.start(id, String::new).unwrap();
        assert!(jobs.start_replacing_recovery(id, String::new).is_err());
        jobs.mark_recovering(id);
        let new = jobs.start_replacing_recovery(id, String::new).unwrap();
        assert!(old.flag.load(Ordering::Relaxed));
        drop(old);
        assert!(
            jobs.get(id).is_some(),
            "old guard must not remove the new job"
        );
        drop(new);
        assert!(jobs.get(id).is_none());
    }

    #[test]
    fn stale_capture_stop_cannot_stop_a_new_session() {
        let jobs = Running::default();
        let device = Uuid::new_v4();
        let old = Uuid::new_v4();
        let new = Uuid::new_v4();
        let first = jobs.start(device, String::new).unwrap();
        jobs.set_capture(device, old);
        assert!(
            jobs.start_replacing_recovery(device, String::new).is_err(),
            "native forwarding cannot start during capture"
        );
        assert!(jobs.stop_session(device, old));
        drop(first);
        let second = jobs.start(device, String::new).unwrap();
        jobs.set_capture(device, new);
        assert!(!jobs.stop_session(device, old));
        assert!(!second.flag.load(Ordering::Relaxed));
        assert!(jobs.stop_session(device, new));
    }

    #[test]
    fn one_job_per_device() {
        let jobs = Running::default();
        let id = Uuid::new_v4();
        let reg = jobs.start(id, || "Phone is busy".into()).unwrap();
        assert_eq!(jobs.status(id), (false, false, false, None));
        assert!(jobs.mark_ready(id));
        assert_eq!(jobs.status(id), (true, false, false, None));
        jobs.mark_recovering(id);
        assert_eq!(jobs.status(id), (false, true, false, None));
        assert!(jobs.mark_ready(id));
        let e = jobs.start(id, || "Phone is busy".into()).err().unwrap();
        assert_eq!(
            (e.code, e.message.as_str()),
            (miko_core::ErrorCode::Busy, "Phone is busy")
        );
        // Another device is fine.
        let other = jobs.start(Uuid::new_v4(), String::new).unwrap();
        // The stop command sets the job's flag.
        assert!(jobs.stop(id));
        assert_eq!(jobs.status(id), (false, false, false, None));
        assert!(!jobs.mark_ready(id), "stopped jobs cannot become ready");
        assert!(reg.flag.load(Ordering::Relaxed));
        drop(reg);
        assert!(jobs.get(id).is_none());
        assert!(!jobs.stop(id), "nothing left to stop");
        assert!(jobs.start(id, String::new).is_ok());
        drop(other);
    }
}

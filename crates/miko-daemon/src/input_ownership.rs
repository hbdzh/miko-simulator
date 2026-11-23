//! Device input ownership for cooperating Miko clients.
use miko_core::{Cancel, MikoError, MikoResult};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Owner {
    connection: u64,
    token: Uuid,
    job: bool,
    preemptible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Waiter {
    connection: u64,
    request: Uuid,
}

pub(crate) struct InputOwnership {
    dir: PathBuf,
    connections: HashSet<u64>,
    owners: HashMap<Uuid, Owner>,
    abandoned: HashSet<Uuid>,
    waiters: HashMap<Uuid, Waiter>,
    job_cancellations: HashMap<Uuid, Cancel>,
}

impl InputOwnership {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            connections: HashSet::new(),
            owners: HashMap::new(),
            abandoned: HashSet::new(),
            waiters: HashMap::new(),
            job_cancellations: HashMap::new(),
        }
    }
    pub fn connected(&mut self, connection: u64) {
        self.connections.insert(connection);
    }
    pub fn disconnected(&mut self, connection: u64) {
        self.connections.remove(&connection);
        self.waiters
            .retain(|_, waiter| waiter.connection != connection);
        self.owners.retain(|device, owner| {
            if owner.connection == connection && !owner.job {
                self.abandoned.insert(*device);
                false
            } else {
                true
            }
        });
    }
    pub fn acquire(
        &mut self,
        device: Uuid,
        connection: u64,
        preemptible: bool,
    ) -> MikoResult<Uuid> {
        if self.waiters.contains_key(&device) {
            return Err(MikoError::busy("device input handoff is in progress"));
        }
        self.acquire_available(device, connection, preemptible)
    }

    fn acquire_available(
        &mut self,
        device: Uuid,
        connection: u64,
        preemptible: bool,
    ) -> MikoResult<Uuid> {
        if !self.connections.contains(&connection) {
            return Err(MikoError::busy("input client disconnected"));
        }
        if self.abandoned.contains(&device) {
            return Err(MikoError::busy(
                "previous input owner disconnected; restart the device before sending input",
            ));
        }
        if self.owners.contains_key(&device) {
            return Err(MikoError::busy("device input is busy"));
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| {
            MikoError::unexpected(format!("creating input ownership directory: {e}"))
        })?;
        let marker = self.dir.join(device.to_string());
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
        {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(MikoError::busy(
                    "previous input owner did not release; restart the device before sending input",
                ));
            }
            Err(error) => {
                return Err(MikoError::unexpected(format!(
                    "saving input ownership: {error}"
                )));
            }
        }
        let token = Uuid::new_v4();
        self.owners.insert(
            device,
            Owner {
                connection,
                token,
                job: false,
                preemptible,
            },
        );
        Ok(token)
    }

    /// Reserve the next turn before notifying a preemptible owner.
    pub fn begin_takeover(&mut self, device: Uuid, connection: u64) -> MikoResult<(Uuid, Uuid)> {
        if !self.connections.contains(&connection) {
            return Err(MikoError::busy("input client disconnected"));
        }
        if self.waiters.contains_key(&device) {
            return Err(MikoError::busy("device input handoff is in progress"));
        }
        let owner = self
            .owners
            .get(&device)
            .ok_or_else(|| MikoError::busy("input owner changed"))?;
        if !owner.preemptible {
            return Err(MikoError::busy("device input is busy"));
        }
        let owner_token = owner.token;
        if let Some(cancel) = self.job_cancellations.get(&device) {
            cancel.cancel();
        }
        let request = Uuid::new_v4();
        self.waiters.insert(
            device,
            Waiter {
                connection,
                request,
            },
        );
        Ok((request, owner_token))
    }

    pub fn finish_takeover(
        &mut self,
        device: Uuid,
        connection: u64,
        request: Uuid,
    ) -> MikoResult<Option<Uuid>> {
        if self.waiters.get(&device)
            != Some(&Waiter {
                connection,
                request,
            })
        {
            return Err(MikoError::busy("input handoff was cancelled"));
        }
        if self.owners.contains_key(&device) {
            return Ok(None);
        }
        self.waiters.remove(&device);
        self.acquire_available(device, connection, false).map(Some)
    }

    pub fn cancel_takeover(&mut self, device: Uuid, connection: u64, request: Uuid) {
        if self.waiters.get(&device)
            == Some(&Waiter {
                connection,
                request,
            })
        {
            self.waiters.remove(&device);
        }
    }
    pub fn owns(&self, device: Uuid, connection: u64, token: Uuid) -> bool {
        self.owners
            .get(&device)
            .is_some_and(|owner| owner.connection == connection && owner.token == token)
    }

    pub fn pin_job(&mut self, device: Uuid, connection: u64, token: Uuid) -> MikoResult<()> {
        if !self.owns(device, connection, token) {
            return Err(MikoError::busy("input ownership changed"));
        }
        if let Some(owner) = self.owners.get_mut(&device) {
            owner.job = true;
        }
        Ok(())
    }
    pub fn set_job_cancel(
        &mut self,
        device: Uuid,
        connection: u64,
        token: Uuid,
        cancel: Cancel,
    ) -> MikoResult<()> {
        if !self.owns(device, connection, token) {
            return Err(MikoError::busy("input ownership changed"));
        }
        if self.waiters.contains_key(&device) {
            cancel.cancel();
        }
        self.job_cancellations.insert(device, cancel);
        Ok(())
    }
    pub fn release(&mut self, device: Uuid, connection: u64, token: Uuid) -> MikoResult<()> {
        if !self.owns(device, connection, token) {
            return Err(MikoError::busy("input ownership changed"));
        }
        std::fs::remove_file(self.dir.join(device.to_string()))
            .map_err(|e| MikoError::unexpected(format!("releasing input ownership: {e}")))?;
        self.owners.remove(&device);
        self.job_cancellations.remove(&device);
        Ok(())
    }
    pub fn abandon(&mut self, device: Uuid, connection: u64, token: Uuid) {
        if self.owns(device, connection, token) {
            self.owners.remove(&device);
            self.job_cancellations.remove(&device);
            self.abandoned.insert(device);
        }
    }
    pub fn device_stopped(&mut self, device: Uuid) {
        if let Some(cancel) = self.job_cancellations.remove(&device) {
            cancel.cancel();
        }
        self.waiters.remove(&device);
        match std::fs::remove_file(self.dir.join(device.to_string())) {
            Ok(()) => {
                self.owners.remove(&device);
                self.abandoned.remove(&device);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.owners.remove(&device);
                self.abandoned.remove(&device);
            }
            Err(error) => {
                tracing::warn!(%device, %error, "cannot clear input ownership after stopping device")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excludes_conflicts_and_fences_abandoned_owners() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = InputOwnership::new(dir.path().to_owned());
        let device = Uuid::new_v4();
        input.connected(1);
        input.connected(2);
        let old = input.acquire(device, 1, false).unwrap();
        assert!(input.acquire(device, 2, false).is_err());
        assert!(input.release(device, 2, old).is_err());
        input.disconnected(1);
        assert!(input.acquire(device, 2, false).is_err());
        input.device_stopped(device);
        let new = input.acquire(device, 2, false).unwrap();
        assert!(input.release(device, 1, old).is_err());
        input.release(device, 2, new).unwrap();
    }

    #[test]
    fn daemon_restart_does_not_forget_an_unreleased_owner() {
        let dir = tempfile::tempdir().unwrap();
        let device = Uuid::new_v4();
        let mut first = InputOwnership::new(dir.path().to_owned());
        first.connected(1);
        first.acquire(device, 1, false).unwrap();
        drop(first);
        let mut second = InputOwnership::new(dir.path().to_owned());
        second.connected(2);
        assert!(second.acquire(device, 2, false).is_err());
        second.device_stopped(device);
        let token = second.acquire(device, 2, false).unwrap();
        second.release(device, 2, token).unwrap();
    }

    #[test]
    fn uncertain_input_failure_fences_the_device() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = InputOwnership::new(dir.path().to_owned());
        let device = Uuid::new_v4();
        input.connected(1);
        input.connected(2);
        let token = input.acquire(device, 1, false).unwrap();
        input.abandon(device, 1, token);
        assert!(input.acquire(device, 2, false).is_err());
        input.device_stopped(device);
        let next = input.acquire(device, 2, false).unwrap();
        input.release(device, 2, next).unwrap();
    }

    #[test]
    fn job_keeps_ownership_after_request_connection_closes() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = InputOwnership::new(dir.path().to_owned());
        let device = Uuid::new_v4();
        input.connected(1);
        input.connected(2);
        let token = input.acquire(device, 1, false).unwrap();
        input.pin_job(device, 1, token).unwrap();
        input.disconnected(1);
        assert!(input.acquire(device, 2, false).is_err());
        input.release(device, 1, token).unwrap();
        let next = input.acquire(device, 2, false).unwrap();
        input.release(device, 2, next).unwrap();
    }

    #[test]
    fn simultaneous_claims_grant_only_one_owner() {
        let dir = tempfile::tempdir().unwrap();
        let device = Uuid::new_v4();
        let mut input = InputOwnership::new(dir.path().to_owned());
        for connection in 1..=8 {
            input.connected(connection);
        }
        let input = std::sync::Arc::new(std::sync::Mutex::new(input));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (1..=8)
            .map(|connection| {
                let input = input.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    input
                        .lock()
                        .unwrap()
                        .acquire(device, connection, false)
                        .ok()
                        .map(|token| (connection, token))
                })
            })
            .collect();
        let winners: Vec<_> = threads
            .into_iter()
            .filter_map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(winners.len(), 1);
        input
            .lock()
            .unwrap()
            .release(device, winners[0].0, winners[0].1)
            .unwrap();
    }

    #[test]
    fn takeover_waits_for_release_and_reserves_the_next_owner() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = InputOwnership::new(dir.path().to_owned());
        let device = Uuid::new_v4();
        for connection in 1..=3 {
            input.connected(connection);
        }
        let old = input.acquire(device, 1, true).unwrap();
        let (request, notified) = input.begin_takeover(device, 2).unwrap();
        assert_eq!(notified, old);
        assert!(input.finish_takeover(device, 2, request).unwrap().is_none());
        assert!(input.acquire(device, 3, false).is_err());
        input.release(device, 1, old).unwrap();
        assert!(input.acquire(device, 3, false).is_err());
        let next = input.finish_takeover(device, 2, request).unwrap().unwrap();
        assert!(input.release(device, 1, old).is_err());
        input.release(device, 2, next).unwrap();
    }

    #[test]
    fn workflow_takeover_cancels_job_before_ownership_is_released() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = InputOwnership::new(dir.path().to_owned());
        let device = Uuid::new_v4();
        input.connected(1);
        input.connected(2);
        let token = input.acquire(device, 1, true).unwrap();
        input.pin_job(device, 1, token).unwrap();
        let (request, _) = input.begin_takeover(device, 2).unwrap();
        let cancel = Cancel::new();
        input
            .set_job_cancel(device, 1, token, cancel.clone())
            .unwrap();
        assert!(cancel.is_cancelled());
        assert!(input.finish_takeover(device, 2, request).unwrap().is_none());
        input.release(device, 1, token).unwrap();
        let next = input.finish_takeover(device, 2, request).unwrap().unwrap();
        input.release(device, 2, next).unwrap();
    }

    #[test]
    fn takeover_refuses_nonpreemptible_and_cancellation_clears_reservation() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = InputOwnership::new(dir.path().to_owned());
        let device = Uuid::new_v4();
        input.connected(1);
        input.connected(2);
        let first = input.acquire(device, 1, false).unwrap();
        assert!(input.begin_takeover(device, 2).is_err());
        input.release(device, 1, first).unwrap();
        let first = input.acquire(device, 1, true).unwrap();
        let (request, _) = input.begin_takeover(device, 2).unwrap();
        input.cancel_takeover(device, 2, request);
        input.release(device, 1, first).unwrap();
        let next = input.acquire(device, 1, false).unwrap();
        input.release(device, 1, next).unwrap();
    }
}

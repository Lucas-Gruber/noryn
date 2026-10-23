use crate::{
    port_forward::{Forward, ForwardInfo},
    system_info::{self, Section, Snapshot},
};
use anyhow::{ensure, Result};
use ssh2::Session;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub struct Tools {
    session: Session,
    closed: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
    forwards: Mutex<HashMap<String, Forward>>,
    password: zeroize::Zeroizing<String>,
    actions: Mutex<()>,
    snapshots: Mutex<HashMap<Section, (Instant, Snapshot)>>,
}
impl Tools {
    pub fn new(session: Session) -> Self {
        session.set_blocking(false);
        Self {
            session,
            closed: Arc::new(AtomicBool::new(false)),
            active: Arc::new(AtomicUsize::new(0)),
            forwards: Mutex::new(HashMap::new()),
            password: zeroize::Zeroizing::new(String::new()),
            actions: Mutex::new(()),
            snapshots: Mutex::new(HashMap::new()),
        }
    }
    pub fn with_password(mut self, password: String) -> Self {
        self.password = zeroize::Zeroizing::new(password);
        self
    }
    pub fn control(
        &self,
        request: &crate::system_control::Request,
        elevated: bool,
        password: Option<String>,
    ) -> Result<crate::system_control::Reply> {
        let _guard = self
            .actions
            .lock()
            .map_err(|_| anyhow::anyhow!("Aktion nicht verfügbar"))?;
        let result = crate::system_control::run(
            &self.session,
            &self.closed,
            &self.password,
            request,
            elevated,
            password,
        );
        if let Ok(mut cache) = self.snapshots.lock() {
            cache.clear();
        }
        result
    }
    pub fn start_forward(
        &self,
        local_port: u16,
        remote_host: String,
        remote_port: u16,
    ) -> Result<ForwardInfo> {
        let mut forwards = self
            .forwards
            .lock()
            .map_err(|_| anyhow::anyhow!("Weiterleitungen nicht verfügbar"))?;
        ensure!(
            !self.closed.load(Ordering::Acquire),
            "SSH-Sitzung ist beendet"
        );
        ensure!(
            forwards.len() < 16,
            "Maximal 16 Weiterleitungen pro SSH-Sitzung"
        );
        let forward = Forward::start(
            self.session.clone(),
            self.closed.clone(),
            self.active.clone(),
            local_port,
            remote_host,
            remote_port,
        )?;
        let info = forward.info();
        forwards.insert(info.id.clone(), forward);
        Ok(info)
    }
    pub fn list_forwards(&self) -> Result<Vec<ForwardInfo>> {
        let mut entries: Vec<_> = self
            .forwards
            .lock()
            .map_err(|_| anyhow::anyhow!("Weiterleitungen nicht verfügbar"))?
            .values()
            .map(Forward::info)
            .collect();
        entries.sort_by_key(|forward| forward.local_port);
        Ok(entries)
    }
    pub fn stop_forward(&self, id: &str) -> Result<()> {
        let mut forward = self
            .forwards
            .lock()
            .map_err(|_| anyhow::anyhow!("Weiterleitungen nicht verfügbar"))?
            .remove(id)
            .ok_or_else(|| anyhow::anyhow!("Weiterleitung nicht gefunden"))?;
        forward.stop();
        Ok(())
    }
    pub fn snapshot(&self, section: Section) -> Result<Snapshot> {
        let mut cache = self
            .snapshots
            .lock()
            .map_err(|_| anyhow::anyhow!("Systemansicht nicht verfügbar"))?;
        ensure!(
            !self.closed.load(Ordering::Acquire),
            "SSH-Sitzung ist beendet"
        );
        if let Some((time, snapshot)) = cache.get(&section) {
            if time.elapsed() < Duration::from_secs(1) {
                return Ok(snapshot.clone());
            }
        }
        let snapshot = system_info::collect(&self.session, &self.closed, section)?;
        cache.insert(section, (Instant::now(), snapshot.clone()));
        Ok(snapshot)
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(mut forwards) = self.forwards.lock() {
            for (_, mut forward) in forwards.drain() {
                forward.stop();
            }
        }
    }
}
impl Drop for Tools {
    fn drop(&mut self) {
        self.close();
    }
}

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
    login_health: Mutex<Option<crate::login_health::LoginHealth>>,
}
impl Tools {
    pub fn run_command(
        &self,
        command: &str,
        cancelled: &AtomicBool,
        timeout: Duration,
    ) -> crate::command_execution::Output {
        crate::command_execution::run(&self.session, &self.closed, cancelled, command, timeout)
    }
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
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
            login_health: Mutex::new(None),
        }
    }
    pub fn with_password(mut self, password: String) -> Self {
        self.password = zeroize::Zeroizing::new(password);
        self
    }
    pub fn login_health(
        &self,
        previous: Option<&crate::login_health::LoginPoint>,
    ) -> Result<crate::login_health::LoginHealth> {
        let mut cache = self
            .login_health
            .lock()
            .map_err(|_| anyhow::anyhow!("Login-Bericht nicht verfügbar"))?;
        ensure!(
            !self.closed.load(Ordering::Acquire),
            "SSH-Sitzung ist beendet"
        );
        if let Some(report) = &*cache {
            return Ok(report.clone());
        }
        let report = crate::login_health::collect(&self.session, &self.closed, previous)?;
        *cache = Some(report.clone());
        Ok(report)
    }
    pub fn listening_ports(&self) -> Result<Vec<crate::login_health::ListeningPort>> {
        crate::login_health::ports(&self.session, &self.closed)
    }
    pub fn probe_forward(&self, id: &str) -> Result<String> {
        ensure!(
            !self.closed.load(Ordering::Acquire),
            "SSH-Sitzung ist beendet"
        );
        let forward = self
            .list_forwards()?
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| anyhow::anyhow!("Weiterleitung nicht gefunden"))?;
        // Verify the target using this authenticated transport; never scan a
        // supplied address or send protocol requests to an unknown service.
        let mut channel = crate::ssh::retry(|| {
            self.session
                .channel_direct_tcpip(&forward.remote_host, forward.remote_port, None)
        })?;
        use std::io::Read;
        let started = Instant::now();
        let mut buffer = [0u8; 1024];
        let result = loop {
            if self.closed.load(Ordering::Acquire) {
                break Err(anyhow::anyhow!("SSH-Sitzung ist beendet"));
            }
            match channel.read(&mut buffer) {
                Ok(n) if n > 0 => {
                    let banner: String = String::from_utf8_lossy(&buffer[..n])
                        .chars()
                        .filter(|c| !c.is_control() || ['\n', '\t'].contains(c))
                        .collect();
                    break Ok(format!("TCP-Ziel erreichbar. Begrüßung: {banner}"));
                }
                Ok(_) if channel.eof() => {
                    break Ok("TCP-Ziel erreichbar; Verbindung ohne Begrüßung geschlossen.".into())
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => break Err(e.into()),
            }
            if started.elapsed() >= Duration::from_millis(750) {
                break Ok("TCP-Ziel erreichbar; keine passive Begrüßung. HTTP/TLS und viele Datenbanken senden erst nach einer Anfrage Daten.".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let _ = channel.close();
        result
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

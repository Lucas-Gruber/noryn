use anyhow::{ensure, Context, Result};
use polling::{Event, Events, Poller};
use serde::Serialize;
use ssh2::{BlockDirections, Session};
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardInfo {
    pub id: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub active_connections: usize,
    pub uploaded: u64,
    pub downloaded: u64,
    pub last_error: Option<String>,
}

struct Control {
    stopped: AtomicBool,
    closed: Arc<AtomicBool>,
    active: AtomicUsize,
    total_active: Arc<AtomicUsize>,
    uploaded: AtomicU64,
    downloaded: AtomicU64,
    error: Mutex<Option<String>>,
}
impl Control {
    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire) || self.closed.load(Ordering::Acquire)
    }
    fn error(&self, error: impl ToString) {
        if let Ok(mut value) = self.error.lock() {
            *value = Some(error.to_string());
        }
    }
}

pub struct Forward {
    info: ForwardInfo,
    control: Arc<Control>,
    poller: Arc<Poller>,
    listener: Option<JoinHandle<()>>,
}

impl Forward {
    pub fn start(
        session: Session,
        closed: Arc<AtomicBool>,
        total_active: Arc<AtomicUsize>,
        local_port: u16,
        remote_host: String,
        remote_port: u16,
    ) -> Result<Self> {
        ensure!(!closed.load(Ordering::Acquire), "SSH-Sitzung ist beendet");
        ensure!(
            remote_port > 0
                && !remote_host.is_empty()
                && remote_host.len() <= 253
                && !remote_host.chars().any(char::is_whitespace)
                && !remote_host.chars().any(char::is_control),
            "Ungültiges Weiterleitungsziel"
        );
        let listener = TcpListener::bind(("127.0.0.1", local_port))
            .context("Lokaler Port kann nicht geöffnet werden; möglicherweise bereits belegt")?;
        listener.set_nonblocking(true)?;
        let local_port = listener.local_addr()?.port();
        let poller = Arc::new(Poller::new()?);
        let control = Arc::new(Control {
            stopped: AtomicBool::new(false),
            closed,
            active: AtomicUsize::new(0),
            total_active,
            uploaded: AtomicU64::new(0),
            downloaded: AtomicU64::new(0),
            error: Mutex::new(None),
        });
        let info = ForwardInfo {
            id: uuid::Uuid::new_v4().to_string(),
            local_port,
            remote_host: remote_host.clone(),
            remote_port,
            active_connections: 0,
            uploaded: 0,
            downloaded: 0,
            last_error: None,
        };
        let worker_control = control.clone();
        let worker_poller = poller.clone();
        let worker = thread::Builder::new().name("noryn-forward-listener".into()).spawn(move || {
            let run = || -> Result<()> {
                // SAFETY: listener stays alive until it is removed from this poller.
                unsafe { worker_poller.add(&listener, Event::readable(0))?; }
                let result = (|| -> Result<()> {
                    let mut events = Events::new();
                    while !worker_control.stopped() {
                        match listener.accept() {
                            Ok((stream, source)) => {
                                if worker_control.stopped() { break; }
                                if worker_control.total_active.try_update(Ordering::AcqRel, Ordering::Acquire, |count| (count < 32).then_some(count + 1)).is_err() {
                                    worker_control.error("Maximal 32 gleichzeitige Tunnel-Verbindungen pro SSH-Sitzung");
                                    continue;
                                }
                                worker_control.active.fetch_add(1, Ordering::Relaxed);
                                let active = worker_control.clone();
                                let session = session.clone();
                                let host = remote_host.clone();
                                let source_host = source.ip().to_string();
                                let spawn = thread::Builder::new().name("noryn-forward-stream".into()).spawn(move || {
                                    struct Count(Arc<Control>);
                                    impl Drop for Count { fn drop(&mut self) { self.0.active.fetch_sub(1, Ordering::Relaxed); self.0.total_active.fetch_sub(1, Ordering::AcqRel); } }
                                    let _count = Count(active.clone());
                                    if let Err(error) = pump(session, stream, &host, remote_port, &source_host, source.port(), &active) { if !active.stopped() { active.error(format!("{error:#}")); } }
                                });
                                if let Err(error) = spawn {
                                    worker_control.active.fetch_sub(1, Ordering::Relaxed);
                                    worker_control.total_active.fetch_sub(1, Ordering::AcqRel);
                                    worker_control.error(error);
                                }
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                worker_poller.modify(&listener, Event::readable(0))?;
                                events.clear();
                                // No periodic wake-ups while the local listener is idle.
                                worker_poller.wait(&mut events, None)?;
                            }
                            Err(error) => return Err(error.into()),
                        }
                    }
                    Ok(())
                })();
                let _ = worker_poller.delete(&listener);
                result
            };
            if let Err(error) = run() { worker_control.error(error); }
        })?;
        Ok(Self {
            info,
            control,
            poller,
            listener: Some(worker),
        })
    }

    pub fn info(&self) -> ForwardInfo {
        let mut info = self.info.clone();
        info.active_connections = self.control.active.load(Ordering::Relaxed);
        info.uploaded = self.control.uploaded.load(Ordering::Relaxed);
        info.downloaded = self.control.downloaded.load(Ordering::Relaxed);
        info.last_error = self
            .control
            .error
            .lock()
            .ok()
            .and_then(|value| value.clone());
        info
    }
    pub fn stop(&mut self) {
        self.control.stopped.store(true, Ordering::Release);
        let _ = self.poller.notify();
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
    }
}
impl Drop for Forward {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(unix)]
fn ssh_socket(session: &Session) -> std::os::fd::BorrowedFd<'_> {
    use std::os::fd::AsRawFd;
    // SAFETY: the session owns the socket and outlives this borrowed handle.
    unsafe { std::os::fd::BorrowedFd::borrow_raw(session.as_raw_fd()) }
}
#[cfg(windows)]
fn ssh_socket(session: &Session) -> std::os::windows::io::BorrowedSocket<'_> {
    use std::os::windows::io::AsRawSocket;
    // SAFETY: the session owns the socket and outlives this borrowed handle.
    unsafe { std::os::windows::io::BorrowedSocket::borrow_raw(session.as_raw_socket()) }
}

fn pump(
    session: Session,
    mut stream: TcpStream,
    host: &str,
    port: u16,
    source: &str,
    source_port: u16,
    control: &Control,
) -> Result<()> {
    stream.set_nonblocking(true)?;
    stream.set_nodelay(true)?;
    let poller = Poller::new()?;
    let socket = ssh_socket(&session);
    // SAFETY: both sources are removed before the stream/session can be dropped.
    unsafe {
        poller.add(&stream, Event::readable(0))?;
    }
    if let Err(error) = unsafe { poller.add(&socket, Event::readable(1)) } {
        let _ = poller.delete(&stream);
        return Err(error.into());
    }
    let result = (|| -> Result<()> {
        let mut events = Events::new();
        let start = Instant::now();
        let mut channel = loop {
            ensure!(!control.stopped(), "Weiterleitung beendet");
            match session.channel_direct_tcpip(host, port, Some((source, source_port))) {
                Ok(channel) => break channel,
                Err(error) if error.code() == ssh2::ErrorCode::Session(-37) => {
                    ensure!(
                        start.elapsed() < Duration::from_secs(15),
                        "Zeitlimit beim Öffnen des SSH-Tunnels"
                    );
                    poller.modify(
                        socket,
                        Event::new(
                            1,
                            true,
                            matches!(
                                session.block_directions(),
                                BlockDirections::Outbound | BlockDirections::Both
                            ),
                        ),
                    )?;
                    events.clear();
                    poller.wait(&mut events, Some(Duration::from_millis(100)))?;
                }
                Err(error) => return Err(error.into()),
            }
        };
        let mut up = [0_u8; 32 * 1024];
        let mut down = [0_u8; 32 * 1024];
        let (mut up_start, mut up_end, mut down_start, mut down_end) = (0, 0, 0, 0);
        let (mut local_eof, mut remote_eof, mut sent_eof, mut write_shutdown) =
            (false, false, false, false);
        while !control.stopped() {
            let mut progress = false;
            if !local_eof && up_start == up_end {
                match stream.read(&mut up) {
                    Ok(0) => {
                        local_eof = true;
                        progress = true;
                    }
                    Ok(n) => {
                        up_start = 0;
                        up_end = n;
                        progress = true;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error.into()),
                }
            }
            let mut ssh_write = false;
            if up_start < up_end {
                match channel.write(&up[up_start..up_end]) {
                    Ok(n) => {
                        up_start += n;
                        control.uploaded.fetch_add(n as u64, Ordering::Relaxed);
                        progress |= n > 0;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        ssh_write = matches!(
                            session.block_directions(),
                            BlockDirections::Outbound | BlockDirections::Both
                        );
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            if local_eof && up_start == up_end && !sent_eof {
                match channel.send_eof() {
                    Ok(()) => {
                        sent_eof = true;
                        progress = true;
                    }
                    Err(error) if error.code() == ssh2::ErrorCode::Session(-37) => {
                        ssh_write |= matches!(
                            session.block_directions(),
                            BlockDirections::Outbound | BlockDirections::Both
                        );
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            if !remote_eof && down_start == down_end {
                match channel.read(&mut down) {
                    Ok(0) if channel.eof() => {
                        remote_eof = true;
                        progress = true;
                    }
                    Ok(n) => {
                        down_start = 0;
                        down_end = n;
                        progress |= n > 0;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error.into()),
                }
            }
            if down_start < down_end {
                match stream.write(&down[down_start..down_end]) {
                    Ok(n) => {
                        down_start += n;
                        control.downloaded.fetch_add(n as u64, Ordering::Relaxed);
                        progress |= n > 0;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error.into()),
                }
            }
            if remote_eof && down_start == down_end && !write_shutdown {
                stream.shutdown(Shutdown::Write)?;
                write_shutdown = true;
            }
            if local_eof && remote_eof && up_start == up_end && down_start == down_end {
                break;
            }
            if !progress {
                poller.modify(
                    &stream,
                    Event::new(0, !local_eof && up_start == up_end, down_start < down_end),
                )?;
                poller.modify(socket, Event::new(1, true, ssh_write))?;
                events.clear();
                // A different SSH channel may have already buffered our data.
                // The timeout checks those buffers without busy-polling idle streams.
                poller.wait(&mut events, Some(Duration::from_millis(100)))?;
            }
        }
        let _ = channel.close();
        Ok(())
    })();
    let _ = poller.delete(&stream);
    let _ = poller.delete(socket);
    result
}

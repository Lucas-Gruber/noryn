//! One TLS/NLA RDP transport per tab. No external process, target agent or disk frames.
mod display;
use crate::{
    ensure_connection_context, quick_connections, vault_action, AppState, ConnectResult,
    Connection, Result,
};
use anyhow::{ensure, Context};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use display::{frame, Cursor, Damage, FRAME_INTERVAL, MAX_IN_FLIGHT};
use ironrdp_connector::{self as connector, Config, Credentials};
use ironrdp_displaycontrol::client::DisplayControlClient;
use ironrdp_dvc::DrdynvcClient;
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_input::{Database, MouseButton, MousePosition, Operation, WheelRotations};
use ironrdp_pdu::{
    gcc::{ConnectionType, KeyboardType},
    rdp::{
        capability_sets::MajorPlatformType,
        client_info::{CompressionType, PerformanceFlags, TimezoneInfo},
    },
};
use ironrdp_session::{image::DecodedImage, ActiveStage, ActiveStageBuilder, ActiveStageOutput};
use ironrdp_tokio::{FramedWrite, TokioFramed};
use noryn_core::{
    model::{Credential, Node},
    ssh::TerminalEvent,
};
use serde::Deserialize;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{
    ipc::{Channel, Response},
    State,
};
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot},
};

const MAX_PIXELS: usize = 3840 * 2160;
type Framed = TokioFramed<ironrdp_tls::TlsStream<TcpStream>>;

pub struct Handle {
    input: mpsc::Sender<Message>,
    task: tauri::async_runtime::JoinHandle<()>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.task.abort();
    }
}
enum Message {
    Input(Input),
    Subscribe(Channel<Response>),
    Resize(ResizeRequest),
}
struct ResizeRequest {
    width: u16,
    height: u16,
    deadline: tokio::time::Instant,
    reply: oneshot::Sender<Result<()>>,
}
fn resize_size(width: u16, height: u16) -> anyhow::Result<()> {
    size(width, height)?;
    ensure!(
        width >= 200 && height >= 200 && width.is_multiple_of(2),
        "RDP-Auflösung muss mindestens 200 × 200 Pixel groß sein und eine gerade Breite haben"
    );
    Ok(())
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Input {
    Key {
        code: u16,
        down: bool,
    },
    Text {
        text: String,
    },
    Move {
        x: u16,
        y: u16,
    },
    Button {
        button: u8,
        down: bool,
        x: u16,
        y: u16,
    },
    Wheel {
        delta: i16,
        horizontal: bool,
    },
    Release,
    SecureAttention,
    Visible {
        visible: bool,
    },
    Ack {
        sequence: u32,
    },
}
fn size(width: u16, height: u16) -> anyhow::Result<()> {
    ensure!(
        width > 0
            && height > 0
            && width <= 3840
            && height <= 2160
            && usize::from(width) * usize::from(height) <= MAX_PIXELS,
        "RDP-Bildschirm überschreitet das Limit von 3840 × 2160 Pixeln"
    );
    Ok(())
}
fn fingerprint(cert: &[u8]) -> String {
    format!(
        "SHA256:{}",
        STANDARD_NO_PAD.encode(noryn_core::text_files::digest(cert))
    )
}
fn config(credential: &Credential) -> Config {
    let (domain, username) = credential
        .username
        .split_once('\\')
        .map_or((None, credential.username.as_str()), |(domain, user)| {
            (Some(domain.to_owned()), user)
        });
    Config {
        desktop_size: connector::DesktopSize {
            width: 1600,
            height: 900,
        },
        monitor_layout: None,
        desktop_scale_factor: 0,
        desktop_physical_size: None,
        desktop_orientation: None,
        device_scale_factor: None,
        monitor_extended_layout: None,
        relative_mouse_input: false,
        enable_tls: false,
        enable_credssp: true,
        enable_standard_rdp_security: false,
        credentials: Credentials::UsernamePassword {
            username: username.into(),
            password: credential.login_password().into(),
        },
        domain,
        client_build: 0,
        client_name: "Noryn".into(),
        keyboard_type: KeyboardType::IBM_ENHANCED,
        keyboard_subtype: 0,
        keyboard_functional_keys_count: 12,
        keyboard_layout: 0,
        connection_type: ConnectionType::Lan,
        ime_file_name: String::new(),
        bitmap: None,
        dig_product_id: String::new(),
        client_dir: "C:\\Windows\\System32\\mstscax.dll".into(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        remote_application_mode: false,
        rail_support_level: ironrdp_pdu::rdp::capability_sets::RailSupportLevel::empty(),
        platform: if cfg!(target_os = "windows") {
            MajorPlatformType::WINDOWS
        } else if cfg!(target_os = "macos") {
            MajorPlatformType::MACINTOSH
        } else {
            MajorPlatformType::UNIX
        },
        hardware_id: None,
        request_data: None,
        autologon: true,
        enable_audio_playback: false,
        enable_audio_capture: false,
        performance_flags: PerformanceFlags::default(),
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
        compression_type: Some(CompressionType::Rdp61),
        enable_server_pointer: true,
        pointer_software_rendering: false,
        multitransport_flags: None,
        support_dyn_vc_gfx_protocol: false,
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn connect(
    state: &AppState,
    node: Node,
    credential: Credential,
    origin: quick_connections::Origin,
    epoch: u64,
    trusted: Option<String>,
    output: Channel<TerminalEvent>,
) -> Result<ConnectResult> {
    let endpoint = format!("rdp://{}:{}", node.host, node.port);
    let known = vault_action(state, |vault| {
        Ok(vault.data.known_hosts.get(&endpoint).cloned())
    })?;
    let observed: Arc<Mutex<Option<(String, bool)>>> = Arc::default();
    let capture = observed.clone();
    let review_pin = trusted.or_else(|| known.clone());
    let expected = review_pin.clone();
    let expected_endpoint = endpoint.clone();
    let callback = Arc::new(move |cert: &[u8], host: &str, _reason: &str| {
        let value = fingerprint(cert);
        let accepted = host == expected_endpoint && expected.as_ref() == Some(&value);
        *capture.lock().unwrap_or_else(|e| e.into_inner()) = Some((value, accepted));
        accepted
    });
    let handshake = async {
        ensure!(
            credential.kind == "password" || !credential.login_password().is_empty(),
            "RDP benötigt ein Benutzerpasswort; ein SSH-Schlüssel allein genügt nicht"
        );
        let stream = TcpStream::connect((node.host.as_str(), node.port))
            .await
            .context("RDP-Verbindung fehlgeschlagen")?;
        stream.set_nodelay(true)?;
        let address = stream.local_addr()?;
        let mut framed = TokioFramed::new(stream);
        let display = DisplayControlClient::new(|_| Ok(Vec::new()));
        let mut connector = connector::ClientConnector::new(config(&credential), address)
            .with_static_channel(DrdynvcClient::new().with_dynamic_channel(display));
        let upgrade = ironrdp_tokio::connect_begin(&mut framed, &mut connector).await?;
        let (stream, cert) =
            ironrdp_tls::upgrade_with_certificate_validation_callback_for_endpoint(
                framed.into_inner_no_leftover(),
                &node.host,
                &endpoint,
                callback,
            )
            .await
            .context("RDP-Zertifikat konnte nicht geprüft werden")?;
        // An explicit certificate pin also applies when a replacement has a valid CA chain.
        // Check before CredSSP can send credentials, and persist a new pin only after login.
        if let Some(expected) = review_pin.as_ref() {
            let current = fingerprint(
                stream
                    .get_ref()
                    .1
                    .peer_certificates()
                    .and_then(|chain| chain.first())
                    .context("RDP-Zertifikat fehlt")?
                    .as_ref(),
            );
            let accepted = &current == expected;
            *observed.lock().unwrap_or_else(|e| e.into_inner()) = Some((current, accepted));
            ensure!(accepted, "RDP-Zertifikat hat sich geändert");
        }
        vault_action(state, |vault| {
            ensure_connection_context(state, vault, &node, &credential, epoch, &origin)
        })
        .map_err(anyhow::Error::msg)?;
        let public_key = ironrdp_tls::extract_tls_server_public_key(&cert)
            .context("RDP-Zertifikat enthält keinen öffentlichen Schlüssel")?
            .to_vec();
        let upgraded = ironrdp_tokio::mark_as_upgraded(upgrade, &mut connector);
        let mut framed = TokioFramed::new(stream);
        let result = ironrdp_tokio::connect_finalize(
            upgraded,
            connector,
            &mut framed,
            &mut ironrdp_tokio::reqwest::ReqwestNetworkClient::new(),
            node.host.clone().into(),
            public_key,
            None,
        )
        .await?
        .into_connected()?;
        size(result.desktop_size.width, result.desktop_size.height)?;
        fast_input(result.input_flags)?;
        Ok::<_, anyhow::Error>((framed, result))
    };
    let connected = tokio::time::timeout(Duration::from_secs(45), handshake).await;
    let observed = observed.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let (framed, result) = match connected {
        Ok(Ok(connected)) => connected,
        failure => {
            vault_action(state, |vault| {
                ensure_connection_context(state, vault, &node, &credential, epoch, &origin)
            })?;
            if let Some((cert, false)) = observed {
                // A rejected certificate is surfaced before sending NLA credentials.
                if known.as_ref() != Some(&cert) {
                    return Ok(ConnectResult::HostKey {
                        fingerprint: cert,
                        previous: known,
                        host: format!("{}:{}", node.host, node.port),
                        certificate: true,
                    });
                }
            }
            return Err(match failure {
                Ok(Err(error)) => format!("RDP-Anmeldung fehlgeschlagen: {error:#}"),
                _ => "RDP-Verbindungsaufbau hat das Zeitlimit von 45 Sekunden überschritten".into(),
            });
        }
    };
    vault_action(state, |vault| {
        ensure_connection_context(state, vault, &node, &credential, epoch, &origin)?;
        if let Some((observed, _)) = observed {
            vault.change(|data| {
                data.known_hosts.insert(endpoint, observed);
                Ok(())
            })?;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let (input, receiver) = mpsc::channel(256);
        let task = tauri::async_runtime::spawn(async move {
            let result = run(framed, result, receiver).await;
            let message = result.map_or_else(
                |e| format!("RDP-Sitzung beendet: {e:#}"),
                |_| "Remote-Sitzung beendet".into(),
            );
            let _ = output.send(TerminalEvent::Closed { message });
        });
        state
            .connections
            .lock()
            .map_err(|_| anyhow::anyhow!("Sitzungen nicht verfügbar"))?
            .insert(
                id.clone(),
                Connection {
                    health: None,
                    quick: quick_connections::draft(&origin, &node, &credential),
                    terminal: None,
                    tools: None,
                    remote: None,
                    rdp: Some(Handle { input, task }),
                },
            );
        Ok(ConnectResult::Connected {
            id,
            home: String::new(),
            username: credential.username.clone(),
        })
    })
}

fn operations(
    input: Input,
    database: &mut Database,
    width: u16,
    height: u16,
) -> anyhow::Result<Vec<ironrdp_pdu::input::fast_path::FastPathInputEvent>> {
    let operations = match input {
        Input::Key { code, down } => {
            ensure!(code <= 0x1ff && code & 0xff != 0, "Ungültige RDP-Taste");
            vec![if down {
                Operation::KeyPressed(code.into())
            } else {
                Operation::KeyReleased(code.into())
            }]
        }
        Input::Text { text } => {
            ensure!(
                text.len() <= 4096 && !text.contains('\0'),
                "RDP-Texteingabe ist zu groß oder ungültig"
            );
            text.chars()
                .flat_map(|c| {
                    [
                        Operation::UnicodeKeyPressed(c),
                        Operation::UnicodeKeyReleased(c),
                    ]
                })
                .collect()
        }
        Input::Move { x, y } => vec![Operation::MouseMove(MousePosition {
            x: x.min(width - 1),
            y: y.min(height - 1),
        })],
        Input::Button { button, down, x, y } => {
            let button = MouseButton::from_web_button(button).context("Ungültige RDP-Maustaste")?;
            vec![
                Operation::MouseMove(MousePosition {
                    x: x.min(width - 1),
                    y: y.min(height - 1),
                }),
                if down {
                    Operation::MouseButtonPressed(button)
                } else {
                    Operation::MouseButtonReleased(button)
                },
            ]
        }
        Input::Wheel { delta, horizontal } => vec![Operation::WheelRotations(WheelRotations {
            is_vertical: !horizontal,
            rotation_units: delta.clamp(-120, 120),
        })],
        Input::Release => return Ok(database.release_all().into_vec()),
        Input::SecureAttention => vec![
            Operation::KeyPressed(0x1du16.into()),
            Operation::KeyPressed(0x38u16.into()),
            Operation::KeyPressed(0x153u16.into()),
            Operation::KeyReleased(0x153u16.into()),
            Operation::KeyReleased(0x38u16.into()),
            Operation::KeyReleased(0x1du16.into()),
        ],
        _ => Vec::new(),
    };
    Ok(database.apply(operations).into_vec())
}

struct Desktop {
    image: DecodedImage,
    stage: ActiveStage,
    activation: connector::connection_activation::ConnectionActivationFactory,
    dirty: Damage,
    cursor: Cursor,
    cursor_dirty: bool,
}
impl Desktop {
    async fn outputs(
        &mut self,
        framed: &mut Framed,
        outputs: Vec<ActiveStageOutput>,
    ) -> anyhow::Result<bool> {
        let regions = self.stage.take_damage_regions();
        let exact_damage = !regions.is_empty();
        for rect in regions {
            self.dirty.mark(rect);
        }
        for output in outputs {
            match output {
                ActiveStageOutput::ResponseFrame(bytes) => {
                    tokio::time::timeout(Duration::from_secs(20), framed.write_all(&bytes))
                        .await??
                }
                ActiveStageOutput::GraphicsUpdate(rect) => {
                    if !exact_damage {
                        self.dirty.mark(rect);
                    }
                }
                ActiveStageOutput::PointerDefault => {
                    self.cursor_dirty |= self.cursor.update(Cursor::Default);
                }
                ActiveStageOutput::PointerHidden => {
                    self.cursor_dirty |= self.cursor.update(Cursor::Hidden);
                }
                ActiveStageOutput::PointerBitmap(pointer) => {
                    self.cursor_dirty |= self.cursor.update(Cursor::bitmap(pointer));
                }
                ActiveStageOutput::Terminate(_) => return Ok(false),
                ActiveStageOutput::DeactivateAll => {
                    // A display reactivation stays on this authenticated transport.
                    // It is distinct from automatic reconnect or server redirection.
                    use connector::{connection_activation::ConnectionActivationState, Sequence};
                    let mut sequence = self.activation.create();
                    let mut buffer = ironrdp_core::WriteBuf::new();
                    tokio::time::timeout(Duration::from_secs(20), async {
                        while !sequence.state().is_terminal() {
                            ironrdp_tokio::single_sequence_step(framed, &mut sequence, &mut buffer)
                                .await?;
                        }
                        Ok::<_, anyhow::Error>(())
                    })
                    .await??;
                    if let ConnectionActivationState::Finalized {
                        desktop_size,
                        share_id,
                        static_channel_chunk_size,
                        enable_server_pointer,
                        pointer_software_rendering,
                        input_flags,
                        ..
                    } = sequence.connection_activation_state()
                    {
                        size(desktop_size.width, desktop_size.height)?;
                        fast_input(input_flags)?;
                        ensure!(
                            self.stage.reactivate(
                                sequence.io_channel_id(),
                                sequence.user_channel_id(),
                                share_id,
                                enable_server_pointer,
                                pointer_software_rendering,
                                static_channel_chunk_size
                            ),
                            "RDP-Bildschirm konnte nicht neu initialisiert werden"
                        );
                        self.image = DecodedImage::new(
                            PixelFormat::RgbA32,
                            desktop_size.width,
                            desktop_size.height,
                        );
                        self.dirty = Damage::new(self.image.width(), self.image.height());
                        self.dirty.all();
                        self.cursor_dirty = true;
                    } else {
                        anyhow::bail!("Der RDP-Server hat die Sitzung umgeleitet. Bitte das neue Ziel ausdrücklich verbinden.");
                    }
                }
                _ => {}
            }
        }
        Ok(true)
    }
}
fn fast_input(flags: ironrdp_pdu::rdp::capability_sets::InputFlags) -> anyhow::Result<()> {
    use ironrdp_pdu::rdp::capability_sets::InputFlags;
    ensure!(
        flags.intersects(InputFlags::FASTPATH_INPUT | InputFlags::FASTPATH_INPUT_2),
        "Der RDP-Server unterstützt keine Fast-Path-Eingaben"
    );
    Ok(())
}
async fn run(
    mut framed: Framed,
    result: connector::ConnectionResult,
    mut receiver: mpsc::Receiver<Message>,
) -> anyhow::Result<()> {
    let image = DecodedImage::new(
        PixelFormat::RgbA32,
        result.desktop_size.width,
        result.desktop_size.height,
    );
    let mut dirty = Damage::new(image.width(), image.height());
    dirty.all();
    let mut desktop = Desktop {
        image,
        dirty,
        cursor: Cursor::Default,
        cursor_dirty: true,
        activation: result.activation_factory,
        stage: ActiveStageBuilder {
            static_channels: result.static_channels,
            user_channel_id: result.user_channel_id,
            io_channel_id: result.io_channel_id,
            message_channel_id: result.message_channel_id,
            share_id: result.share_id,
            compression_type: result.compression_type,
            enable_server_pointer: result.enable_server_pointer,
            pointer_software_rendering: false,
        }
        .build(),
    };
    let mut database = Database::new();
    let mut output: Option<Channel<Response>> = None;
    let mut in_flight = VecDeque::with_capacity(MAX_IN_FLIGHT);
    let mut sequence = 0u32;
    let mut visible = true;
    let mut next_frame = tokio::time::Instant::now();
    let mut pending_resize: Option<ResizeRequest> = None;
    loop {
        tokio::select! {
            message = receiver.recv() => {
                let input = match message {
                    None => return Ok(()),
                    Some(Message::Subscribe(channel)) => {
                        output = Some(channel);
                        desktop.dirty.all();
                        desktop.cursor_dirty = true;
                        in_flight.clear();
                        None
                    }
                    Some(Message::Resize(request)) => {
                        if let Some(previous) = pending_resize.replace(request) {
                            let _ = previous.reply.send(Err("Eine neuere Größenanpassung wurde angefordert".into()));
                        }
                        None
                    }
                    Some(Message::Input(Input::Ack { sequence })) => {
                        if let Some(index) = in_flight.iter().position(|&pending| pending == sequence) {
                            in_flight.remove(index);
                        }
                        None
                    }
                    Some(Message::Input(Input::Visible { visible: next })) => {
                        let changed = visible != next;
                        visible = next;
                        if next {
                            if changed {
                                desktop.dirty.all();
                                desktop.cursor_dirty = true;
                            }
                            None
                        } else { Some(Input::Release) }
                    }
                    Some(Message::Input(input)) => Some(input),
                };
                if let Some(input) = input {
                    let events = operations(input, &mut database, desktop.image.width(), desktop.image.height())?;
                    let replies = desktop.stage.process_fastpath_input(&mut desktop.image, &events)?;
                    if !desktop.outputs(&mut framed, replies).await? { return Ok(()); }
                }
            }
            pdu = framed.read_pdu() => {
                let (action, payload) = pdu?;
                let replies = desktop.stage.process(&mut desktop.image, action, &payload, framed.last_read_at())?;
                if !desktop.outputs(&mut framed, replies).await? { return Ok(()); }
            }
            _ = tokio::time::sleep_until(pending_resize.as_ref().map_or(next_frame, |r| r.deadline)), if pending_resize.is_some() => {
                if let Some(request) = pending_resize.take() {
                    let _ = request.reply.send(Err("Der RDP-Server unterstützt die dynamische Auflösung nicht oder ist noch nicht bereit. Die Bildskalierung bleibt verfügbar.".into()));
                }
            }
            _ = tokio::time::sleep_until(next_frame),
                if (!desktop.dirty.is_empty() || desktop.cursor_dirty) && visible && output.is_some() && in_flight.len() < MAX_IN_FLIGHT =>
            {
                sequence = sequence.wrapping_add(1);
                let rectangles = desktop.dirty.take();
                let cursor = desktop.cursor_dirty.then_some(&desktop.cursor);
                let bytes = frame(&desktop.image, &rectangles, sequence, cursor);
                output.as_ref().unwrap().send(Response::new(bytes))?;
                desktop.cursor_dirty = false;
                in_flight.push_back(sequence);
                next_frame = tokio::time::Instant::now() + FRAME_INTERVAL;
            }
        }
        if pending_resize.as_ref().is_some_and(|r| r.reply.is_closed()) {
            pending_resize = None;
        }
        if desktop.stage.display_control_ready() == Some(true) {
            if let Some(request) = pending_resize.take() {
                if request.reply.is_closed() {
                    continue;
                }
                if request.deadline <= tokio::time::Instant::now() {
                    let _ = request.reply.send(Err(
                        "RDP-Größenanpassung hat das Zeitlimit überschritten".into(),
                    ));
                    continue;
                }
                let encoded = desktop.stage.encode_resize(
                    u32::from(request.width),
                    u32::from(request.height),
                    None,
                    None,
                );
                match encoded {
                    Some(Ok(bytes)) => {
                        let result =
                            tokio::time::timeout_at(request.deadline, framed.write_all(&bytes))
                                .await;
                        match result {
                            Ok(Ok(())) => {
                                let _ = request.reply.send(Ok(()));
                            }
                            failure => {
                                let _ = request.reply.send(Err(
                                    "RDP-Größenanpassung konnte nicht gesendet werden".into(),
                                ));
                                anyhow::bail!(
                                    "RDP-Größenanpassung konnte nicht gesendet werden: {failure:?}"
                                );
                            }
                        }
                    }
                    Some(Err(error)) => {
                        let _ = request
                            .reply
                            .send(Err(format!("RDP-Größenanpassung fehlgeschlagen: {error}")));
                    }
                    None => {
                        let _ = request
                            .reply
                            .send(Err("Dynamische RDP-Auflösung ist nicht verfügbar".into()));
                    }
                }
            }
        }
    }
}
#[tauri::command]
pub async fn rdp_resize(
    state: State<'_, AppState>,
    id: String,
    width: u16,
    height: u16,
) -> Result<()> {
    resize_size(width, height).map_err(|e| e.to_string())?;
    let (reply, receive) = oneshot::channel();
    vault_action(&state, |_| {
        let connections = state
            .connections
            .lock()
            .map_err(|_| anyhow::anyhow!("Sitzungen nicht verfügbar"))?;
        connections
            .get(&id)
            .and_then(|c| c.rdp.as_ref())
            .context("RDP-Sitzung geschlossen")?
            .input
            .try_send(Message::Resize(ResizeRequest {
                width,
                height,
                reply,
                deadline: tokio::time::Instant::now() + Duration::from_secs(5),
            }))
            .context("RDP-Eingabepuffer ist voll")?;
        Ok(())
    })?;
    tokio::time::timeout(Duration::from_secs(6), receive)
        .await
        .map_err(|_| "RDP-Größenanpassung hat das Zeitlimit überschritten".to_string())?
        .map_err(|_| "RDP-Sitzung geschlossen".to_string())?
}
#[tauri::command]
pub fn rdp_subscribe(
    state: State<'_, AppState>,
    id: String,
    output: Channel<Response>,
) -> Result<()> {
    vault_action(&state, |_| {
        let connections = state
            .connections
            .lock()
            .map_err(|_| anyhow::anyhow!("Sitzungen nicht verfügbar"))?;
        connections
            .get(&id)
            .and_then(|c| c.rdp.as_ref())
            .context("RDP-Sitzung geschlossen")?
            .input
            .try_send(Message::Subscribe(output))
            .context("RDP-Eingabepuffer ist voll")?;
        Ok(())
    })
}
#[tauri::command]
pub fn rdp_input(state: State<'_, AppState>, id: String, input: Input) -> Result<()> {
    vault_action(&state, |_| {
        let connections = state
            .connections
            .lock()
            .map_err(|_| anyhow::anyhow!("Sitzungen nicht verfügbar"))?;
        connections
            .get(&id)
            .and_then(|c| c.rdp.as_ref())
            .context("RDP-Sitzung geschlossen")?
            .input
            .try_send(Message::Input(input))
            .context("RDP-Eingabepuffer ist voll")?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn credential() -> Credential {
        Credential {
            id: "rdp-user".into(),
            name: "Test account".into(),
            username: "noryn-test-user".into(),
            kind: "password".into(),
            secret: String::new(),
            password: "noryn-test-password".into(),
            passphrase: String::new(),
            team_id: None,
        }
    }
    #[test]
    fn rdp_requires_nla_separates_domain_and_never_uses_a_private_key_as_password() {
        let mut login = credential();
        login.username = "EXAMPLE\\user".into();
        login.secret = "test-only-private-key".into();
        login.passphrase = "test-only-key-passphrase".into();
        login.kind = "key".into();
        let config = config(&login);
        assert!(
            !config.pointer_software_rendering,
            "the mouse cursor must not wait for framebuffer round trips"
        );
        assert!(
            config.enable_credssp && !config.enable_tls && !config.enable_standard_rdp_security
        );
        assert!(
            !config.enable_audio_capture
                && !config.enable_audio_playback
                && !config.remote_application_mode
        );
        assert_eq!(config.domain.as_deref(), Some("EXAMPLE"));
        match config.credentials {
            Credentials::UsernamePassword { username, password } => {
                assert_eq!(username, "user");
                assert_eq!(password, "noryn-test-password");
            }
            _ => panic!("unexpected credential type"),
        }
        assert_ne!(fingerprint(b"first cert"), fingerprint(b"different cert"));
    }
    #[test]
    fn input_bounds_release_and_frame_damage_are_validated() {
        let mut database = Database::new();
        assert_eq!(
            operations(
                Input::Key {
                    code: 0x1e,
                    down: true
                },
                &mut database,
                10,
                10
            )
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            operations(
                Input::Key {
                    code: 0x1e,
                    down: true
                },
                &mut database,
                10,
                10
            )
            .unwrap()
            .len(),
            2,
            "held keys must repeat"
        );
        assert_eq!(
            operations(Input::Release, &mut database, 10, 10)
                .unwrap()
                .len(),
            1
        );
        assert!(operations(Input::Release, &mut database, 10, 10)
            .unwrap()
            .is_empty());
        assert!(operations(
            Input::Key {
                code: 0x200,
                down: true
            },
            &mut database,
            10,
            10
        )
        .is_err());
        assert!(operations(
            Input::Button {
                button: 99,
                down: true,
                x: 0,
                y: 0,
            },
            &mut database,
            10,
            10
        )
        .is_err());
        assert!(operations(
            Input::Text {
                text: "x".repeat(4097)
            },
            &mut database,
            10,
            10
        )
        .is_err());
        assert!(size(u16::MAX, u16::MAX).is_err());
        assert!(resize_size(199, 600).is_err());
        assert!(resize_size(801, 600).is_err());
        assert!(resize_size(4000, 2000).is_err());
        assert!(resize_size(800, 600).is_ok());
    }
    #[tokio::test]
    #[ignore = "Run .venv-ftp/bin/python scripts/test-rdp.py against the disposable loopback TLS/NLA fixture"]
    async fn actual_tls_nla_graphics_input_and_lock_cleanup() {
        use tauri::Manager;
        let fixture = std::path::PathBuf::from(std::env::var("NORYN_TEST_RDP_ROOT").unwrap());
        let port = std::fs::read_to_string(fixture.join("port"))
            .unwrap()
            .parse()
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut vault = noryn_core::vault::Vault::open(
            &directory.path().join("test-vault.json"),
            "temporary-test-password",
        )
        .unwrap();
        let node = Node {
            id: "rdp-node".into(),
            parent_id: None,
            workspace: "private".into(),
            kind: "connection".into(),
            name: "Disposable RDP".into(),
            protocol: "rdp".into(),
            host: "127.0.0.1".into(),
            port,
            initial_path: String::new(),
            notes: String::new(),
            tags: vec![],
            favorite: false,
        };
        let login = credential();
        vault
            .change(|data| {
                data.upsert_node(node.clone())?;
                data.upsert_credential(login.clone())?;
                data.bindings.insert(node.id.clone(), login.id.clone());
                Ok(())
            })
            .unwrap();
        let state = AppState::default();
        *state.vault.lock().unwrap() = Some(vault);
        let output = || Channel::new(|_| Ok(()));
        let challenge = connect(
            &state,
            node.clone(),
            login.clone(),
            quick_connections::Origin::Saved,
            0,
            None,
            output(),
        )
        .await
        .unwrap();
        let cert = match challenge {
            ConnectResult::HostKey {
                fingerprint,
                certificate,
                ..
            } => {
                assert!(certificate);
                fingerprint
            }
            _ => panic!("an unknown self-signed certificate must be reviewed before NLA"),
        };
        assert!(state.connections.lock().unwrap().is_empty());
        let mut wrong = login.clone();
        wrong.password = "wrong-test-password".into();
        vault_action(&state, |vault| {
            vault.change(|data| data.upsert_credential(wrong.clone()))
        })
        .unwrap();
        assert!(
            connect(
                &state,
                node.clone(),
                wrong,
                quick_connections::Origin::Saved,
                0,
                Some(cert.clone()),
                output()
            )
            .await
            .is_err(),
            "a password failure must not become another certificate prompt"
        );
        vault_action(&state, |vault| {
            vault.change(|data| data.upsert_credential(login.clone()))
        })
        .unwrap();
        let endpoint = format!("rdp://127.0.0.1:{port}");
        vault_action(&state, |vault| {
            assert!(
                !vault.data.known_hosts.contains_key(&endpoint),
                "failed authentication must not store a certificate exception"
            );
            vault.change(|data| {
                data.known_hosts
                    .insert(endpoint.clone(), "retired-test-certificate".into());
                Ok(())
            })
        })
        .unwrap();
        let changed = connect(
            &state,
            node.clone(),
            login.clone(),
            quick_connections::Origin::Saved,
            0,
            None,
            output(),
        )
        .await
        .unwrap();
        assert!(
            matches!(changed, ConnectResult::HostKey { certificate: true, previous: Some(previous), .. } if previous == "retired-test-certificate")
        );
        assert!(state.connections.lock().unwrap().is_empty());
        let connected = connect(
            &state,
            node,
            login,
            quick_connections::Origin::Saved,
            0,
            Some(cert.clone()),
            output(),
        )
        .await
        .unwrap();
        vault_action(&state, |vault| {
            assert_eq!(vault.data.known_hosts.get(&endpoint), Some(&cert));
            Ok(())
        })
        .unwrap();
        let id = match connected {
            ConnectResult::Connected { id, .. } => id,
            _ => panic!("RDP should connect after explicit certificate review"),
        };
        let sender = state.connections.lock().unwrap()[&id]
            .rdp
            .as_ref()
            .unwrap()
            .input
            .clone();
        let (frames, mut received) = mpsc::unbounded_channel();
        sender
            .try_send(Message::Subscribe(Channel::new(move |body| {
                if let tauri::ipc::InvokeResponseBody::Raw(bytes) = body {
                    let _ = frames.send(bytes);
                }
                Ok(())
            })))
            .unwrap();
        let mut local_cursor = false;
        let rendered = tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let bytes = received.recv().await.unwrap();
                assert!(bytes.len() >= 24);
                let sequence = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
                let kind = u16::from_le_bytes(bytes[14..16].try_into().unwrap());
                local_cursor |= kind == 3;
                if packet_has_blue(&bytes) {
                    break sequence;
                }
                sender
                    .try_send(Message::Input(Input::Ack { sequence }))
                    .unwrap();
            }
        })
        .await
        .expect("real compressed server graphics must reach the binary frame channel");
        assert!(
            local_cursor,
            "server cursor shapes must reach the local cursor renderer"
        );
        tokio::time::sleep(Duration::from_millis(350)).await;
        let mut held = vec![rendered];
        while let Ok(bytes) = received.try_recv() {
            held.push(u32::from_le_bytes(bytes[4..8].try_into().unwrap()));
        }
        assert_eq!(
            held.len(),
            MAX_IN_FLIGHT,
            "display packets must stay bounded while the renderer stalls"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            received.try_recv().is_err(),
            "a stalled renderer must never accumulate more packets"
        );
        for sequence in held.into_iter().rev() {
            sender
                .try_send(Message::Input(Input::Ack { sequence }))
                .unwrap();
        }
        let start = tokio::time::Instant::now();
        let mut count = 0;
        let mut payload = 0;
        while start.elapsed() < Duration::from_secs(1) {
            let bytes = tokio::time::timeout(Duration::from_secs(1), received.recv())
                .await
                .unwrap()
                .unwrap();
            count += 1;
            payload += bytes.len();
            let sequence = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
            sender
                .try_send(Message::Input(Input::Ack { sequence }))
                .unwrap();
        }
        eprintln!(
            "RDP sustained local updates: {count} packets, {payload} bytes in {:?}",
            start.elapsed()
        );
        assert!(
            count >= 30,
            "the old 20 FPS presentation bottleneck must not return: {count} packets"
        );
        sender
            .try_send(Message::Input(Input::Key {
                code: 0x1e,
                down: true,
            }))
            .unwrap();
        sender
            .try_send(Message::Input(Input::Button {
                button: 0,
                down: true,
                x: 40,
                y: 50,
            }))
            .unwrap();
        sender
            .try_send(Message::Input(Input::Visible { visible: false }))
            .unwrap();
        sender
            .try_send(Message::Input(Input::Move { x: 20, y: 30 }))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let log = std::fs::read_to_string(fixture.join("input.log")).unwrap_or_default();
                if log.contains("Pressed") && log.contains("Released") && log.contains("mouse") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("input must reach the real RDP server");
        // Flush already sent packets; hidden tabs must not keep sending image data.
        while let Ok(bytes) = received.try_recv() {
            let sequence = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
            sender
                .try_send(Message::Input(Input::Ack { sequence }))
                .unwrap();
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(100), received.recv())
                .await
                .is_err()
        );
        sender
            .try_send(Message::Input(Input::Visible { visible: true }))
            .unwrap();
        let refreshed = tokio::time::timeout(Duration::from_secs(1), received.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            refreshed.len() >= 640 * 480 * 4,
            "returning to the tab requires a fresh full image"
        );
        let app = tauri::test::mock_app();
        app.manage(state.clone());
        let sequence = u32::from_le_bytes(refreshed[4..8].try_into().unwrap());
        sender
            .try_send(Message::Input(Input::Ack { sequence }))
            .unwrap();
        for (width, height) in [(1024, 768), (800, 600)] {
            rdp_resize(app.state(), id.clone(), width, height)
                .await
                .expect("dynamic resolution must use the existing authenticated transport");
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let bytes = received.recv().await.unwrap();
                    let sequence = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
                    sender
                        .try_send(Message::Input(Input::Ack { sequence }))
                        .unwrap();
                    if u16::from_le_bytes(bytes[8..10].try_into().unwrap()) == width
                        && u16::from_le_bytes(bytes[10..12].try_into().unwrap()) == height
                    {
                        break;
                    }
                }
            })
            .await
            .expect("remote resize must negotiate a new framebuffer without reconnecting");
            assert_eq!(state.connections.lock().unwrap().len(), 1);
        }
        let (reply, expired) = oneshot::channel();
        sender
            .try_send(Message::Resize(ResizeRequest {
                width: 1200,
                height: 800,
                reply,
                deadline: tokio::time::Instant::now() - Duration::from_secs(1),
            }))
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), expired)
                .await
                .unwrap()
                .unwrap()
                .is_err(),
            "expired resize requests must never be sent later"
        );
        crate::vault_lock(app.state()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), sender.closed())
            .await
            .unwrap();
        assert!(state.connections.lock().unwrap().is_empty());
        assert!(state.vault.lock().unwrap().is_none());
    }
    fn packet_has_blue(bytes: &[u8]) -> bool {
        assert_eq!(&bytes[..4], b"NRD2");
        let u16_at = |offset| u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
        let mut offset = 24;
        if u16_at(14) == 3 {
            offset += usize::from(u16_at(16)) * usize::from(u16_at(18)) * 4;
        }
        let mut blue = false;
        for _ in 0..u16_at(12) {
            let pixels = usize::from(u16_at(offset + 4)) * usize::from(u16_at(offset + 6)) * 4;
            offset += 8;
            blue |= bytes[offset..offset + pixels]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[2] > p[0] && p[2] > 80);
            offset += pixels;
        }
        assert_eq!(offset, bytes.len());
        blue
    }
}

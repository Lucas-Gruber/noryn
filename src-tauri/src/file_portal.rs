//! Writable file-descriptor handoff, with an acknowledged portal response.
use anyhow::{ensure, Context, Result};
use gio::{
    glib::{
        self,
        variant::{Handle, ObjectPath, ToVariant},
    },
    prelude::*,
};
use noryn_core::transfer::Control;
use std::{
    collections::HashMap,
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};
const DESKTOP: &str = "org.freedesktop.portal.Desktop";
const ROOT: &str = "/org/freedesktop/portal/desktop";
const REQUEST: &str = "org.freedesktop.portal.Request";
pub fn launch(path: &Path, control: &Control) -> Result<crate::external_editor::Launch> {
    let context = glib::MainContext::new();
    context.with_thread_default(|| {
        let bus=gio::bus_get_sync(gio::BusType::Session,None::<&gio::Cancellable>).context("Desktop-Portal nicht erreichbar")?;
        open(&bus,&context,path,control,Duration::from_secs(120))?;
        let mut result=crate::external_editor::Launch::manual();
        result.note="Datei über das Desktop-Portal im Standardprogramm geöffnet. Im Editor speichern und schließen, dann ‚Bearbeitung beenden‘ wählen. Das Portal meldet keinen Dokumentabschluss.".into();
        Ok(result)
    })?
}
fn open(
    bus: &gio::DBusConnection,
    context: &glib::MainContext,
    path: &Path,
    control: &Control,
    timeout: Duration,
) -> Result<()> {
    control.check()?;
    let file = noryn_core::transfer::open_regular(path)?;
    let fds = gio::UnixFDList::new();
    let index = fds.append(file)?;
    let token = format!("noryn_{}", uuid::Uuid::new_v4().simple());
    let options = HashMap::from([
        ("handle_token", token.to_variant()),
        ("writable", true.to_variant()),
        ("ask", false.to_variant()),
    ]);
    let (send, receive) = mpsc::channel();
    // Subscribe before OpenFile; a fast response may precede the method reply.
    let subscription = bus.signal_subscribe(
        Some(DESKTOP),
        Some(REQUEST),
        Some("Response"),
        None,
        None,
        gio::DBusSignalFlags::NONE,
        move |_, _, path, _, _, value| {
            if let Some((code, _)) = value.get::<(u32, HashMap<String, glib::Variant>)>() {
                let _ = send.send((path.to_owned(), code));
            }
        },
    );
    let result = (|| {
        // Gio 0.18's convenience call incorrectly assumes a returned FD list.
        // OpenFile replies contain only an object path, so use the message API.
        let message = gio::DBusMessage::new_method_call(
            Some(DESKTOP),
            ROOT,
            Some("org.freedesktop.portal.OpenURI"),
            "OpenFile",
        );
        message.set_body(&("", Handle(index), options).to_variant());
        message.set_unix_fd_list(Some(&fds));
        let (reply, _) = bus
            .send_message_with_reply_sync(
                &message,
                gio::DBusSendMessageFlags::NONE,
                5000,
                None::<&gio::Cancellable>,
            )
            .context("Datei konnte nicht an das Desktop-Portal übergeben werden")?;
        ensure!(
            reply.message_type() == gio::DBusMessageType::MethodReturn,
            "Desktop-Portal hat die Dateiübergabe abgelehnt: {}",
            reply.error_name().unwrap_or_default()
        );
        let (handle,) = reply
            .body()
            .and_then(|v| v.get::<(ObjectPath,)>())
            .context("Ungültige Portal-Antwort")?;
        let started = Instant::now();
        let result = (|| loop {
            control.check()?;
            ensure!(
                started.elapsed() < timeout,
                "Zeitlimit beim Öffnen im Desktop-Portal"
            );
            while context.pending() {
                context.iteration(false);
            }
            while let Ok((path, code)) = receive.try_recv() {
                if path == handle.as_str() {
                    return match code {
                        0 => Ok(()),
                        1 => anyhow::bail!("Öffnen im Desktop-Portal abgebrochen"),
                        _ => anyhow::bail!(
                            "Desktop-Portal konnte kein Programm für diese Datei starten"
                        ),
                    };
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        })();
        if control.is_cancelled() || started.elapsed() >= timeout {
            let _ = bus.call_sync(
                Some(DESKTOP),
                handle.as_str(),
                REQUEST,
                "Close",
                None,
                None,
                gio::DBusCallFlags::NONE,
                1000,
                None::<&gio::Cancellable>,
            );
        }
        result
    })();
    bus.signal_unsubscribe(subscription);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Read,
        os::fd::FromRawFd,
        sync::{
            atomic::{AtomicBool, AtomicU32, Ordering},
            Arc,
        },
    };
    #[test]
    #[ignore = "Run only on an isolated bus: dbus-run-session -- env NORYN_TEST_PORTAL=1 cargo test -p noryn --lib file_portal::tests::portal_acknowledges_writable_fds_and_reports_cancellation_failure_and_timeouts -- --ignored --exact"]
    fn portal_acknowledges_writable_fds_and_reports_cancellation_failure_and_timeouts() {
        assert_eq!(
            std::env::var("NORYN_TEST_PORTAL").as_deref(),
            Ok("1"),
            "Never replace the user's desktop portal on the real session bus"
        );
        let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
        let connect = move || {
            gio::DBusConnection::for_address_sync(
                &address,
                gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                    | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                None::<&gio::DBusAuthObserver>,
                None::<&gio::Cancellable>,
            )
            .unwrap()
        };
        let code = Arc::new(AtomicU32::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let (ready, wait) = mpsc::channel();
        let service_code = code.clone();
        let stop = done.clone();
        let service = std::thread::spawn(move || {
            let context = glib::MainContext::new();
            context.with_thread_default(|| {
                let bus=connect();bus.call_sync(Some("org.freedesktop.DBus"),"/org/freedesktop/DBus","org.freedesktop.DBus","RequestName",Some(&(DESKTOP,4u32).to_variant()),None,gio::DBusCallFlags::NONE,1000,None::<&gio::Cancellable>).unwrap();
                let xml=r#"<node><interface name="org.freedesktop.portal.OpenURI"><method name="OpenFile"><arg type="s" direction="in"/><arg type="h" direction="in"/><arg type="a{sv}" direction="in"/><arg type="o" direction="out"/></method></interface></node>"#;
                let info=gio::DBusNodeInfo::for_xml(xml).unwrap();
                let registration=bus.register_object(ROOT,&info.lookup_interface("org.freedesktop.portal.OpenURI").unwrap(),move|bus,sender,_,_,method,args,invocation| {
                    assert_eq!(method,"OpenFile");
                    let (parent,fd,options)=args.get::<(String,Handle,HashMap<String,glib::Variant>)>().unwrap();assert!(parent.is_empty());assert_eq!(options["writable"].get::<bool>(),Some(true));assert_eq!(options["ask"].get::<bool>(),Some(false));
                    let raw=invocation.message().unix_fd_list().unwrap().get(fd.0).unwrap();
                    // GUnixFDList.get returns an owned duplicate, never the sender's descriptor.
                    let mut file=unsafe{std::fs::File::from_raw_fd(raw)};let mut body=String::new();file.read_to_string(&mut body).unwrap();assert_eq!(body,"isolated portal fixture");
                    let token=options["handle_token"].get::<String>().unwrap();let path=format!("/org/freedesktop/portal/desktop/request/test/{token}");
                    let code=service_code.load(Ordering::Acquire);
                    if code<3 {bus.emit_signal(Some(sender),&path,REQUEST,"Response",Some(&(code,HashMap::<String,glib::Variant>::new()).to_variant())).unwrap();}
                    invocation.return_value(Some(&(ObjectPath::try_from(path).unwrap(),).to_variant()));
                },|_,_,_,_,_|false.to_variant(),|_,_,_,_,_,_|false).unwrap();
                ready.send(()).unwrap();while !stop.load(Ordering::Acquire) {while context.pending() {context.iteration(false);}std::thread::sleep(Duration::from_millis(2));}
                bus.unregister_object(registration).unwrap();
            }).unwrap();
        });
        wait.recv().unwrap();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let bus =
                    gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>).unwrap();
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("fixture 'quoted'.txt");
                std::fs::write(&path, "isolated portal fixture").unwrap();
                for response in 0..4 {
                    code.store(response, Ordering::Release);
                    let result = open(
                        &bus,
                        &context,
                        &path,
                        &Control::default(),
                        Duration::from_millis(150),
                    );
                    match response {
                        0 => result.unwrap(),
                        1 => assert!(result.unwrap_err().to_string().contains("abgebrochen")),
                        2 => assert!(result.unwrap_err().to_string().contains("kein Programm")),
                        _ => assert!(result.unwrap_err().to_string().contains("Zeitlimit")),
                    }
                }
                let cancelled = Control::default();
                cancelled.cancel();
                assert!(open(&bus, &context, &path, &cancelled, Duration::from_secs(1)).is_err());
                // A file association is never invoked for a substituted symlink.
                let link = dir.path().join("link.txt");
                std::os::unix::fs::symlink(&path, &link).unwrap();
                assert!(open(
                    &bus,
                    &context,
                    &link,
                    &Control::default(),
                    Duration::from_secs(1)
                )
                .is_err());
                assert_eq!(
                    std::fs::read_to_string(&path).unwrap(),
                    "isolated portal fixture"
                );
            })
            .unwrap();
        done.store(true, Ordering::Release);
        service.join().unwrap();
    }
}

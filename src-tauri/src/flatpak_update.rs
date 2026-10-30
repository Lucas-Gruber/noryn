//! Short-lived Flatpak portal transaction. The host owns package verification and deployment.
use anyhow::{bail, ensure, Context, Result};
use gio::glib::{self, variant::ToVariant};
use std::{
    collections::HashMap,
    sync::mpsc,
    time::{Duration, Instant},
};

const SERVICE: &str = "org.freedesktop.portal.Flatpak";
const ROOT: &str = "/org/freedesktop/portal/Flatpak";
const MONITOR: &str = "org.freedesktop.portal.Flatpak.UpdateMonitor";
type Dict = HashMap<String, glib::Variant>;

/// A monitor is created only for an explicit install, never for idle polling.
pub fn install(cancel: &std::sync::atomic::AtomicBool, progress: impl Fn(u32)) -> Result<()> {
    let context = glib::MainContext::new();
    context.with_thread_default(|| {
        let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
            .context("Flatpak-Portal nicht erreichbar")?;
        transaction(&bus, &context, cancel, progress, Duration::from_secs(600))
    })?
}

fn transaction(
    bus: &gio::DBusConnection,
    context: &glib::MainContext,
    cancel: &std::sync::atomic::AtomicBool,
    progress: impl Fn(u32),
    timeout: Duration,
) -> Result<()> {
    ensure!(
        !cancel.load(std::sync::atomic::Ordering::Acquire),
        "Flatpak-Update abgebrochen"
    );
    let token = format!("noryn_{}", uuid::Uuid::new_v4().simple());
    let sender = bus
        .unique_name()
        .context("Flatpak-Portal ohne Bus-Identität")?
        .trim_start_matches(':')
        .replace('.', "_");
    let path = format!("{ROOT}/update_monitor/{sender}/{token}");
    let (send, receive) = mpsc::channel();
    // Subscribe first: progress is allowed to arrive before Update's reply.
    let subscription = bus.signal_subscribe(
        Some(SERVICE),
        Some(MONITOR),
        Some("Progress"),
        Some(&path),
        None,
        gio::DBusSignalFlags::NONE,
        move |_, _, _, _, _, value| {
            let _ = send.send(value.get::<(Dict,)>());
        },
    );
    let result = (|| {
        let reply = bus
            .call_sync(
                Some(SERVICE),
                ROOT,
                SERVICE,
                "CreateUpdateMonitor",
                Some(&(HashMap::from([("handle_token", token.to_variant())]),).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                5000,
                None::<&gio::Cancellable>,
            )
            .context("Flatpak-Update-Monitor konnte nicht erstellt werden")?;
        let (handle,) = reply
            .get::<(glib::variant::ObjectPath,)>()
            .context("Ungültige Flatpak-Portal-Antwort")?;
        ensure!(handle.as_str() == path, "Unerwarteter Flatpak-Monitor");
        bus.call_sync(
            Some(SERVICE),
            &path,
            MONITOR,
            "Update",
            Some(&("", Dict::new()).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            5000,
            None::<&gio::Cancellable>,
        )
        .context("Flatpak hat die Aktualisierung abgelehnt")?;
        let started = Instant::now();
        loop {
            ensure!(
                !cancel.load(std::sync::atomic::Ordering::Acquire),
                "Flatpak-Update abgebrochen"
            );
            ensure!(started.elapsed() < timeout, "Zeitlimit beim Flatpak-Update");
            while context.pending() {
                context.iteration(false);
            }
            while let Ok(value) = receive.try_recv() {
                let (info,) = value.context("Ungültiger Flatpak-Fortschritt")?;
                match info.get("status").and_then(|v| v.get::<u32>()).unwrap_or(0) {
                    0 => {
                        if let (Some(ops), Some(op), Some(percent)) = (
                            number(&info, "n_ops"), number(&info, "op"), number(&info, "progress")) {
                            if ops > 0 && op < ops && percent <= 100 {
                                progress(((u64::from(op)*100 + u64::from(percent))/u64::from(ops)).min(99) as u32);
                            }
                        }
                    }
                    1 => bail!("Die Flatpak-Paketquelle bietet kein installierbares Update. Falls Discover bereits aktualisiert hat, Noryn schließen und erneut starten. Ältere Einzelpakete einmal über die neue .flatpakref installieren."),
                    2 => { progress(100); return Ok(()); }
                    3 => bail!("Flatpak-Update fehlgeschlagen: {}. Bei zusätzlichen Berechtigungen bitte über Discover oder die Softwareverwaltung aktualisieren.",
                        info.get("error_message").and_then(|v| v.get::<String>()).unwrap_or_else(|| "Unbekannter Portal-Fehler".into()).chars().take(2048).collect::<String>()),
                    _ => bail!("Unbekannter Flatpak-Update-Status"),
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    // Also cancels an outstanding transaction on timeout/failure/cancellation.
    let _ = bus.call_sync(
        Some(SERVICE),
        &path,
        MONITOR,
        "Close",
        None,
        None,
        gio::DBusCallFlags::NONE,
        1000,
        None::<&gio::Cancellable>,
    );
    bus.signal_unsubscribe(subscription);
    result
}
fn number(info: &Dict, name: &str) -> Option<u32> {
    info.get(name)?.get::<u32>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    };
    #[test]
    #[ignore = "Run on a private bus with dbus-run-session and NORYN_TEST_PORTAL=1"]
    fn portal_transaction_handles_progress_failure_cancellation_and_timeout() {
        assert_eq!(
            std::env::var("NORYN_TEST_PORTAL").as_deref(),
            Ok("1"),
            "Use an isolated D-Bus, never the desktop bus"
        );
        let mode = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let closes = Arc::new(AtomicU32::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let (ready, wait) = mpsc::channel();
        let (service_mode, service_cancel, service_closes, stop) =
            (mode.clone(), cancel.clone(), closes.clone(), done.clone());
        let service = std::thread::spawn(move || {
            let context = glib::MainContext::new();
            context.with_thread_default(|| {
                let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>).unwrap();
                bus.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName", Some(&(SERVICE, 4u32).to_variant()), None, gio::DBusCallFlags::NONE, 1000, None::<&gio::Cancellable>).unwrap();
                let xml = r#"<node><interface name="org.freedesktop.portal.Flatpak"><method name="CreateUpdateMonitor"><arg type="a{sv}" direction="in"/><arg type="o" direction="out"/></method></interface><interface name="org.freedesktop.portal.Flatpak.UpdateMonitor"><method name="Update"><arg type="s" direction="in"/><arg type="a{sv}" direction="in"/></method><method name="Close"/></interface></node>"#;
                let info = gio::DBusNodeInfo::for_xml(xml).unwrap();

                let registrations = Arc::new(std::sync::Mutex::new(Vec::new()));
                let keep = registrations.clone();
                let root = bus.register_object(ROOT, &info.lookup_interface(SERVICE).unwrap(), move |bus, sender, _, _, method, args, invocation| {
                    assert_eq!(method, "CreateUpdateMonitor");
                    let (options,) = args.get::<(Dict,)>().unwrap();
                    assert_eq!(options.len(), 1);
                    let token = options["handle_token"].get::<String>().unwrap();
                    let path = format!("{ROOT}/update_monitor/{}/{}", sender.trim_start_matches(':').replace('.', "_"), token);
                    let (mode, cancel, closes) = (service_mode.clone(), service_cancel.clone(), service_closes.clone());
                    let registration = bus.register_object(&path, &gio::DBusNodeInfo::for_xml(xml).unwrap().lookup_interface(MONITOR).unwrap(), move |bus, sender, path, _, method, args, invocation| {
                        if method == "Close" { closes.fetch_add(1, Ordering::Release); invocation.return_value(None); return; }
                        assert_eq!(method, "Update");
                        let (parent, options) = args.get::<(String, Dict)>().unwrap();
                        assert!(parent.is_empty() && options.is_empty());
                        let mode = mode.load(Ordering::Acquire);
                        if mode == 5 { invocation.return_dbus_error("org.freedesktop.DBus.Error.NotSupported", "New permissions require system tools"); return; }
                        if mode == 4 { cancel.store(true, Ordering::Release); }
                        if mode < 3 {
                            // Emit before the method reply, including an operation at 100%
                            // that must not be mistaken for completion of the transaction.
                            for (op, percent, status) in [(0u32, 100u32, 0u32), (1, 50, 0), (1, 100, [2, 1, 3][mode as usize])] {
                                let mut dict = Dict::from([("op".into(), op.to_variant()), ("n_ops".into(), 2u32.to_variant()), ("progress".into(), percent.to_variant()), ("status".into(), status.to_variant())]);
                                if mode == 2 { dict.insert("error_message".into(), "Fixture denied".to_variant()); }
                                bus.emit_signal(Some(sender), path, MONITOR, "Progress", Some(&(dict,).to_variant())).unwrap();
                            }
                        }
                        invocation.return_value(None);
                    }, |_,_,_,_,_| false.to_variant(), |_,_,_,_,_,_| false).unwrap();
                    keep.lock().unwrap().push(registration);
                    invocation.return_value(Some(&(glib::variant::ObjectPath::try_from(path).unwrap(),).to_variant()));
                }, |_,_,_,_,_| false.to_variant(), |_,_,_,_,_,_| false).unwrap();
                ready.send(()).unwrap();
                while !stop.load(Ordering::Acquire) { while context.pending() {context.iteration(false);} std::thread::sleep(Duration::from_millis(2)); }
                for registration in registrations.lock().unwrap().drain(..) { bus.unregister_object(registration).unwrap(); }
                bus.unregister_object(root).unwrap();
            }).unwrap();
        });
        wait.recv().unwrap();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let bus =
                    gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>).unwrap();
                for case in 0..6 {
                    mode.store(case, Ordering::Release);
                    cancel.store(false, Ordering::Release);
                    let output = std::cell::RefCell::new(Vec::new());
                    let result = transaction(
                        &bus,
                        &context,
                        &cancel,
                        |percent| output.borrow_mut().push(percent),
                        Duration::from_millis(150),
                    );
                    match case {
                        0 => {
                            result.unwrap();
                            assert_eq!(*output.borrow(), [50, 75, 100]);
                        }
                        1 => assert!(result
                            .unwrap_err()
                            .to_string()
                            .contains("kein installierbares Update")),
                        2 => assert!(result.unwrap_err().to_string().contains("Fixture denied")),
                        3 => assert!(result.unwrap_err().to_string().contains("Zeitlimit")),
                        4 => assert!(result.unwrap_err().to_string().contains("abgebrochen")),
                        _ => assert!(format!("{:#}", result.unwrap_err()).contains("NotSupported")),
                    }
                    assert_eq!(
                        closes.load(Ordering::Acquire),
                        case + 1,
                        "Every transaction closes its monitor"
                    );
                }
            })
            .unwrap();
        done.store(true, Ordering::Release);
        service.join().unwrap();
    }
}

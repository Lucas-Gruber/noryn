//! One bounded read-only login report, using existing SSH and host telemetry only.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::{collections::HashSet, sync::atomic::AtomicBool, time::Duration};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoginPoint {
    pub identity: String,
    pub server_time: u64,
    pub active_services: Vec<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListeningPort {
    pub protocol: String,
    pub address: String,
    pub port: u16,
    pub process: String,
    pub forward_host: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceHealth {
    pub name: String,
    pub state: String,
    pub description: String,
    pub became_inactive: bool,
}
#[derive(Clone, Serialize)]
pub struct ServiceEvent {
    pub at: u64,
    pub unit: String,
    pub message: String,
}
#[derive(Clone, Serialize)]
pub struct LoadPeak {
    pub at: u64,
    pub load: f64,
}
#[derive(Clone, Serialize)]
pub struct Updates {
    pub manager: String,
    pub total: u32,
    pub security: Option<u32>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginHealth {
    pub captured_at: u64,
    pub uptime: f64,
    pub cores: u32,
    pub previous_login: Option<u64>,
    pub services_available: bool,
    pub journal_available: bool,
    pub ports_available: bool,
    pub services: Vec<ServiceHealth>,
    pub events: Vec<ServiceEvent>,
    pub updates: Option<Updates>,
    pub peaks: Vec<LoadPeak>,
    pub load_samples: u64,
    pub load_window: u64,
    pub ports: Vec<ListeningPort>,
    pub notes: Vec<String>,
    pub active_services: Vec<String>,
}
const SCRIPT: &str = include_str!("login_health.sh");
pub fn validate_logins(data: &crate::model::VaultData) -> Result<()> {
    ensure!(
        data.health_logins.len() <= 20_000,
        "Zu viele lokale Anmeldevergleiche"
    );
    for (id, point) in &data.health_logins {
        ensure!(
            !id.is_empty()
                && id.len() <= 128
                && point.identity.len() <= 2048
                && (1..=253_402_300_799).contains(&point.server_time)
                && point.active_services.len() <= 400
                && point
                    .active_services
                    .iter()
                    .all(|s| s.len() <= 256 && !s.chars().any(char::is_control)),
            "Ungültiger lokaler Anmeldevergleich"
        );
    }
    Ok(())
}
pub fn collect(
    session: &Session,
    closed: &AtomicBool,
    previous: Option<&LoginPoint>,
) -> Result<LoginHealth> {
    let since = previous.map(|p| p.server_time).unwrap_or(0);
    let command = format!(
        "sh -c '{}' noryn-health {since}",
        SCRIPT.replace('\'', "'\\''")
    );
    let text =
        crate::system_info::execute_bounded(session, closed, &command, Duration::from_secs(25))?;
    parse(&text, previous)
}
pub fn ports(session: &Session, closed: &AtomicBool) -> Result<Vec<ListeningPort>> {
    let text = crate::system_info::execute_bounded(
        session,
        closed,
        "LC_ALL=C ss -H -lntup",
        Duration::from_secs(5),
    )?;
    parse_ports(&text)
}
fn parse_ports(text: &str) -> Result<Vec<ListeningPort>> {
    let mut ports = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines().filter(|s| !s.trim().is_empty()).take(512) {
        let fields: Vec<_> = line.split_whitespace().collect();
        ensure!(
            fields.len() >= 6 && ["tcp", "udp"].contains(&fields[0]),
            "Unbekanntes ss-Ausgabeformat"
        );
        let (address, port) = fields[4]
            .rsplit_once(':')
            .ok_or_else(|| anyhow::anyhow!("Ungültiger Listener"))?;
        let port: u16 = port.parse()?;
        let address = address.trim_matches(['[', ']']);
        if port == 0 || !seen.insert((fields[0].to_owned(), address.to_owned(), port)) {
            continue;
        }
        let forward_host = match address {
            "*" | "0.0.0.0" => "127.0.0.1",
            "::" => "::1",
            address => address,
        };
        ports.push(ListeningPort {
            protocol: fields[0].into(),
            address: address.into(),
            port,
            process: fields[6..].join(" ").chars().take(512).collect(),
            forward_host: forward_host.into(),
        });
    }
    ports.sort_by_key(|p| (p.port, p.protocol.clone()));
    Ok(ports)
}
fn parse(text: &str, previous: Option<&LoginPoint>) -> Result<LoginHealth> {
    let mut report = LoginHealth {
        captured_at: 0,
        uptime: 0.,
        cores: 1,
        previous_login: previous.map(|p| p.server_time),
        services_available: false,
        journal_available: false,
        ports_available: false,
        services: vec![],
        events: vec![],
        updates: None,
        peaks: vec![],
        load_samples: 0,
        load_window: 0,
        ports: vec![],
        notes: vec![],
        active_services: vec![],
    };
    let mut section = "";
    let mut port_text = String::new();
    for line in text.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        match fields[0] {
            "NORYN_HEALTH" if fields.len() == 4 => {
                ensure!(report.captured_at == 0, "Doppelter Login-Bericht");
                report.captured_at = fields[1].parse()?;
                report.uptime = fields[2].parse()?;
                report.cores = fields[3].parse()?;
                ensure!(
                    report.captured_at > 0
                        && report.captured_at <= 253_402_300_799
                        && report.uptime.is_finite()
                        && report.uptime >= 0.
                        && report.uptime <= report.captured_at as f64
                        && (1..=65_536).contains(&report.cores),
                    "Ungültige Gesundheitsdaten"
                );
            }
            "NORYN_SECTION" if fields.len() == 2 => section = fields[1],
            "NORYN_NOTE" if fields.len() >= 2 => {
                if report.notes.len() < 20 {
                    report.notes.push(fields[1..].join(" "));
                }
            }
            "NORYN_SERVICES_OK" => report.services_available = true,
            "NORYN_JOURNAL_OK" => report.journal_available = true,
            "NORYN_PORTS_OK" => report.ports_available = true,
            "NORYN_UPDATES" if fields.len() == 4 => {
                let total = fields[2].trim().parse()?;
                let security = if fields[3].trim() == "unknown" {
                    None
                } else {
                    Some(fields[3].trim().parse()?)
                };
                ensure!(
                    security.is_none_or(|n| n <= total),
                    "Ungültige Update-Zahlen"
                );
                report.updates = Some(Updates {
                    manager: fields[1].into(),
                    total,
                    security,
                });
            }
            "NORYN_LOAD_WINDOW" if fields.len() == 2 => report.load_window = fields[1].parse()?,
            "NORYN_LOAD_SAMPLES" if fields.len() == 2 => {
                let samples = fields[1].parse::<u64>()?;
                ensure!(
                    samples <= 1_000_000 && report.load_samples <= 1_000_000 - samples,
                    "Zu viele historische Messpunkte"
                );
                report.load_samples += samples;
            }
            "NORYN_LOAD" if fields.len() == 3 => {
                let at = fields[1].parse()?;
                let load: f64 = fields[2].parse()?;
                ensure!(load.is_finite() && load >= 0., "Ungültiger Load-Wert");
                if report.peaks.len() < 700 {
                    report.peaks.push(LoadPeak { at, load });
                }
            }
            _ => match section {
                "services" => {
                    let f: Vec<_> = line.split_whitespace().collect();
                    if f.len() >= 4
                        && f[0].ends_with(".service")
                        && report.active_services.len() + report.services.len() < 400
                    {
                        if f[2] == "active" {
                            report.active_services.push(f[0].into());
                        } else if ["failed", "inactive"].contains(&f[2]) {
                            report.services.push(ServiceHealth {
                                name: f[0].into(),
                                state: f[2].into(),
                                description: f[4..].join(" "),
                                became_inactive: previous
                                    .is_some_and(|p| p.active_services.iter().any(|s| s == f[0])),
                            });
                        }
                    }
                }
                "journal" if line.starts_with('{') && report.events.len() < 100 => {
                    let event: serde_json::Value = serde_json::from_str(line)?;
                    if event["EXIT_STATUS"].as_str() == Some("0")
                        || event["RESULT"].as_str() == Some("success")
                    {
                        continue;
                    }
                    let unit = event["UNIT"]
                        .as_str()
                        .or_else(|| event["_SYSTEMD_UNIT"].as_str())
                        .unwrap_or("");
                    if unit.ends_with(".service") {
                        if let (Some(timestamp), Some(message)) = (
                            event["__REALTIME_TIMESTAMP"].as_str(),
                            event["MESSAGE"].as_str(),
                        ) {
                            let at = timestamp.parse::<u64>()? / 1_000_000;
                            report.events.push(ServiceEvent {
                                at,
                                unit: unit.chars().take(256).collect(),
                                message: message.chars().take(2048).collect(),
                            });
                        }
                    }
                }
                "ports" => {
                    port_text.push_str(line);
                    port_text.push('\n');
                }
                _ => {}
            },
        }
    }
    ensure!(report.captured_at > 0, "Login-Bericht unvollständig");
    if report
        .previous_login
        .is_some_and(|t| t > report.captured_at)
    {
        report.previous_login = None;
        for service in &mut report.services {
            service.became_inactive = false;
        }
        report.notes.push(
            "Die Serveruhr liegt vor der letzten Anmeldung; Vergleichsbasis wurde verworfen."
                .into(),
        );
    }
    if report.ports_available {
        report.ports = parse_ports(&port_text)?;
    }
    report.peaks.retain(|p| {
        p.at >= report.load_window && p.at <= report.captured_at && p.load > f64::from(report.cores)
    });
    report.peaks.sort_by_key(|p| std::cmp::Reverse(p.at));
    report.peaks.dedup_by_key(|p| p.at);
    report.peaks.truncate(100);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listener_parser_preserves_ipv6_and_does_not_offer_udp_as_tcp() {
        let ports=parse_ports("tcp LISTEN 0 128 [::]:22 [::]:* users:((\"sshd\",pid=1,fd=3))\nudp UNCONN 0 0 0.0.0.0:53 0.0.0.0:*\ntcp LISTEN 0 10 127.0.0.1:5432 0.0.0.0:*\n").unwrap();
        assert_eq!(ports[0].forward_host, "::1");
        assert_eq!(ports[1].protocol, "udp");
        assert_eq!(ports[2].port, 5432);
        assert!(parse_ports("tcp LISTEN 0 0 invalid-port").is_err());
    }
    #[test]
    fn report_distinguishes_missing_history_and_excludes_boot_and_future_peaks() {
        let prior = LoginPoint {
            identity: "test".into(),
            server_time: 1000,
            active_services: vec!["nginx.service".into()],
        };
        let text="NORYN_HEALTH\t2000\t1000.0\t2\nNORYN_SECTION\tservices\nnginx.service loaded inactive dead Web\nsshd.service loaded active running SSH\nNORYN_SERVICES_OK\nNORYN_UPDATES\tapt\t5\t2\nNORYN_LOAD_WINDOW\t1300\nNORYN_LOAD\t1200\t20\nNORYN_LOAD\t1800\t3.5\nNORYN_LOAD\t3000\t30\nNORYN_LOAD_SAMPLES\t12\n";
        let report = parse(text, Some(&prior)).unwrap();
        assert!(report.services[0].became_inactive);
        assert_eq!(report.peaks.len(), 1);
        assert_eq!(report.peaks[0].at, 1800);
        assert_eq!(report.updates.unwrap().security, Some(2));
        assert!(!report.journal_available);
        assert!(parse("NORYN_HEALTH\t2000\tNaN\t2", None).is_err());
    }
}

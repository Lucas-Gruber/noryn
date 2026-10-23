use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::{
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum Section {
    Processes,
    Services,
    Hardware,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Process {
    pub pid: u32,
    pub parent_pid: u32,
    pub user: String,
    pub cpu: f64,
    pub memory_percent: f64,
    pub memory: u64,
    pub name: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub name: String,
    pub load: String,
    pub active: String,
    pub state: String,
    pub description: String,
    pub file_state: String,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuTicks {
    pub total: u64,
    pub idle: u64,
}
#[derive(Clone, Serialize)]
pub struct IoCounter {
    pub name: String,
    pub read: u64,
    pub written: u64,
}
#[derive(Clone, Serialize)]
pub struct NetCounter {
    pub name: String,
    pub received: u64,
    pub sent: u64,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub architecture: String,
    pub cpu_model: String,
    pub logical_cpus: u32,
    pub memory_total: u64,
    pub memory_available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub uptime: f64,
    pub load: Vec<f64>,
    pub disks: Vec<Disk>,
    pub cpu_ticks: CpuTicks,
    pub io: Vec<IoCounter>,
    pub network: Vec<NetCounter>,
}
#[derive(Clone, Serialize)]
pub struct Disk {
    pub name: String,
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub mount: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub section: Section,
    pub captured_at: u64,
    pub processes: Vec<Process>,
    pub services: Vec<Service>,
    pub hardware: Option<Hardware>,
    pub message: Option<String>,
}

const PROCESSES: &str = r#"export LC_ALL=C
[ "$(uname -s)" = Linux ] || { echo 'Die Prozessansicht unterstützt derzeit Linux.' >&2; exit 1; }
ps --version >/dev/null 2>&1 || { echo 'Die Prozessansicht benötigt procps (ps).' >&2; exit 1; }
ps -eo pid=,ppid=,user=,pcpu=,pmem=,rss=,comm= --sort=-pcpu | head -n 100
"#;
const SERVICES: &str = r#"export LC_ALL=C
if ! command -v systemctl >/dev/null 2>&1; then
 printf 'NORYN_UNAVAILABLE\tAuf diesem Ziel ist keine erreichbare systemd-Diensteverwaltung vorhanden.\n'
 exit 0
fi
systemctl --no-pager show -p Version >/dev/null 2>&1 || { printf 'NORYN_UNAVAILABLE\tKeine erreichbare systemd-Diensteverwaltung.\n'; exit 0; }
systemctl --no-pager --plain --no-legend list-units --type=service --all | head -n 400
printf 'NORYN_UNIT_FILES\n'
systemctl --no-pager --no-legend list-unit-files --type=service | head -n 400
"#;
const HARDWARE: &str = r#"export LC_ALL=C
[ "$(uname -s)" = Linux ] && [ -r /proc/meminfo ] || { echo 'Die Hardwareansicht unterstützt derzeit Linux mit /proc.' >&2; exit 1; }
printf 'hostname\t'; uname -n
printf 'kernel\t'; uname -sr
printf 'architecture\t'; uname -m
printf 'os\t'; sed -n 's/^PRETTY_NAME=//p' /etc/os-release 2>/dev/null | tr -d '"'
printf 'cpu\t'; awk -F: '/^model name|^Hardware|^Processor/ {sub(/^[ \t]+/, "", $2); print $2; exit}' /proc/cpuinfo
printf 'cores\t'; getconf _NPROCESSORS_ONLN 2>/dev/null || echo 0
awk '/^(MemTotal:|MemAvailable:|MemFree:|Buffers:|Cached:|SwapTotal:|SwapFree:)/ {printf "%s\t%.0f\n",$1,$2*1024}' /proc/meminfo
printf 'uptime\t'; cat /proc/uptime
printf 'load\t'; cat /proc/loadavg
awk '/^cpu / {total=0;for(i=2;i<=9;i++)total+=$i;printf "cpuTicks\t%.0f\t%.0f\n",total,$5+$6}' /proc/stat
awk '$3 !~ /^(loop|ram)/ {printf "io\t%s\t%.0f\t%.0f\n",$3,$6*512,$10*512}' /proc/diskstats
awk 'NR>2 {gsub(/:/," ");printf "net\t%s\t%s\t%s\n",$1,$2,$10}' /proc/net/dev
df -Pk -l -x tmpfs -x devtmpfs 2>/dev/null | awk 'NR>1 {print "disk\t" $0}' | head -n 40
"#;

pub fn collect(session: &Session, closed: &AtomicBool, section: Section) -> Result<Snapshot> {
    let text = execute(
        session,
        closed,
        match section {
            Section::Processes => PROCESSES,
            Section::Services => SERVICES,
            Section::Hardware => HARDWARE,
        },
    )?;
    let mut snapshot = Snapshot {
        section,
        captured_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        processes: Vec::new(),
        services: Vec::new(),
        hardware: None,
        message: None,
    };
    match section {
        Section::Processes => {
            for line in text.lines().take(100) {
                let fields: Vec<_> = line.split_whitespace().collect();
                ensure!(
                    fields.len() >= 7,
                    "Prozessdaten konnten nicht gelesen werden"
                );
                snapshot.processes.push(Process {
                    pid: fields[0].parse()?,
                    parent_pid: fields[1].parse()?,
                    user: fields[2].into(),
                    cpu: fields[3].parse()?,
                    memory_percent: fields[4].parse()?,
                    memory: fields[5].parse::<u64>()?.saturating_mul(1024),
                    name: fields[6..].join(" "),
                });
            }
        }
        Section::Services => {
            let mut files = false;
            for line in text.lines().take(802) {
                if let Some(message) = line.strip_prefix("NORYN_UNAVAILABLE\t") {
                    snapshot.message = Some(message.into());
                    break;
                }
                if line == "NORYN_UNIT_FILES" {
                    files = true;
                    continue;
                }
                let fields: Vec<_> = line.split_whitespace().collect();
                if files {
                    if fields.len() < 2 || !fields[0].ends_with(".service") {
                        continue;
                    }
                    if let Some(service) =
                        snapshot.services.iter_mut().find(|s| s.name == fields[0])
                    {
                        service.file_state = fields[1].into();
                    } else if snapshot.services.len() < 400 {
                        snapshot.services.push(Service {
                            name: fields[0].into(),
                            load: "not-loaded".into(),
                            active: "inactive".into(),
                            state: "not-loaded".into(),
                            description: String::new(),
                            file_state: fields[1].into(),
                        });
                    }
                } else if fields.len() >= 4 {
                    snapshot.services.push(Service {
                        name: fields[0].into(),
                        load: fields[1].into(),
                        active: fields[2].into(),
                        state: fields[3].into(),
                        description: fields[4..].join(" "),
                        file_state: "unknown".into(),
                    });
                }
            }
        }
        Section::Hardware => snapshot.hardware = Some(parse_hardware(&text)?),
    }
    Ok(snapshot)
}

fn parse_hardware(text: &str) -> Result<Hardware> {
    let mut info = Hardware::default();
    let (mut free, mut buffers, mut cached) = (0_u64, 0_u64, 0_u64);
    let mut has_available = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('\t') else {
            continue;
        };
        match key {
            "hostname" => info.hostname = value.into(),
            "kernel" => info.kernel = value.into(),
            "architecture" => info.architecture = value.into(),
            "os" => info.os = value.into(),
            "cpu" => info.cpu_model = value.into(),
            "cores" => info.logical_cpus = value.parse().unwrap_or(0),
            "MemTotal:" => info.memory_total = value.parse()?,
            "MemAvailable:" => {
                info.memory_available = value.parse()?;
                has_available = true;
            }
            "MemFree:" => free = value.parse()?,
            "Buffers:" => buffers = value.parse()?,
            "Cached:" => cached = value.parse()?,
            "SwapTotal:" => info.swap_total = value.parse()?,
            "SwapFree:" => info.swap_free = value.parse()?,
            "uptime" => info.uptime = value.split_whitespace().next().unwrap_or("0").parse()?,
            "load" => {
                info.load = value
                    .split_whitespace()
                    .take(3)
                    .map(str::parse)
                    .collect::<std::result::Result<Vec<_>, _>>()?
            }
            "cpuTicks" => {
                let fields: Vec<_> = value.split_whitespace().collect();
                if fields.len() == 2 {
                    info.cpu_ticks = CpuTicks {
                        total: fields[0].parse()?,
                        idle: fields[1].parse()?,
                    };
                }
            }
            "io" => {
                let fields: Vec<_> = value.split_whitespace().collect();
                if fields.len() == 3 {
                    info.io.push(IoCounter {
                        name: fields[0].into(),
                        read: fields[1].parse()?,
                        written: fields[2].parse()?,
                    });
                }
            }
            "net" => {
                let fields: Vec<_> = value.split_whitespace().collect();
                if fields.len() == 3 {
                    info.network.push(NetCounter {
                        name: fields[0].into(),
                        received: fields[1].parse()?,
                        sent: fields[2].parse()?,
                    });
                }
            }
            "disk" => {
                let fields: Vec<_> = value.split_whitespace().collect();
                if fields.len() >= 6 {
                    info.disks.push(Disk {
                        name: fields[0].into(),
                        total: fields[1].parse::<u64>()?.saturating_mul(1024),
                        used: fields[2].parse::<u64>()?.saturating_mul(1024),
                        available: fields[3].parse::<u64>()?.saturating_mul(1024),
                        mount: fields[5..].join(" "),
                    });
                }
            }
            _ => {}
        }
    }
    if !has_available {
        info.memory_available = free.saturating_add(buffers).saturating_add(cached);
    }
    info.memory_available = info.memory_available.min(info.memory_total);
    Ok(info)
}

fn execute(session: &Session, closed: &AtomicBool, command: &str) -> Result<String> {
    ensure!(!closed.load(Ordering::Acquire), "SSH-Sitzung ist beendet");
    let mut channel = crate::ssh::retry(|| session.channel_session())?;
    crate::ssh::retry(|| channel.exec(command))?;
    let result = (|| -> Result<String> {
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let start = Instant::now();
        let mut buffer = [0_u8; 8192];
        loop {
            ensure!(!closed.load(Ordering::Acquire), "SSH-Sitzung ist beendet");
            ensure!(
                start.elapsed() < Duration::from_secs(8),
                "Zeitlimit bei der Systemabfrage"
            );
            let mut progress = false;
            for (stream, target) in [(0, &mut output), (1, &mut errors)] {
                match channel.stream(stream).read(&mut buffer) {
                    Ok(n) => {
                        ensure!(target.len() + n <= 256 * 1024, "Systemantwort ist zu groß");
                        target.extend_from_slice(&buffer[..n]);
                        progress |= n > 0;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error.into()),
                }
            }
            if channel.eof() {
                break;
            }
            if !progress {
                thread::sleep(Duration::from_millis(5));
            }
        }
        crate::ssh::retry(|| channel.wait_close())?;
        ensure!(
            channel.exit_status()? == 0,
            "Systemabfrage fehlgeschlagen: {}",
            String::from_utf8_lossy(&errors).trim()
        );
        String::from_utf8(output).context("Systemdaten sind nicht UTF-8")
    })();
    let _ = channel.close();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hardware_parser_counts_bytes_and_preserves_mount_names() {
        let info = parse_hardware("MemTotal:\t10000\nMemFree:\t1000\nCached:\t2000\nBuffers:\t500\nload\t0.5 1.0 2.0 1/100 1\ndisk\t/dev/sda1 100 40 60 40% /a mount\n").unwrap();
        assert_eq!(info.memory_available, 3500);
        assert_eq!(info.disks[0].total, 102400);
        assert_eq!(info.disks[0].mount, "/a mount");
        assert_eq!(info.load.len(), 3);
    }
}

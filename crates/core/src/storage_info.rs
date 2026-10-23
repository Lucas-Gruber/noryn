//! Mount topology and optional read-only LVM reports, loaded only on demand.
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
#[derive(Serialize)]
pub struct Mount {
    pub source: String,
    pub target: String,
    pub filesystem: String,
    pub options: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    pub mounts: Vec<Mount>,
    pub pvs: Vec<Value>,
    pub vgs: Vec<Value>,
    pub lvs: Vec<Value>,
    pub messages: Vec<String>,
}
pub const SCRIPT: &str = r#"printf 'NORYN_MOUNTS\n'
head -n 400 /proc/self/mountinfo
for tool in pvs vgs lvs; do
 printf '\nNORYN_%s\n' "$tool"
 if command -v "$tool" >/dev/null 2>&1; then
  case "$tool" in
   pvs) pvs --readonly --reportformat json --units b --nosuffix -o pv_name,pv_size,pv_free,vg_name 2>/dev/null || printf 'NORYN_ERROR:PV-Daten nicht lesbar; gegebenenfalls mit sudo laden.\n' ;;
   vgs) vgs --readonly --reportformat json --units b --nosuffix -o vg_name,vg_size,vg_free,pv_count,lv_count 2>/dev/null || printf 'NORYN_ERROR:VG-Daten nicht lesbar; gegebenenfalls mit sudo laden.\n' ;;
   lvs) lvs --readonly --reportformat json --units b --nosuffix -o lv_name,vg_name,lv_size,lv_attr,lv_path,pool_lv,origin 2>/dev/null || printf 'NORYN_ERROR:LV-Daten nicht lesbar; gegebenenfalls mit sudo laden.\n' ;;
  esac
 else
  printf 'NORYN_ERROR:%s ist auf diesem Ziel nicht installiert.\n' "$tool"
 fi
done
"#;
fn unescape(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
        {
            let value = u16::from(bytes[i + 1] - b'0') * 64
                + u16::from(bytes[i + 2] - b'0') * 8
                + u16::from(bytes[i + 3] - b'0');
            out.push(u8::try_from(value).unwrap_or(b'?'));
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into()
}
pub fn parse(text: &str) -> Result<Storage> {
    let mut output = Storage {
        mounts: Vec::new(),
        pvs: Vec::new(),
        vgs: Vec::new(),
        lvs: Vec::new(),
        messages: Vec::new(),
    };
    let mut section = "";
    let mut json = String::new();
    fn finish(section: &str, json: &str, output: &mut Storage) -> Result<()> {
        if json.trim().is_empty() {
            return Ok(());
        }
        let (key, target) = match section {
            "pvs" => ("pv", &mut output.pvs),
            "vgs" => ("vg", &mut output.vgs),
            "lvs" => ("lv", &mut output.lvs),
            _ => return Ok(()),
        };
        let value: Value = serde_json::from_str(json)?;
        if let Some(reports) = value["report"].as_array() {
            for report in reports {
                if let Some(rows) = report[key].as_array() {
                    target.extend(rows.iter().take(1000).cloned());
                }
            }
        }
        Ok(())
    }
    for line in text.lines() {
        if let Some(next) = line.strip_prefix("NORYN_") {
            if let Some(message) = next.strip_prefix("ERROR:") {
                output.messages.push(message.into());
                json.clear();
                continue;
            }
            finish(section, &json, &mut output)?;
            json.clear();
            section = next;
            continue;
        }
        if section == "MOUNTS" {
            let fields: Vec<_> = line.split_whitespace().collect();
            let divider = fields.iter().position(|field| *field == "-");
            if let Some(divider) = divider.filter(|index| *index >= 6 && *index + 3 < fields.len())
            {
                output.mounts.push(Mount {
                    source: unescape(fields[divider + 2]),
                    target: unescape(fields[4]),
                    filesystem: fields[divider + 1].into(),
                    options: format!("{},{}", fields[5], fields[divider + 3]),
                });
            }
        } else {
            json.push_str(line);
            json.push('\n');
        }
    }
    finish(section, &json, &mut output)?;
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mount_paths_and_optional_lvm_reports_are_parsed() {
        let data=parse("NORYN_MOUNTS\n20 1 8:1 / /my\\040mount rw - ext4 /dev/sda1 rw\nNORYN_pvs\n{\"report\":[{\"pv\":[{\"pv_name\":\"/dev/sda2\"}]}]}\nNORYN_vgs\nNORYN_ERROR:no permissions\nNORYN_lvs\n{\"report\":[{\"lv\":[]}]}\n").unwrap();
        assert_eq!(data.mounts[0].target, "/my mount");
        assert_eq!(unescape("\\777"), "?");
        assert_eq!(data.pvs.len(), 1);
        assert_eq!(data.messages, vec!["no permissions"]);
    }
}

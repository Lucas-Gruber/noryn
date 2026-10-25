//! The ssh2 wrapper lacks OpenSSH's POSIX rename API. Use one small SFTP subsystem
//! channel on the existing transport; no shell, additional login, or target agent.
//! https://github.com/openssh/openssh-portable/blob/master/PROTOCOL#L381
use crate::{ssh::retry, transfer::Control};
use anyhow::{ensure, Context, Result};
use std::{
    io::{Read, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};
const EXTENSION: &[u8] = b"posix-rename@openssh.com";
const MAX_PACKET: usize = 64 * 1024;

pub(crate) fn replace(
    session: &ssh2::Session,
    from: &Path,
    to: &Path,
    control: &Control,
) -> Result<()> {
    let mut channel = retry(|| session.channel_session())?;
    retry(|| channel.subsystem("sftp"))?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let result = (|| {
        send(&mut channel, &[1, 0, 0, 0, 3], deadline, control)?; // SSH_FXP_INIT v3
        let version = receive(&mut channel, deadline, control)?;
        ensure!(supports_replace(&version)?,"SFTP-Server unterstützt kein sicheres Ersetzen (posix-rename). Originaldatei wurde nicht verändert");
        let mut request = vec![200, 0, 0, 0, 1]; // SSH_FXP_EXTENDED, request ID 1
        for value in [
            EXTENSION,
            from.to_str().context("Ungültiger SFTP-Pfad")?.as_bytes(),
            to.to_str().context("Ungültiger SFTP-Pfad")?.as_bytes(),
        ] {
            ensure!(value.len() <= 16 * 1024, "SFTP-Pfad ist zu lang");
            request.extend_from_slice(&(value.len() as u32).to_be_bytes());
            request.extend_from_slice(value);
        }
        control.check()?;
        send(&mut channel, &request, deadline, control)?;
        let status=receive(&mut channel,deadline,control).context("Bestätigung für das Ersetzen fehlt; Ausgang unklar. Originaldatei erneut herunterladen")?;
        check_status(&status)
    })();
    let _ = channel.close();
    result
}
fn supports_replace(packet: &[u8]) -> Result<bool> {
    ensure!(
        packet.len() >= 5 && packet[0] == 2,
        "Ungültige SFTP-Versionsantwort"
    );
    ensure!(
        u32::from_be_bytes(packet[1..5].try_into()?) == 3,
        "SFTP-Version wird für das Ersetzen nicht unterstützt"
    );
    let mut remaining = &packet[5..];
    let mut supported = false;
    while !remaining.is_empty() {
        let name = string(&mut remaining)?;
        let version = string(&mut remaining)?;
        supported |= name == EXTENSION && version == b"1";
    }
    Ok(supported)
}
fn string<'a>(packet: &mut &'a [u8]) -> Result<&'a [u8]> {
    ensure!(packet.len() >= 4, "Unvollständige SFTP-Antwort");
    let length = u32::from_be_bytes(packet[..4].try_into()?) as usize;
    *packet = &packet[4..];
    ensure!(length <= packet.len(), "Ungültige SFTP-Feldlänge");
    let (value, rest) = packet.split_at(length);
    *packet = rest;
    Ok(value)
}
fn check_status(packet: &[u8]) -> Result<()> {
    ensure!(
        packet.len() >= 9 && packet[0] == 101 && u32::from_be_bytes(packet[1..5].try_into()?) == 1,
        "Ungültige SFTP-Statusantwort; Ausgang unklar"
    );
    let status = u32::from_be_bytes(packet[5..9].try_into()?);
    ensure!(
        status == 0,
        "SFTP-Ersetzen wurde abgelehnt (Status {status}); Originaldatei bitte prüfen"
    );
    Ok(())
}
fn transfer(
    channel: &mut ssh2::Channel,
    buffer: &mut [u8],
    write: bool,
    deadline: Instant,
    control: &Control,
) -> Result<()> {
    let mut offset = 0;
    while offset < buffer.len() {
        control.check()?;
        ensure!(
            Instant::now() < deadline,
            "Zeitüberschreitung beim SFTP-Ersetzen"
        );
        let result = if write {
            channel.write(&buffer[offset..])
        } else {
            channel.read(&mut buffer[offset..])
        };
        match result {
            Ok(0) => anyhow::bail!("SFTP-Kanal unerwartet geschlossen"),
            Ok(count) => offset += count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2))
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
fn send(
    channel: &mut ssh2::Channel,
    packet: &[u8],
    deadline: Instant,
    control: &Control,
) -> Result<()> {
    ensure!(packet.len() <= MAX_PACKET, "SFTP-Paket zu groß");
    let mut bytes = Vec::with_capacity(packet.len() + 4);
    bytes.extend_from_slice(&(packet.len() as u32).to_be_bytes());
    bytes.extend_from_slice(packet);
    transfer(channel, &mut bytes, true, deadline, control)
}
fn receive(channel: &mut ssh2::Channel, deadline: Instant, control: &Control) -> Result<Vec<u8>> {
    let mut length = [0; 4];
    transfer(channel, &mut length, false, deadline, control)?;
    let length = u32::from_be_bytes(length) as usize;
    ensure!(
        length > 0 && length <= MAX_PACKET,
        "SFTP-Antwort zu groß oder leer"
    );
    let mut packet = vec![0; length];
    transfer(channel, &mut packet, false, deadline, control)?;
    Ok(packet)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extension_and_status_parsers_reject_wrong_versions_truncation_and_request_ids() {
        let mut version = vec![2, 0, 0, 0, 3];
        for field in [EXTENSION, &b"1"[..]] {
            version.extend_from_slice(&(field.len() as u32).to_be_bytes());
            version.extend_from_slice(field);
        }
        assert!(supports_replace(&version).unwrap());
        assert!(!supports_replace(&[2, 0, 0, 0, 3]).unwrap());
        version.pop();
        assert!(supports_replace(&version).is_err());
        assert!(supports_replace(&[2, 0, 0, 0, 3, 255, 255, 255, 255]).is_err());
        assert!(supports_replace(&[2, 0, 0, 0, 6]).is_err());
        assert!(check_status(&[101, 0, 0, 0, 1, 0, 0, 0, 0]).is_ok());
        assert!(check_status(&[101, 0, 0, 0, 2, 0, 0, 0, 0]).is_err());
        assert!(check_status(&[101, 0, 0, 0, 1, 0, 0, 0, 4]).is_err());
        assert!(check_status(&[101]).is_err());
    }
}

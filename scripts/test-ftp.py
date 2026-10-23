"""Isolated real FTP/FTPS servers. Requires scripts/requirements-ftp.txt locally, never on a target."""
import datetime
import ipaddress
import logging
import os
import signal
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def serve(mode, root, certificate, port_file):
    from pyftpdlib.authorizers import DummyAuthorizer
    from pyftpdlib.handlers import FTPHandler, TLS_FTPHandler
    from pyftpdlib.servers import FTPServer
    from pyftpdlib.log import config_logging

    config_logging(level=logging.CRITICAL)
    authorizer = DummyAuthorizer()
    authorizer.add_user("test-user", "test-password", root, perm="elradfmwMT")
    base = TLS_FTPHandler if mode.startswith("tls") else FTPHandler

    class Handler(base):
        def ftp_MLSD(self, path):
            if mode in ("posix", "dos"):
                self.respond("502 MLSD not implemented")
            else:
                super().ftp_MLSD(path)

        def ftp_LIST(self, path):
            if mode != "dos":
                return super().ftp_LIST(path)
            lines = []
            for entry in sorted(Path(path).iterdir()):
                kind = "<DIR>" if entry.is_dir() else str(entry.stat().st_size)
                lines.append(f"10-23-26  09:10AM {kind:>14} {entry.name}\r\n")
            self.push_dtp_data("".join(lines).encode("utf-8"), isproducer=False, cmd="LIST")

    Handler.authorizer = authorizer
    Handler.auth_failed_timeout = 0
    # A deliberately unusable advertised address tests the passive NAT workaround.
    Handler.masquerade_address = "192.0.2.123"
    if mode.startswith("tls"):
        Handler.certfile = certificate
        Handler.tls_control_required = True
        Handler.tls_data_required = True
    server = FTPServer(("127.0.0.1", 0), Handler)
    Path(port_file).write_text(str(server.socket.getsockname()[1]))
    if hasattr(signal, 'SIGTERM'):
        signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    try:
        server.serve_forever(timeout=0.1, handle_exit=False)
    finally:
        server.close_all()


def certificates(directory):
    from cryptography import x509
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import rsa
    from cryptography.x509.oid import NameOID, ExtendedKeyUsageOID

    now = datetime.datetime.now(datetime.timezone.utc)
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Noryn disposable test CA")])
    ca = (x509.CertificateBuilder().subject_name(name).issuer_name(name)
          .public_key(key.public_key()).serial_number(x509.random_serial_number())
          .not_valid_before(now - datetime.timedelta(minutes=5))
          .not_valid_after(now + datetime.timedelta(days=1))
          .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
          .sign(key, hashes.SHA256()))
    (directory / "ca.pem").write_bytes(ca.public_bytes(serialization.Encoding.PEM))
    for mode in ("tls", "tls-wrong-host"):
        leaf_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        leaf_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, mode)])
        alternatives = ([x509.IPAddress(ipaddress.ip_address("127.0.0.1")), x509.DNSName("localhost")]
                        if mode == "tls" else [x509.DNSName("wrong.invalid")])
        leaf = (x509.CertificateBuilder().subject_name(leaf_name).issuer_name(name)
                .public_key(leaf_key.public_key()).serial_number(x509.random_serial_number())
                .not_valid_before(now - datetime.timedelta(minutes=5))
                .not_valid_after(now + datetime.timedelta(days=1))
                .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
                .add_extension(x509.SubjectAlternativeName(alternatives), critical=False)
                .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH]), critical=False)
                .sign(key, hashes.SHA256()))
        (directory / f"{mode}.pem").write_bytes(
            leaf.public_bytes(serialization.Encoding.PEM)
            + leaf_key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8,
                                     serialization.NoEncryption()))


def main():
    with tempfile.TemporaryDirectory(prefix="noryn-ftp-") as temporary:
        directory = Path(temporary)
        certificates(directory)
        children = []
        environment = dict(os.environ)
        try:
            for mode in ("mlsd", "posix", "dos", "tls", "tls-wrong-host"):
                root = directory / mode
                root.mkdir()
                (root / "folder").mkdir()
                (root / "source.bin").write_bytes(bytes(range(256)) * 1024)
                (root / "ä space; file.txt").write_text("UTF-8 content")
                (root / ".hidden").write_text("private")
                port_file = directory / f"{mode}.port"
                children.append(subprocess.Popen(
                    [sys.executable, __file__, "--serve", mode, str(root),
                     str(directory / f"{mode}.pem"), str(port_file)],
                    stdout=subprocess.DEVNULL))
                deadline = time.monotonic() + 15
                while not port_file.exists():
                    if children[-1].poll() is not None or time.monotonic() > deadline:
                        raise RuntimeError(f"FTP test server {mode} did not start")
                    time.sleep(0.05)
                environment[f"NORYN_TEST_FTP_{mode.upper().replace('-', '_')}"] = port_file.read_text()
            environment["NORYN_TEST_FTP_CA"] = str(directory / "ca.pem")
            result = subprocess.run(
                ["cargo", "test", "-p", "harbor-core", "--test", "ftp_integration", "--",
                 "--ignored", "--nocapture"], env=environment, check=False)
            return result.returncode
        finally:
            for child in children:
                child.terminate()
            for child in children:
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--serve":
        serve(*sys.argv[2:])
    else:
        sys.exit(main())

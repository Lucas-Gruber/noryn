"""Real loopback TLS/NLA/graphics/input test. Test dependencies stay outside desktop release builds."""
from pathlib import Path
import datetime
import ipaddress
import os
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]

def main():
    from cryptography import x509
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import rsa
    from cryptography.x509.oid import NameOID
    manifest = ROOT / "scripts/rdp-fixture/Cargo.toml"
    subprocess.run(["cargo", "build", "--locked", "--manifest-path", str(manifest)], check=True, cwd=ROOT)
    with tempfile.TemporaryDirectory(prefix="noryn-rdp-") as temporary:
        directory = Path(temporary)
        key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "localhost")])
        now = datetime.datetime.now(datetime.timezone.utc)
        cert = (x509.CertificateBuilder().subject_name(name).issuer_name(name).public_key(key.public_key())
                .serial_number(x509.random_serial_number()).not_valid_before(now-datetime.timedelta(minutes=1))
                .not_valid_after(now+datetime.timedelta(days=1))
                .add_extension(x509.SubjectAlternativeName([x509.DNSName("localhost"),x509.IPAddress(ipaddress.ip_address("127.0.0.1"))]),critical=False)
                .sign(key,hashes.SHA256()))
        (directory / "cert.pem").write_bytes(cert.public_bytes(serialization.Encoding.PEM))
        private = directory / "key.pem"
        private.write_bytes(key.private_bytes(serialization.Encoding.PEM,serialization.PrivateFormat.PKCS8,serialization.NoEncryption()))
        if os.name != 'nt': private.chmod(0o600)
        binary = manifest.parent / "target/debug" / ("noryn-rdp-test-server.exe" if os.name=='nt' else "noryn-rdp-test-server")
        environment = dict(os.environ)
        environment.pop('SSLKEYLOGFILE', None)
        server = subprocess.Popen([str(binary),temporary],cwd=ROOT,env=environment)
        try:
            deadline=time.monotonic()+15
            while not (directory/'port').exists():
                if server.poll() is not None or time.monotonic()>deadline: raise RuntimeError('Disposable RDP server did not start')
                time.sleep(.05)
            environment['NORYN_TEST_RDP_ROOT']=temporary
            return subprocess.run(['cargo','test','-p','noryn','--lib','--locked','rdp::tests::actual_tls_nla_graphics_input_and_lock_cleanup','--','--ignored','--exact','--nocapture'],cwd=ROOT,env=environment).returncode
        finally:
            server.terminate()
            try: server.wait(timeout=5)
            except subprocess.TimeoutExpired: server.kill();server.wait()

if __name__=='__main__': raise SystemExit(main())

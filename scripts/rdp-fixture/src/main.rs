//! Disposable loopback RDP/NLA server, using a separate implementation/version from the client.
use ironrdp_server::{
    BitmapUpdate, Credentials, DesktopSize, DisplayUpdate, KeyboardEvent, MouseEvent, PixelFormat,
    RGBAPointer, RdpServer, RdpServerDisplay, RdpServerDisplayUpdates, RdpServerInputHandler,
    TlsIdentityCtx,
};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    num::{NonZeroU16, NonZeroUsize},
    path::PathBuf,
    time::Duration,
};
struct Input(File);
impl RdpServerInputHandler for Input {
    fn keyboard(&mut self, event: KeyboardEvent) {
        let _ = writeln!(self.0, "key {event:?}");
    }
    fn mouse(&mut self, event: MouseEvent) {
        let _ = writeln!(self.0, "mouse {event:?}");
    }
}
struct Display;
struct Updates(u32);
#[async_trait::async_trait]
impl RdpServerDisplayUpdates for Updates {
    async fn next_update(&mut self) -> anyhow::Result<Option<DisplayUpdate>> {
        if self.0 == 0 {
            self.0 = 1;
            return Ok(Some(DisplayUpdate::RGBAPointer(RGBAPointer {
                cache_index: 0,
                width: 16,
                height: 16,
                hot_x: 1,
                hot_y: 1,
                data: [240, 240, 240, 255].repeat(256),
            })));
        }
        let (x, y, width, height) = if self.0 == 1 {
            (0, 0, 640, 480)
        } else {
            tokio::time::sleep(Duration::from_millis(8)).await;
            (10, 10, 16, 16)
        };
        self.0 += 1;
        let pixels = [40u8, 90, 180 + (self.0 % 60) as u8, 255]
            .repeat(usize::from(width) * usize::from(height));
        Ok(Some(DisplayUpdate::Bitmap(BitmapUpdate {
            x,
            y,
            width: NonZeroU16::new(width).unwrap(),
            height: NonZeroU16::new(height).unwrap(),
            format: PixelFormat::RgbA32,
            data: pixels.into(),
            stride: NonZeroUsize::new(usize::from(width) * 4).unwrap(),
        })))
    }
}
#[async_trait::async_trait]
impl RdpServerDisplay for Display {
    async fn size(&mut self) -> DesktopSize {
        DesktopSize {
            width: 640,
            height: 480,
        }
    }
    async fn updates(&mut self) -> anyhow::Result<Box<dyn RdpServerDisplayUpdates>> {
        Ok(Box::new(Updates(0)))
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("temporary fixture directory"),
    );
    let identity = TlsIdentityCtx::init_from_paths(&root.join("cert.pem"), &root.join("key.pem"))?;
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    let address = listener.local_addr()?;
    drop(listener);
    let input = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("input.log"))?;
    let mut server = RdpServer::builder()
        .with_addr(address)
        .with_hybrid(identity.make_acceptor()?, identity.pub_key.clone())
        .with_input_handler(Input(input))
        .with_display_handler(Display)
        .with_honor_client_desktop_size(false)
        .build();
    server.set_credentials(Some(Credentials {
        username: "noryn-test-user".into(),
        password: "noryn-test-password".into(),
        domain: None,
    }));
    std::fs::write(root.join("port"), address.port().to_string())?;
    server.run().await
}

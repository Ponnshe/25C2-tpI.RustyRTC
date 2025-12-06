use std::net::UdpSocket;
use std::sync::mpsc::Sender;
use anyhow::Result;

pub fn start_udp_receiver(tx: Sender<Vec<i16>>, port: u16) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", port))?;
    socket.set_nonblocking(true)?;

    std::thread::spawn(move || {
        let mut buf = [0u8; 2048];
        loop {
            if let Ok((len, _src)) = socket.recv_from(&mut buf) {
                let slice: &[i16] = bytemuck::cast_slice(&buf[..len]);
                tx.send(slice.to_vec()).ok();
            }
        }
    });

    Ok(())
}

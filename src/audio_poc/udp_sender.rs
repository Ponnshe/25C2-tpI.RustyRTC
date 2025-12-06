use std::net::UdpSocket;
use anyhow::Result;

pub struct UdpSender {
    socket: UdpSocket,
    remote: String,
}

impl UdpSender {
    pub fn new(remote_addr: &str) -> Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        Ok(Self {
            socket,
            remote: remote_addr.to_string(),
        })
    }

    pub fn send(&self, data: &[i16]) -> Result<()> {
        let bytes = bytemuck::cast_slice(data);
        self.socket.send_to(bytes, &self.remote)?;
        Ok(())
    }
}

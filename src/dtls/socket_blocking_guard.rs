use std::{io, net::UdpSocket, sync::Arc, time::Duration};

/// RAII guard para dejar el socket en blocking + timeout mientras exista,
/// y restaurar non-blocking + sin timeout cuando se suelte.
pub(crate) struct SocketBlockingGuard {
    sock: Arc<UdpSocket>,
}

impl SocketBlockingGuard {
    /// Pone el socket en blocking y setea read timeout.
    pub(crate) fn new(sock: Arc<UdpSocket>, timeout: Option<Duration>) -> io::Result<Self> {
        // Setear timeout primero (opcional)
        sock.set_read_timeout(timeout)?;
        // Forzar blocking (true -> blocking == set_nonblocking(false))
        sock.set_nonblocking(false)?;
        Ok(SocketBlockingGuard { sock })
    }
}

impl Drop for SocketBlockingGuard {
    fn drop(&mut self) {
        // Restauramos non-blocking y limpiamos el timeout.
        // Ignoramos errores en el Drop para no panicar.
        let _ = self.sock.set_nonblocking(true);
        let _ = self.sock.set_read_timeout(None);
    }
}

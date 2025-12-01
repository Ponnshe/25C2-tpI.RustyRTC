use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

use crate::ice::type_ice::ice_agent::{BINDING_REQUEST, IceAgent};

/// A worker that handles ICE connectivity checks in a background thread.
pub struct IceWorker {
    run: Arc<AtomicBool>,
    rx: Receiver<(Vec<u8>, SocketAddr)>,
    handle: Option<thread::JoinHandle<()>>,
}

impl IceWorker {
    /// Spawns a new `IceWorker` thread.
    #[must_use]
    pub fn spawn(agent: &IceAgent) -> Self {
        let run = Arc::new(AtomicBool::new(true));
        let (tx, rx) = mpsc::channel();

        // Snapshot sockets
        let sockets: Vec<Arc<std::net::UdpSocket>> = agent
            .local_candidates
            .iter()
            .filter_map(|c| c.socket.clone())
            .collect();

        // Snapshot send targets per socket index
        let mut targets_per_sock: Vec<Vec<SocketAddr>> = vec![Vec::new(); sockets.len()];
        for pair in &agent.candidate_pairs {
            if let Some(ls) = &pair.local.socket
                && let Some(idx) = sockets.iter().position(|s| Arc::ptr_eq(s, ls))
            {
                targets_per_sock[idx].push(pair.remote.address);
            }
        }

        let run2 = Arc::clone(&run);
        let handle = thread::spawn(move || {
            let () = sockets.iter().for_each(|s| {
                let _ = s.set_nonblocking(true);
            });
            let mut buf = [0u8; 1500];
            let resend_every = Duration::from_millis(200);
            let mut last_tx = Instant::now();

            while run2.load(Ordering::SeqCst) {
                // Drain inbound
                for s in &sockets {
                    loop {
                        match s.recv_from(&mut buf) {
                            Ok((n, from)) => {
                                let _ = tx.send((buf[..n].to_vec(), from));
                            }
                            Err(ref e)
                                if e.kind() == std::io::ErrorKind::WouldBlock
                                    || e.kind() == std::io::ErrorKind::TimedOut =>
                            {
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                }
                // Periodic re-send BINDING_REQUEST
                if last_tx.elapsed() >= resend_every {
                    for (i, s) in sockets.iter().enumerate() {
                        for &dst in &targets_per_sock[i] {
                            let _ = s.send_to(BINDING_REQUEST, dst);
                        }
                    }
                    last_tx = Instant::now();
                }
                thread::sleep(Duration::from_millis(20));
            }
        });

        Self {
            run,
            rx,
            handle: Some(handle),
        }
    }

    /// Tries to receive a packet from the worker thread without blocking.
    #[must_use]
    pub fn try_recv(&self) -> Option<(Vec<u8>, SocketAddr)> {
        self.rx.try_recv().ok()
    }

    /// Stops the worker thread.
    pub fn stop(&mut self) {
        self.run.store(false, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

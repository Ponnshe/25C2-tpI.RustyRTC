use std::{env, fs, net::SocketAddr, path::Path, thread, time::Duration};

use rustyrtc::connection_manager::ConnectionManager;
use rustyrtc::sdp::sdpc::Sdp;

const OFFER_FILE: &str = "offer.txt";
const ANSWER_FILE: &str = "answer.txt";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("error");

    if mode != "A" && mode != "B" {
        eprintln!("Uso: cargo run -- A|B");
        return Ok(());
    }

    let mut conn_manager = ConnectionManager::new();

    if mode == "A" {
        // Peer A genera offer
        let offer_sdp = conn_manager.create_offer()?;
        fs::write(OFFER_FILE, offer_sdp.encode())?;
        println!("[A] Oferta generada, esperando answer...");
        wait_for_file(ANSWER_FILE);
        let answer_str = fs::read_to_string(ANSWER_FILE)?;
        conn_manager.receive_answer(Sdp::parse(&answer_str)?)?;
        println!("[A] Answer procesada, conexión lista.");
    } else {
        // Peer B espera offer
        wait_for_file(OFFER_FILE);
        let offer_str = fs::read_to_string(OFFER_FILE)?;
        let answer_sdp = conn_manager.receive_offer_and_create_answer(&offer_str)?;
        fs::write(ANSWER_FILE, answer_sdp.encode())?;
        println!("[B] Offer procesada y answer escrita.");
    }

    // --- Configuración de sockets ---
    let local_candidate = conn_manager
        .ice_agent
        .local_candidates
        .get_mut(0)
        .expect("Debe haber al menos un candidato local");

    let remote_candidate = conn_manager
        .ice_agent
        .remote_candidates
        .get_mut(0)
        .expect("Debe haber al menos un candidato remoto");

    let socket = local_candidate
        .socket
        .take()
        .expect("Socket local no inicializado");

    let remote_addr: SocketAddr = remote_candidate.address;

    socket.connect(remote_addr)?;
    println!("Local: {}", socket.local_addr()?);
    println!("Peer:  {}", socket.peer_addr()?);

    // Clone the socket for the sending thread (same underlying FD/port)
    let send_sock = socket.try_clone()?;

    // identify the side
    let tag = if mode == "A" { "A" } else { "B" };

    // Periodic sender (every 1s)
    thread::spawn(move || {
        let mut seq: u64 = 0;
        loop {
            let msg = format!("{tag}:{seq}");
            if let Err(e) = send_sock.send(msg.as_bytes()) {
                eprintln!("[SEND ERROR] {e}");
                break;
            }
            seq += 1;
            thread::sleep(Duration::from_secs(1));
        }
    });

    // Receive loop (blocking)
    let mut buf = [0u8; 1500];
    loop {
        let n = socket.recv(&mut buf)?;
        println!("[RECV] {}", String::from_utf8_lossy(&buf[..n]));
    }
}

/// Espera hasta que un archivo exista
fn wait_for_file(path: &str) {
    while !Path::new(path).exists() {
        thread::sleep(Duration::from_millis(50));
    }
}

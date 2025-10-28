use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender},
    },
    thread,
    time::Duration,
};

use super::{
    rtp_recv_config::RtpRecvConfig, rtp_recv_stream::RtpRecvStream, rtp_send_config::RtpSendConfig,
    rtp_send_stream::RtpSendStream, rtp_session_error::RtpSessionError,
};
use crate::core::events::EngineEvent;
use crate::rtcp::rtcp::RtcpPacket;
use rand::{RngCore, rngs::OsRng};

pub struct RtpSession {
    sock: Arc<UdpSocket>,
    peer: SocketAddr,

    // Demux maps
    recv_streams: Arc<Mutex<HashMap<u32, RtpRecvStream>>>, // key: remote_ssrc
    pending_recv: Arc<Mutex<Vec<RtpRecvStream>>>,          // remote_ssrc=None
    send_streams: Arc<Mutex<HashMap<u32, RtpSendStream>>>, // key: local_ssrc

    // Control
    run: Arc<AtomicBool>,
    tx_evt: Sender<EngineEvent>,
    rx_media: Option<Receiver<Vec<u8>>>,

    // Session RTCP identity (used for RR sender_ssrc and SDES)
    local_rtcp_ssrc: u32,
    cname: String,
    rtcp_interval: Duration,
}

impl RtpSession {
    pub fn new(
        sock: Arc<UdpSocket>,
        peer: SocketAddr,
        tx_evt: Sender<EngineEvent>,
        rx_media: Receiver<Vec<u8>>,
        initial_recv: Vec<RecvStreamInfo>, // can be empty
        initial_send: Vec<SendStreamInfo>, // can be empty
    ) -> Self {
        let recv_map = HashMap::new();
        let send_map = HashMap::new();

        let mut this = Self {
            sock,
            peer,
            recv_streams: Arc::new(Mutex::new(recv_map)),
            send_streams: Arc::new(Mutex::new(send_map)),
            run: Arc::new(AtomicBool::new(false)),
            tx_evt,
            rx_media: Some(rx_media),
            local_rtcp_ssrc: OsRng.next_u32(),
            cname: "roomrtc@local".into(),
            rtcp_interval: Duration::from_millis(500),
        };

        for info in initial_recv {
            let _ = this.add_recv_stream(info.remote_ssrc, info.payload_type, info.clock_rate);
        }
        for info in initial_send {
            let _ = this.add_send_stream(info.local_ssrc, info.payload_type, info.clock_rate);
        }

        this
    }

    pub fn add_recv_stream(&mut self, cfg: RtpRecvConfig) {
        if let Some(ssrc) = cfg.remote_ssrc {
            let st =
                RtpRecvStream::new(cfg, Arc::clone(&self.sock), self.peer, self.tx_evt.clone());
            self.recv_streams.lock().unwrap().insert(ssrc, st);
        } else {
            let st =
                RtpRecvStream::new(cfg, Arc::clone(&self.sock), self.peer, self.tx_evt.clone());
            self.pending_recv.lock().unwrap().push(st);
        }
    }

    pub fn add_send_stream(&mut self, cfg: RtpSendConfig) {
        let ssrc = cfg.local_ssrc;
        let st = RtpSendStream::new(cfg, Arc::clone(&self.sock), self.peer);
        self.send_streams.lock().unwrap().insert(ssrc, st);
    }

    pub fn start(&mut self) {
        self.run.store(true, Ordering::SeqCst);

        // === inbound RTP/RTCP loop ===
        let run = Arc::clone(&self.run);
        let rx = self.rx_media.take().expect("start() must be called once");
        let recv_map = Arc::clone(&self.recv_streams);
        let tx_evt = self.tx_evt.clone();
        thread::spawn(move || {
            while run.load(Ordering::SeqCst) {
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(pkt) => {
                        if pkt.len() < 2 {
                            continue;
                        }
                        if is_rtcp(&pkt) {
                            handle_rtcp(&pkt, &recv_map, &tx_evt);
                            continue;
                        }
                        // RTP fast-path
                        if pkt.len() < 12 {
                            continue;
                        }
                        if (pkt[0] >> 6) != 2 {
                            continue;
                        } // RTP v2
                        let ssrc = u32::from_be_bytes([pkt[8], pkt[9], pkt[10], pkt[11]]);
                        if let Some(st) = recv_map.lock().unwrap().get_mut(&ssrc) {
                            st.handle_packet(&pkt);
                        } else {
                            let _ = tx_evt.send(EngineEvent::Log(format!(
                                "[RTP] unknown remote SSRC={ssrc}"
                            )));
                            // (optional) auto-create stream here if desired.
                        }
                    }
                    Err(_) => {}
                }
            }
        });

        // === periodic RTCP sender: RR (from all RecvRtpStreams) + SDES ===
        let run2 = Arc::clone(&self.run);
        let sock = Arc::clone(&self.sock);
        let peer = self.peer;
        let recv_map2 = Arc::clone(&self.recv_streams);
        let tx_evt2 = self.tx_evt.clone();
        let interval = self.rtcp_interval;
        let rr_ssrc = self.local_rtcp_ssrc;
        let cname = self.cname.clone();

        thread::spawn(move || {
            while run2.load(Ordering::SeqCst) {
                std::thread::sleep(interval);

                // Build report blocks from *receive* streams
                let mut blocks: Vec<ReportBlock> = Vec::new();
                {
                    let mut guard = recv_map2.lock().unwrap();
                    for (remote_ssrc, st) in guard.iter_mut() {
                        blocks.push(st.build_report_block(*remote_ssrc));
                    }
                }

                let rr = ReceiverReport {
                    ssrc: rr_ssrc,
                    reports: blocks,
                }
                .encode();
                let sdes = Sdes {
                    items: vec![SdesItem {
                        ssrc: rr_ssrc,
                        cname: cname.clone(),
                    }],
                }
                .encode();

                let mut comp = Vec::with_capacity(rr.len() + sdes.len());
                comp.extend_from_slice(&rr);
                comp.extend_from_slice(&sdes);

                let _ = sock.send_to(&comp, peer);
                let _ = tx_evt2.send(EngineEvent::Log("[RTCP] tx RR+SDES".into()));
            }
        });

        // (Optional) add a periodic SR loop per send-stream later if you need SRs.
    }

    pub fn stop(&self) {
        self.run.store(false, Ordering::SeqCst);
    }

    /// Send PLI for a specific remote source.
    pub fn send_pli(&self, remote_ssrc: u32) {
        let pli = rtcp::PictureLossIndication {
            sender_ssrc: self.local_rtcp_ssrc,
            media_ssrc: remote_ssrc,
        }
        .encode();
        let _ = self.sock.send_to(&pli, self.peer);
        let _ = self.tx_evt.send(EngineEvent::Log(format!(
            "[RTCP] tx PLI media_ssrc={remote_ssrc}"
        )));
    }

    /// Convenience: does this remote SSRC exist as a recv stream?
    pub fn has_recv_ssrc(&self, remote_ssrc: u32) -> bool {
        self.recv_streams.lock().unwrap().contains_key(&remote_ssrc)
    }

    /// Convenience: get a mutable handle to a send stream by local SSRC (e.g., to call send_frame).
    pub fn with_send_stream<F: FnOnce(&mut RtpSendStream)>(&self, local_ssrc: u32, f: F) {
        if let Some(st) = self.send_streams.lock().unwrap().get_mut(&local_ssrc) {
            f(st);
        }
    }
}

// --------------------- helpers ---------------------

fn is_rtcp(pkt: &[u8]) -> bool {
    // RTCP PT lives in byte[1] (7-bit). 200..206 are common (SR/RR/SDES/BYE/APP/RTPFB/PSFB)
    let pt = pkt[1] & 0x7F;
    (200..=206).contains(&pt)
}

fn handle_rtcp(
    buf: &[u8],
    recv_map: &Arc<Mutex<HashMap<u32, RtpRecvStream>>>,
    tx_evt: &Sender<EngineEvent>,
) -> Result<(), RtpSessionError> {
    let rtcp_packet: Vec<RtcpPacket> = RtcpPacket::decode_compound(buf)?;
    // TODO: Implement demux iterating for each RtcpPacket and deciding what action to take.
    Ok(())
}

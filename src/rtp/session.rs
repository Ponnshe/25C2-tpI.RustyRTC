use std::{
    net::UdpSocket,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

use rand::{RngCore, rngs::OsRng};

use crate::core::events::EngineEvent;
use crate::rtp::rtcp::{
    self, ReceiverReport, ReportBlock, RtcpPacket, Sdes, SdesItem, SenderReport,
};
use crate::rtp::rtp_packet::{RtpHeader, RtpPacket, SeqExt};
use crate::rtp::time::ntp_now;

#[derive(Debug, Clone)]
pub struct RtpConfig {
    pub payload_type: u8, // e.g., 96
    pub clock_rate: u32,  // e.g., 90000 for VP8
    pub ssrc_local: u32,
    pub ssrc_remote: u32,
    pub cname: String,
    pub rtcp_interval: Duration, // e.g., 500ms
}

impl Default for RtpConfig {
    fn default() -> Self {
        Self {
            payload_type: 96,
            clock_rate: 90_000,
            ssrc_local: OsRng.next_u32(),
            ssrc_remote: 0,
            cname: "roomrtc@local".into(),
            rtcp_interval: Duration::from_millis(500),
        }
    }
}

pub struct RtpSession {
    sock: Arc<UdpSocket>,
    peer: std::net::SocketAddr,
    cfg: RtpConfig,

    // tx state
    seq: u16,
    ts: u32,
    pkt_count: u32,
    octet_count: u32,

    // rx state
    seqext: SeqExt,
    highest_ext_seq: u32,
    recv_pkts: u32,
    lost_pkts: u32,
    jitter: u32,
    last_arrival_rtp_ts: Option<(u32, u32)>, // (arrival_ms, transit)

    run: Arc<AtomicBool>,
    tx_evt: Sender<EngineEvent>,

    // inbound RTP/RTCP from Session demux
    rx_media: Receiver<Vec<u8>>,
    t0: Instant,
}

impl RtpSession {
    pub fn new(
        sock: Arc<UdpSocket>,
        peer: std::net::SocketAddr,
        cfg: RtpConfig,
        tx_evt: Sender<EngineEvent>,
        rx_media: Receiver<Vec<u8>>,
    ) -> Self {
        let mut rng = OsRng;
        Self {
            sock,
            peer,
            cfg,
            seq: rng.next_u32() as u16,
            ts: rng.next_u32(),
            pkt_count: 0,
            octet_count: 0,
            seqext: Default::default(),
            highest_ext_seq: 0,
            recv_pkts: 0,
            lost_pkts: 0,
            jitter: 0,
            last_arrival_rtp_ts: None,
            run: Arc::new(AtomicBool::new(false)),
            tx_evt,
            rx_media,
            t0: Instant::now(),
        }
    }

    pub fn start(&mut self) {
        self.run.store(true, Ordering::SeqCst);
        let run = self.run.clone();
        let rx = self.rx_media.clone();
        let tx_evt = self.tx_evt.clone();
        let clock = self.cfg.clock_rate;
        let ssrc_remote = self.cfg.ssrc_remote;
        let mut jitter = 0u32;
        let mut seqext = self.seqext.clone();
        let mut highest_ext_seq = self.highest_ext_seq;
        let mut recv_pkts = self.recv_pkts;
        let mut last_transit: Option<u32> = None;

        // === inbound media loop ===
        std::thread::spawn(move || {
            while run.load(Ordering::SeqCst) {
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(pkt) => {
                        if rtcp::is_rtcp(&pkt) {
                            let _ = tx_evt.send(EngineEvent::Log("[RTCP] rx".into()));
                            // Minimal parse could be added; keep as signal only for now
                            continue;
                        }
                        match RtpPacket::decode(&pkt) {
                            Ok(rtp) => {
                                if rtp.header.ssrc != ssrc_remote {
                                    let _ =
                                        tx_evt.send(EngineEvent::Log("[RTP] ssrc mismatch".into()));
                                    continue;
                                }
                                let arrival_ms = now_millis();
                                let ext = seqext.update(rtp.header.sequence_number);
                                if ext > highest_ext_seq {
                                    highest_ext_seq = ext;
                                }
                                recv_pkts = recv_pkts.wrapping_add(1);
                                // Jitter per RFC3550 A.8
                                let rtp_ts = rtp.header.timestamp;
                                let elapsed = self.t0.elapsed();
                                let arrival_rtp_units =
                                    ((elapsed.as_micros() as u64) * clock as u64 / 1_000_000)
                                        as u32;

                                let transit = arrival_rtp_units.wrapping_sub(rtp_ts);
                                if let Some(prev_t) = last_transit {
                                    let d = transit.wrapping_sub(prev_t);
                                    let d_abs = if transit >= prev_t {
                                        transit - prev_t
                                    } else {
                                        prev_t - transit
                                    };
                                    jitter = jitter.wrapping_add(
                                        ((d_abs as u64).saturating_sub(jitter as u64) / 16) as u32,
                                    );
                                }
                                last_transit = Some(transit);
                                let _ = tx_evt.send(EngineEvent::Payload(format!(
                                    "[RTP] {} bytes PT={} M={} seq={} ts={} ssrc={}",
                                    rtp.payload.len(),
                                    rtp.header.payload_type,
                                    rtp.header.marker,
                                    rtp.header.sequence_number,
                                    rtp.header.timestamp,
                                    rtp.header.ssrc
                                )));
                            }
                            Err(e) => {
                                let _ = tx_evt
                                    .send(EngineEvent::Log(format!("[RTP] decode error: {e}")));
                            }
                        }
                    }
                    Err(_) => {}
                }
            }
        });

        // === periodic RTCP sender (very simple) ===
        let run2 = self.run.clone();
        let tx_evt2 = self.tx_evt.clone();
        let sock = Arc::clone(&self.sock);
        let peer = self.peer;
        let ssrc_local = self.cfg.ssrc_local;
        let cname = self.cfg.cname.clone();
        let interval = self.cfg.rtcp_interval;
        let clock2 = self.cfg.clock_rate;
        std::thread::spawn(move || {
            while run2.load(Ordering::SeqCst) {
                std::thread::sleep(interval);
                // Build a tiny compound packet: RR (empty) + SDES(CNAME)
                let rr = ReceiverReport {
                    ssrc: ssrc_local,
                    reports: vec![],
                }
                .encode();
                let sdes = Sdes {
                    items: vec![SdesItem {
                        ssrc: ssrc_local,
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
    }

    /// One-shot demo payload sender: sends a single RTP packet with marker bit.
    pub fn send_demo_tick(&mut self) {
        let mut hdr = RtpHeader {
            payload_type: self.cfg.payload_type,
            ssrc: self.cfg.ssrc_local,
            sequence_number: self.seq,
            timestamp: self.ts,
            marker: true,
            ..Default::default()
        };
        let payload = b"tick".to_vec();
        let pkt = RtpPacket {
            header: hdr,
            payload,
        };
        let bytes = pkt.encode();
        let _ = self.sock.send_to(&bytes, self.peer);
        self.seq = self.seq.wrapping_add(1);
        self.ts = self.ts.wrapping_add(self.cfg.clock_rate / 50); // 20ms per packet
        self.pkt_count = self.pkt_count.wrapping_add(1);
        self.octet_count = self.octet_count.wrapping_add(4);
    }

    pub fn send_pli(&self) {
        let pli = rtcp::PictureLossIndication {
            sender_ssrc: self.cfg.ssrc_local,
            media_ssrc: self.cfg.ssrc_remote,
        };
        let bytes = pli.encode();
        let _ = self.sock.send_to(&bytes, self.peer);
        let _ = self.tx_evt.send(EngineEvent::Log("[RTCP] tx PLI".into()));
    }
}

fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

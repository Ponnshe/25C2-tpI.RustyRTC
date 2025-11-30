use crate::dtls_srtp::SrtpSessionConfig;
use crate::dtls_srtp::srtp_context::SrtpContext;
use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::Duration,
};

use super::{
    outbound_track_handle::OutboundTrackHandle, rtp_codec::RtpCodec,
    rtp_recv_config::RtpRecvConfig, rtp_recv_stream::RtpRecvStream, rtp_send_config::RtpSendConfig,
    rtp_send_stream::RtpSendStream, rtp_session_error::RtpSessionError,
};
use crate::{
    app::{log_level::LogLevel, log_sink::LogSink},
    core::events::EngineEvent,
    logger_debug, logger_error,
    rtcp::{
        packet_type::RtcpPacketType, receiver_report::ReceiverReport, report_block::ReportBlock,
        sdes::Sdes,
    },
    rtp::rtp_packet::RtpPacket,
    sink_debug, sink_error, sink_info, sink_log,
};
use crate::{
    media_transport::payload::rtp_payload_chunk::RtpPayloadChunk,
    rtcp::{picture_loss::PictureLossIndication, rtcp::RtcpPacket},
};
use rand::{RngCore, rngs::OsRng};

pub struct RtpSession {
    sock: Arc<UdpSocket>,
    peer: SocketAddr,

    recv_streams: Arc<Mutex<HashMap<u32, RtpRecvStream>>>, // key: remote_ssrc
    pending_recv: Arc<Mutex<Vec<RtpRecvStream>>>,          // remote_ssrc=None
    send_streams: Arc<Mutex<HashMap<u32, RtpSendStream>>>, // key: local_ssrc

    run: Arc<AtomicBool>,
    tx_evt: Sender<EngineEvent>,
    logger: Arc<dyn LogSink>,
    rx_media: Option<Receiver<Vec<u8>>>,

    local_rtcp_ssrc: u32,
    cname: String,
    rtcp_interval: Duration,
    //Srtp config
    srtp_cfg: Option<SrtpSessionConfig>,
    // Contextos SRTP protegidos por Mutex para acceso compartido
    srtp_inbound: Option<Arc<Mutex<SrtpContext>>>,
    srtp_outbound: Option<Arc<Mutex<SrtpContext>>>,
}

impl RtpSession {
    pub fn new(
        sock: Arc<UdpSocket>,
        peer: SocketAddr,
        tx_evt: Sender<EngineEvent>,
        logger: Arc<dyn LogSink>,
        rx_media: Receiver<Vec<u8>>,
        initial_recv: Vec<RtpRecvConfig>,
        initial_send: Vec<RtpSendConfig>,
        srtp_cfg: Option<SrtpSessionConfig>,
    ) -> Result<Self, RtpSessionError> {
        // Inicializar contextos SRTP si hay configuración
        let (srtp_inbound, srtp_outbound) = if let Some(srtp_session_cfg) = &srtp_cfg {
            (
                Some(Arc::new(Mutex::new(SrtpContext::new(
                    logger.clone(),
                    srtp_session_cfg.inbound.clone(),
                )))),
                Some(Arc::new(Mutex::new(SrtpContext::new(
                    logger.clone(),
                    srtp_session_cfg.outbound.clone(),
                )))),
            )
        } else {
            (None, None)
        };
        let this = Self {
            sock,
            peer,
            recv_streams: Arc::new(Mutex::new(HashMap::new())),
            pending_recv: Arc::new(Mutex::new(Vec::new())),
            send_streams: Arc::new(Mutex::new(HashMap::new())),
            run: Arc::new(AtomicBool::new(false)),
            tx_evt,
            logger,
            rx_media: Some(rx_media),
            local_rtcp_ssrc: OsRng.next_u32(),
            cname: "roomrtc@local".into(),
            rtcp_interval: Duration::from_millis(500),
            srtp_cfg,
            srtp_inbound,
            srtp_outbound,
        };

        this.add_recv_streams(initial_recv)?;
        let _ = this.add_send_streams(initial_send)?;

        Ok(this)
    }

    pub fn add_recv_stream(&self, cfg: RtpRecvConfig) -> Result<(), RtpSessionError> {
        let remote_ssrc = cfg.remote_ssrc;
        let st = RtpRecvStream::new(cfg, self.tx_evt.clone(), self.logger.clone());
        if let Some(ssrc) = remote_ssrc {
            self.recv_streams.lock()?.insert(ssrc, st);
        } else {
            self.pending_recv.lock()?.push(st);
        }
        Ok(())
    }

    pub fn add_recv_streams(&self, configs: Vec<RtpRecvConfig>) -> Result<(), RtpSessionError> {
        for cfg in configs {
            self.add_recv_stream(cfg)?;
        }
        Ok(())
    }

    pub fn add_send_stream(
        &self,
        rtp_send_config: RtpSendConfig,
    ) -> Result<OutboundTrackHandle, RtpSessionError> {
        let ssrc = rtp_send_config.local_ssrc;
        let codec = rtp_send_config.codec.clone();
        let st = RtpSendStream::new(
            self.logger.clone(),
            rtp_send_config,
            Arc::clone(&self.sock),
            self.peer,
            self.srtp_outbound.clone(),
        );
        self.send_streams.lock()?.insert(ssrc, st);
        Ok(OutboundTrackHandle {
            local_ssrc: ssrc,
            codec,
        })
    }

    pub fn add_send_streams(
        &self,
        configs: Vec<RtpSendConfig>,
    ) -> Result<Vec<OutboundTrackHandle>, RtpSessionError> {
        let mut handles = Vec::with_capacity(configs.len());
        for cfg in configs {
            handles.push(self.add_send_stream(cfg)?);
        }
        Ok(handles)
    }

    pub fn register_outbound_track(
        &self,
        codec: RtpCodec,
    ) -> Result<OutboundTrackHandle, RtpSessionError> {
        let cfg = RtpSendConfig::new(codec);
        self.add_send_stream(cfg)
    }

    pub fn start(&mut self) -> Result<(), RtpSessionError> {
        self.run.store(true, Ordering::SeqCst);

        // === inbound RTP/RTCP loop ===
        let run = Arc::clone(&self.run);
        let rx = self
            .rx_media
            .take()
            .ok_or(RtpSessionError::EmptyMediaReceiver)?;
        let recv_map = Arc::clone(&self.recv_streams);
        let send_map = Arc::clone(&self.send_streams);
        let pending_recv = Arc::clone(&self.pending_recv);
        let tx_evt = self.tx_evt.clone();
        let logger = self.logger.clone();
        let srtp_inbound = self.srtp_inbound.clone();

        thread::spawn(move || {
            while run.load(Ordering::SeqCst) {
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(mut pkt) => {
                        if pkt.len() < 2 {
                            sink_log!(&logger, LogLevel::Error, "[RTP] packet too short");
                            continue;
                        }

                        // ---- RTCP ----
                        if is_rtcp(&pkt) {
                            // TODO: Implement SRTCP unprotect here in the future.
                            // For now, pass cleartext or drop if peer encrypts RTCP.
                            if let Err(e) = handle_rtcp(
                                &pkt,
                                &recv_map,
                                &pending_recv,
                                &send_map,
                                &tx_evt,
                                &logger,
                            ) {
                                sink_log!(&logger, LogLevel::Error, "[RTCP] error: {e:?}");
                            }
                            continue;
                        }

                        // ---- RTP fast-path ----
                        if pkt.len() < 12 || (pkt[0] >> 6) != 2 {
                            sink_log!(&logger, LogLevel::Error, "[RTP] invalid header/version");
                            continue;
                        }

                        // 3. SRTP Unprotect
                        if let Some(ctx) = &srtp_inbound {
                            // Mutex lock, attempt unprotect
                            match ctx.lock().unwrap().unprotect(&mut pkt) {
                                Ok(_) => {
                                    // Success: pkt is now cleartext RTP
                                }
                                Err(e) => {
                                    sink_log!(
                                        &logger,
                                        LogLevel::Warn,
                                        "[SRTP] Unprotect failed: {}",
                                        e
                                    );
                                    // Drop the packet! Do not try to parse garbage.
                                    continue;
                                }
                            }
                        }

                        // Decode RTP (adapt if your API returns Result)
                        let Ok(rtp) = RtpPacket::decode(&pkt) else {
                            sink_log!(&logger, LogLevel::Error, " RTP] decode failed");
                            continue;
                        };

                        sink_info!(logger, "[RTP Session] Received RTP packet");

                        let ssrc = rtp.ssrc();
                        let pt = rtp.payload_type();

                        // 1) Known stream?
                        if let Ok(mut guard) = recv_map.lock()
                            && let Some(st) = guard.get_mut(&ssrc)
                        {
                            st.receive_rtp_packet(rtp);
                            continue;
                        }

                        // 2) Bind a pending stream by PT, then move it to the map
                        if let Ok(mut pend) = pending_recv.lock() {
                            if let Some(idx) = pend.iter().position(|s| s.codec.payload_type == pt)
                            {
                                let mut st = pend.swap_remove(idx);
                                st.remote_ssrc = Some(ssrc);
                                st.receive_rtp_packet(rtp);
                                if let Ok(mut map) = recv_map.lock() {
                                    map.insert(ssrc, st);
                                }
                                continue;
                            } else {
                                sink_log!(
                                    &logger,
                                    LogLevel::Warn,
                                    "[RTP] couldn't map codec to payload type on the pool of pending receivers: {pt}"
                                );
                            }
                        }

                        // 3) Unknown SSRC/PT
                        //

                        sink_log!(
                            &logger,
                            LogLevel::Warn,
                            "[RTP] unknown remote SSRC={:#010x} PT={}",
                            ssrc,
                            pt
                        );
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        sink_debug!(logger, "[RTP Session] Received nothing in timeout");
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        sink_error!(logger, "[RTP Session] Disconnected");
                    }
                }
            }
        });

        // === periodic RTCP sender: SR, RR, SDES ===
        let run2 = Arc::clone(&self.run);
        let sock = Arc::clone(&self.sock);
        let peer = self.peer;
        let recv_map2 = Arc::clone(&self.recv_streams);
        let send_map2 = Arc::clone(&self.send_streams);
        let _tx_evt2 = self.tx_evt.clone();
        let logger2 = self.logger.clone();
        let interval = self.rtcp_interval;
        let rr_ssrc = self.local_rtcp_ssrc;
        let cname = self.cname.clone();

        thread::spawn(move || {
            while run2.load(Ordering::SeqCst) {
                std::thread::sleep(interval);

                let mut comp_pkt = Vec::new();

                // Build Sender Reports (SR) for each sending stream ---
                if let Ok(mut guard) = send_map2.lock() {
                    for st in guard.values_mut() {
                        if let Some(sr) = st.maybe_build_sr() {
                            let mut sr_bytes = Vec::new();
                            if let Err(e) = sr.encode_into(&mut sr_bytes) {
                                logger_error!(logger2, "[RTCP] failed to encode SR: {e}");
                                continue;
                            }

                            comp_pkt.extend_from_slice(&sr_bytes);

                            logger_debug!(
                                logger2,
                                "[RTCP] tx built SR ssrc={:#010x}",
                                st.local_ssrc
                            );
                        }
                    }
                }

                // Build one Receiver Report (RR) for all receiving streams ---
                let mut blocks: Vec<ReportBlock> = Vec::new();
                if let Ok(mut guard) = recv_map2.lock() {
                    for st in guard.values_mut() {
                        if let Some(rb) = st.build_report_block() {
                            blocks.push(rb);
                        }
                    }
                }

                // Only send RR if there are blocks. If we are a pure sender, we might not have any.
                if !blocks.is_empty() {
                    let rr = ReceiverReport::new(rr_ssrc, blocks);
                    let mut rr_bytes = Vec::new();
                    if let Err(e) = rr.encode_into(&mut rr_bytes) {
                        logger_error!(logger2, "[RTCP] failed to encode RR: {e}");
                    } else {
                        comp_pkt.extend_from_slice(&rr_bytes);
                        logger_debug!(logger2, "[RTCP] tx built RR");
                    }
                }

                // --- 3) Build SDES with CNAME ---
                // Note: could be conditional if you only want to send it once or twice.
                let sdes = Sdes::cname(rr_ssrc, cname.clone());
                let mut sdes_bytes = Vec::new();
                if let Err(e) = sdes.encode_into(&mut sdes_bytes) {
                    logger_error!(logger2, "[RTCP] failed to encode SDES: {e}");
                } else {
                    comp_pkt.extend_from_slice(&sdes_bytes);
                }

                // --- 4) Send compound packet if not empty ---
                if !comp_pkt.is_empty() {
                    let _ = sock.send_to(&comp_pkt, peer);
                }
            }
        });

        Ok(())
    }

    pub fn stop(&self) {
        self.run.store(false, Ordering::SeqCst);
    }

    /// Send PLI for a specific remote source.
    pub fn send_pli(&self, remote_ssrc: u32) {
        let pli = PictureLossIndication::new(self.local_rtcp_ssrc, remote_ssrc);
        let mut buf = Vec::new();
        let _ = pli.encode_into(&mut buf);
        let _ = self.sock.send_to(&buf, self.peer);
        logger_debug!(self.logger, "[RTCP] tx sent PLI media_ssrc={remote_ssrc}");
    }

    /// Convenience: does this remote SSRC exist as a recv stream?
    pub fn has_recv_ssrc(&self, remote_ssrc: u32) -> bool {
        self.recv_streams.lock().unwrap().contains_key(&remote_ssrc)
    }

    pub fn send_rtp_payload(
        &self,
        local_ssrc: u32,
        payload: &[u8],
        timestamp: u32,
        marker: bool,
    ) -> Result<(), RtpSessionError> {
        let mut g = self.send_streams.lock()?;
        let st = g
            .get_mut(&local_ssrc)
            .ok_or(RtpSessionError::SendStreamMissing { ssrc: local_ssrc })?;
        st.send_rtp_payload(payload, timestamp, marker)
            .map_err(|source| RtpSessionError::SendStream {
                source,
                ssrc: local_ssrc,
            })
    }

    pub fn send_rtp_chunks_for_frame(
        &self,
        local_ssrc: u32,
        chunks: &[RtpPayloadChunk],
        timestamp: u32,
    ) -> Result<(), RtpSessionError> {
        let mut g = self.send_streams.lock()?;
        let st = g
            .get_mut(&local_ssrc)
            .ok_or(RtpSessionError::SendStreamMissing { ssrc: local_ssrc })?;

        for ch in chunks {
            st.send_rtp_payload(&ch.bytes, timestamp, ch.marker)
                .map_err(|source| RtpSessionError::SendStream {
                    source,
                    ssrc: local_ssrc,
                })?;
        }
        Ok(())
    }
}

// --------------------- helpers ---------------------

#[inline]
fn is_rtcp(pkt: &[u8]) -> bool {
    // RTCP header is at least 4 bytes; first byte contains version in top 2 bits
    if pkt.len() < 4 {
        return false;
    }

    let version = pkt[0] >> 6;
    if version != 2 {
        return false;
    } // expect RTP/RTCP v2

    // pkt[1] is the RTCP packet type (8 bits) for RTCP packets
    matches!(pkt[1], 200..=206)
}

#[inline]
fn ntp_to_compact(msw: u32, lsw: u32) -> u32 {
    (msw << 16) | (lsw >> 16)
}

fn handle_rtcp(
    buf: &[u8],
    recv_map: &Arc<Mutex<HashMap<u32, RtpRecvStream>>>,
    pending_recv: &Arc<Mutex<Vec<RtpRecvStream>>>,
    send_map: &Arc<Mutex<HashMap<u32, RtpSendStream>>>,
    tx_evt: &Sender<EngineEvent>,
    logger: &Arc<dyn LogSink>,
) -> Result<(), RtpSessionError> {
    // Decode all RTCP packets in the compound
    let pkts: Vec<RtcpPacket> = RtcpPacket::decode_compound(buf)?;

    // Arrival time for RTT calculus (compact NTP) and for SR anchoring (full NTP)
    let (now_most_sw, now_least_sw) = crate::rtp_session::time::ntp_now();
    let arrival_ntp_compact = ntp_to_compact(now_most_sw, now_least_sw);

    for pkt in pkts {
        match pkt {
            RtcpPacket::Sr(sr) => {
                // 1) SR → recv stream (anchors LSR/DLSR clock)
                if let Ok(mut g) = recv_map.lock() {
                    if let Some(st) = g.get_mut(&sr.ssrc) {
                        st.on_sender_report(sr.ssrc, &sr.info, (now_most_sw, now_least_sw));
                    } else {
                        // (Optional) if you want to bind a pending recv purely on SR (no RTP yet),
                        // you could try heuristic binding here. Generally better to wait for RTP.
                    }
                }

                // 2) Embedded report blocks → sender streams (outbound metrics/RTT)
                if let Ok(mut g) = send_map.lock() {
                    for rb in &sr.reports {
                        if let Some(st) = g.get_mut(&rb.ssrc) {
                            if let Some(metrics) = st.on_report_block(rb, arrival_ntp_compact) {
                                let _ = tx_evt.send(EngineEvent::NetworkMetrics(metrics));
                            }
                        }
                    }
                }
            }

            RtcpPacket::Rr(rr) => {
                // Each report block targets one of our *sender* SSRCs
                if let Ok(mut g) = send_map.lock() {
                    for rb in &rr.reports {
                        if let Some(st) = g.get_mut(&rb.ssrc) {
                            if let Some(metrics) = st.on_report_block(rb, arrival_ntp_compact) {
                                let _ = tx_evt.send(EngineEvent::NetworkMetrics(metrics));
                            }
                        }
                    }
                }
            }

            RtcpPacket::Sdes(sdes) => {
                // Optional: keep SSRC → CNAME mapping at session level
                sink_log!(
                    logger,
                    LogLevel::Debug,
                    "[RTCP][SDES] chunks={}",
                    sdes.chunks.len()
                )
            }

            RtcpPacket::Bye(bye) => {
                // Tear down any recv streams for the listed sources
                if let Ok(mut g) = recv_map.lock() {
                    for ssrc in &bye.sources {
                        if g.remove(ssrc).is_some() {
                            let _ = tx_evt.send(EngineEvent::Status(format!(
                                "[RTCP][BYE] removed recv stream ssrc={:#010x}",
                                ssrc
                            )));
                        }
                    }
                }
                // (Optional) also clear any pending that somehow bound to these sources
                if let Ok(mut pend) = pending_recv.lock() {
                    pend.retain(|_| true); // no-op; adjust if you track identities there
                }
            }

            RtcpPacket::Pli(pli) => {
                // Inbound PLI means the remote wants a keyframe for media_ssrc
                // Route to the *sender* stream of that SSRC, or surface an event:
                sink_log!(
                    logger,
                    LogLevel::Debug,
                    "[RTCP][PLI] keyframe requested for ssrc={:#010x}",
                    pli.media_ssrc
                )
                // If you have encoder wiring, signal it here.
            }

            RtcpPacket::Nack(nack) => {
                // Inbound NACK asks us to retransmit lost seqnos on media_ssrc
                // Route to the *sender* stream (implement your RTX/repair path there)
                sink_log!(
                    logger,
                    LogLevel::Debug,
                    "[RTCP][NACK] for media_ssrc={:#010x} fci_count={}",
                    nack.media_ssrc,
                    nack.entries.len()
                )
            }

            RtcpPacket::App(_app) => {
                sink_log!(logger, LogLevel::Debug, "[RTCP][APP] ignored")
            }
        }
    }

    Ok(())
}

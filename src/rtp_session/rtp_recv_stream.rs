use crate::{core::events::EngineEvent, rtp::rtp_codec::RtpCodec};

use super::rx_tracker::RxTracker;
use std::{
    net::{SocketAddr, UdpSocket}, sync::{mpsc::Sender, Arc}, time::Instant
};

pub struct RtpRecvStream {
    pub codec: RtpCodec,
    pub remote_ssrc: Option<u32>,
    pub rx: RxTracker,              // your existing tracker
    last_activity: Instant,

    sock: Arc<UdpSocket>,           // keep if you will send RTCP from here; otherwise from session
    peer: SocketAddr,
    tx_evt: Sender<EngineEvent>,
}

impl RtpRecvStream {
    pub fn handle_packet(&mut self, pkt: &[u8]) {
        // parse minimal RTP header, extract seq/ts
        let seq = u16::from_be_bytes([pkt[2], pkt[3]]);
        let ts  = u32::from_be_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
        let arrival_units = rtp_units_now(self.clock_rate);
        self.rx.on_rtp(seq, ts, arrival_units);
        self.last_activity = Instant::now();
    }

    pub fn on_sr(&mut self, ntp_secs: u32, ntp_frac: u32, now_ntp: (u32,u32)) {
        self.rx.on_sr_received(ntp_secs, ntp_frac, now_ntp);
    }

    pub fn build_rb(&mut self) -> ReportBlock {
        self.rx.build_report_block(self.remote_ssrc)
    }
}

impl RtpRecvStream {
    pub fn new(cfg: RtpRecvConfig, sock: Arc<UdpSocket>, peer: SocketAddr, tx_evt: Sender<EngineEvent>) -> Self {
        Self {
            codec: cfg.codec,
            remote_ssrc: cfg.remote_ssrc,
            rx: RxTracker::default(),
            last_activity: 0,
            sock,
            peer, 
            tx_evt,
        }
    }

    pub fn handle_packet(&mut self, pkt: &[u8]) {
        if pkt.len() < 12 || (pkt[0] >> 6) != 2 { return; }
        let seq = u16::from_be_bytes([pkt[2], pkt[3]]);
        let ts  = u32::from_be_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
        let ssrc = u32::from_be_bytes([pkt[8], pkt[9], pkt[10], pkt[11]]);
        if self.remote_ssrc.is_none() { self.remote_ssrc = Some(ssrc); }
        let arrival = rtp_units_now(self.codec.clock_rate);
        self.rx.on_rtp(seq, ts, arrival);
    }

    pub fn build_report_block(&mut self) -> Option<ReportBlock> {
        self.remote_ssrc.map(|ssrc| self.rx.build_report_block(ssrc))
    }

    pub fn on_sr(&mut self, ntp_msw: u32, ntp_lsw: u32, now_ntp: (u32,u32)) {
        self.rx.on_sr_received(ntp_msw, ntp_lsw, now_ntp);
    }
}

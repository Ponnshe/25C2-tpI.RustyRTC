use super::{
    connection_error::ConnectionError, ice_and_sdp::ICEAndSDP, ice_phase::IcePhase,
    outbound_sdp::OutboundSdp, rtp_map::RtpMap, signaling_state::SignalingState,
};
use crate::connection_manager::ice_worker::IceWorker;
use crate::ice::type_ice::ice_agent::{IceAgent, IceRole};
use crate::ice::{
    gathering_service,
    type_ice::ice_agent::IceRole::{Controlled, Controlling},
};
use crate::rtp_session::rtp_codec::RtpCodec;
use crate::sdp::addr_type::AddrType as SDPAddrType;
use crate::sdp::attribute::Attribute as SDPAttribute;
use crate::sdp::connection::Connection as SDPConnection;
use crate::sdp::media::Media as SDPMedia;
use crate::sdp::media::MediaKind as SDPMediaKind;
use crate::sdp::origin::Origin as SDPOrigin;
use crate::sdp::port_spec::PortSpec as SDPPortSpec;
use crate::sdp::sdpc::Sdp;
use crate::sdp::time_desc::TimeDesc as SDPTimeDesc;
use std::collections::HashSet;
use std::{
    io::ErrorKind,
    net::UdpSocket,
    sync::Arc,
    time::{Duration, Instant},
};

const DEFAULT_PORT: u16 = 9;
const DEAFULT_PROTO: &str = "UDP/TLS/RTP/SAVPF";
const DEFAULT_FMT: &str = "96";
const DEFAULT_NET_TYPE: &str = "IN";
const DEFAULT_ADDR_TYPE: SDPAddrType = SDPAddrType::IP4;
const DEFAULT_CONN_ADDR: &str = "0.0.0.0";
const DEFAULT_MEDIA_KIND: SDPMediaKind = SDPMediaKind::Video;

// ----------------- ConnectionManager --------------------
pub struct ConnectionManager {
    pub ice_agent: IceAgent,
    signaling: SignalingState,
    local_description: Option<Sdp>,
    remote_description: Option<Sdp>,
    ice_phase: IcePhase,
    //local_codecs: Vec<RtpCodec>,
    remote_codecs: Vec<RtpCodec>,
    ice_worker: Option<IceWorker>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        let ice_agent = IceAgent::new(IceRole::Controlling);
        Self {
            ice_agent,
            signaling: SignalingState::Stable,
            local_description: None,
            remote_description: None,
            ice_phase: IcePhase::Idle,
            //local_codecs,
            remote_codecs: vec![],
            ice_worker: None,
        }
    }

    /// UI calls this to start (re)negotiation. Returns an SDP **Offer** to send.
    pub fn negotiate(&mut self) -> Result<OutboundSdp, ConnectionError> {
        match self.signaling {
            SignalingState::Stable => {
                let offer = self.build_local_sdp()?; // same builder for offer/answer
                self.local_description = Some(offer.clone());
                self.signaling = SignalingState::HaveLocalOffer;
                self.set_ice_role_from_signaling(true, /*remote_is_ice_lite=*/ false);
                Ok(OutboundSdp::Offer(offer))
            }
            SignalingState::HaveLocalOffer => {
                // Already negotiating; nothing to do.
                Ok(OutboundSdp::None)
            }
            SignalingState::HaveRemoteOffer => {
                // We owe an answer; refuse to start a new offer.
                Err(ConnectionError::Negotiation(
                    "cannot create offer while have-remote-offer".into(),
                ))
            }
            SignalingState::Closed => Err(ConnectionError::Negotiation("connection closed".into())),
        }
    }

    /// UI passes *any* remote SDP here (offer or answer). We decide what it is by state.
    /// - If Stable: treat it as **Offer**, store it, generate **Answer** and return it.
    /// - If HaveLocalOffer: treat it as **Answer**, store it, return None.
    /// - If HaveRemoteOffer: receiving another SDP is unexpected (unless you add rollbacks/pranswers).
    pub fn apply_remote_sdp(&mut self, remote: &str) -> Result<OutboundSdp, ConnectionError> {
        let sdp = Sdp::parse(remote).map_err(ConnectionError::Sdp)?;
        let out = {
            match self.signaling {
                SignalingState::Stable => {
                    // Treat as remote offer.
                    let (remote_is_ice_lite, _ufrag, _pwd) =
                        self.extract_and_store_remote_ice_meta(&sdp)?;
                    self.extract_and_store_rtp_meta(&sdp)?;
                    self.remote_description = Some(sdp);
                    self.signaling = SignalingState::HaveRemoteOffer;

                    // Build and send local answer.
                    let answer = self.build_local_sdp()?;
                    self.local_description = Some(answer.clone());
                    // Answerer → Controlled (unless peer is ice-lite → we must be Controlling)
                    self.set_ice_role_from_signaling(false, remote_is_ice_lite);

                    self.signaling = SignalingState::Stable; // back to stable
                    Ok(OutboundSdp::Answer(answer))
                }
                SignalingState::HaveLocalOffer => {
                    // Glare handling (optional)
                    if is_probably_offer(&sdp) {
                        self.rollback_to_stable();
                        return self.apply_remote_sdp(remote);
                    }
                    // Treat as answer
                    // It’s an ANSWER → parse ICE meta & candidates
                    let (_remote_is_ice_lite, _ufrag, _pwd) =
                        self.extract_and_store_remote_ice_meta(&sdp)?;
                    self.extract_and_store_rtp_meta(&sdp)?;
                    self.remote_description = Some(sdp);
                    self.signaling = SignalingState::Stable;
                    Ok(OutboundSdp::None)
                }
                SignalingState::HaveRemoteOffer => Err(ConnectionError::Negotiation(
                    "unexpected SDP while have-remote-offer (answer was not sent yet)".into(),
                )),
                SignalingState::Closed => {
                    Err(ConnectionError::Negotiation("connection closed".into()))
                }
            }
        };

        if out.is_ok() {
            if let Err(e) = self.maybe_start_ice() {
                eprintln!("ICE start failed: {e}");
            }
        }
        out
    }

    pub fn set_pts_and_codecs_for_rtp(&mut self, codecs: Vec<RtpCodec>) {
        //self.codecs.push(codecs);
        todo!();
    }

    pub fn extract_and_store_rtp_meta(&mut self, remote_sdp: &Sdp) -> Result<(), ConnectionError> {
        let mut discovered: Vec<RtpCodec> = Vec::new();

        for m in remote_sdp.media() {
            // Skip non-RTP media
            if !m.proto().to_uppercase().contains("RTP") {
                continue;
            }

            // Parse allowed payload types from the m-line formats
            let allowed_pts: HashSet<u8> = m
                .fmts()
                .iter()
                .filter_map(|fmt| fmt.parse::<u8>().ok())
                .collect();

            // Scan attributes for a=rtpmap
            for a in m.attrs() {
                if a.key() != "rtpmap" {
                    continue;
                }
                let raw = a
                    .value()
                    .ok_or(ConnectionError::RtpMap("Wrong value".into()))?;
                let rm: RtpMap = raw.parse().map_err(|_| {
                    ConnectionError::RtpMap(
                        "No se pudo parsear el valor del atributo rtpmap".into(),
                    )
                })?;

                // Keep only if PT is listed in this media’s fmt list (when present)
                if !allowed_pts.is_empty() && !allowed_pts.contains(&rm.payload_type) {
                    continue;
                }

                // Record as RtpCodec
                discovered.push(RtpCodec::new(rm.payload_type, rm.clock_rate));
            }
        }

        // Dedup by payload_type (first occurrence wins)
        discovered.sort_by_key(|c| c.payload_type);
        discovered.dedup_by_key(|c| c.payload_type);

        self.remote_codecs = discovered;
        Ok(())
    }
    /// separate method if we ever need to apply only candidates from a remote trickle.
    pub fn apply_remote_trickle_candidate(
        &mut self,
        cand_attr_line: &str,
    ) -> Result<(), ConnectionError> {
        let ice_and_sdp: ICEAndSDP = cand_attr_line
            .parse()
            .map_err(|_| ConnectionError::IceAgent)?;
        self.ice_agent.add_remote_candidate(ice_and_sdp.candidate());
        Ok(())
    }

    pub fn set_ice_agent(&mut self, ice_agent: IceAgent) {
        self.ice_agent = ice_agent;
    }

    // ----------------- Helpers -----------------

    /// Build a local SDP (used for both offer and answer in this simple model).
    fn build_local_sdp(&mut self) -> Result<Sdp, ConnectionError> {
        let version: u8 = 0;
        let origin = SDPOrigin::new_blank();
        let session_name = "demo_session".to_owned();
        let session_info = None;
        let uri = None;
        let emails = Vec::new();
        let phones = Vec::new();
        let connection = None;
        let bandwidth = Vec::new();
        let times: Vec<SDPTimeDesc> = vec![SDPTimeDesc::new_blank()];
        let attrs = Vec::new();
        let media: Vec<SDPMedia> = vec![get_mocked_media_description(self)?];
        let extra_lines = Vec::new();
        Ok(Sdp::new(
            version,
            origin,
            session_name,
            session_info,
            uri,
            emails,
            phones,
            connection,
            bandwidth,
            times,
            attrs,
            media,
            extra_lines,
        ))
    }

    fn rollback_to_stable(&mut self) {
        // Drop the local offer; keep remote side clear.
        self.local_description = None;
        self.signaling = SignalingState::Stable;
    }

    fn extract_and_store_remote_ice_meta(
        &mut self,
        remote: &Sdp,
    ) -> Result<(bool, Option<String>, Option<String>), ConnectionError> {
        let mut remote_is_ice_lite = false;
        let mut ufrag: Option<String> = None;
        let mut pwd: Option<String> = None;

        // Session-level attributes (important: ice-lite is usually here)
        for a in remote.attrs() {
            match a.key() {
                "ice-lite" => remote_is_ice_lite = true,
                "ice-ufrag" => {
                    if let Some(v) = a.value() {
                        ufrag = Some(v.to_owned());
                    }
                }
                "ice-pwd" => {
                    if let Some(v) = a.value() {
                        pwd = Some(v.to_owned());
                    }
                }
                _ => {}
            }
        }

        // Media-level attributes (fallbacks + candidates)
        for m in remote.media() {
            for a in m.attrs() {
                match a.key() {
                    "candidate" => {
                        let value = a.value().ok_or(ConnectionError::IceAgent)?;
                        let ice_and_sdp: ICEAndSDP =
                            value.parse().map_err(|_| ConnectionError::IceAgent)?;
                        self.ice_agent.add_remote_candidate(ice_and_sdp.candidate());
                    }
                    "ice-ufrag" => {
                        if ufrag.is_none() {
                            if let Some(v) = a.value() {
                                ufrag = Some(v.to_owned());
                            }
                        }
                    }
                    "ice-pwd" => {
                        if pwd.is_none() {
                            if let Some(v) = a.value() {
                                pwd = Some(v.to_owned());
                            }
                        }
                    }
                    "ice-lite" => remote_is_ice_lite = true,
                    _ => {}
                }
            }
        }

        // store credentials on the agent when we add setters/fields
        // if let Some(u) = &ufrag { self.ice_agent.set_remote_ufrag(u.clone()); }
        // if let Some(p) = &pwd   { self.ice_agent.set_remote_pwd(p.clone()); }

        Ok((remote_is_ice_lite, ufrag, pwd))
    }

    // Call this at the end of any path that leaves you with both local+remote descriptions.
    fn maybe_start_ice(&mut self) -> Result<(), ConnectionError> {
        let ready = self.local_description.is_some()
            && self.remote_description.is_some()
            && matches!(self.signaling, SignalingState::Stable);
        if !ready {
            return Ok(());
        }
        self.start_connectivity_checks()
    }

    /// Runs ICE pipeline to nomination (blocking version for clarity).
    pub fn start_connectivity_checks(&mut self) -> Result<(), ConnectionError> {
        if self.local_description.is_none() || self.remote_description.is_none() {
            return Err(ConnectionError::Negotiation(
                "SDP not complete for ICE".into(),
            ));
        }

        self.ice_phase = IcePhase::Gathering;
        if self.ice_agent.local_candidates.is_empty() {
            self.ice_agent
                .gather_candidates()
                .map_err(|_| ConnectionError::IceAgent)?;
        }

        self.ice_phase = IcePhase::Checking;
        self.ice_agent.form_candidate_pairs();

        // Fire once; worker will keep re-sending
        self.ice_agent.start_checks();

        // Spawn background worker; return immediately (so Answer can be shown)
        self.spawn_ice_worker();
        Ok(())
    }
    fn set_ice_role_from_signaling(&mut self, we_are_offerer: bool, remote_is_ice_lite: bool) {
        self.ice_agent.role = if remote_is_ice_lite {
            // Full agent must be controlling against an ICE-Lite peer
            Controlling
        } else if we_are_offerer {
            Controlling
        } else {
            Controlled
        };
    }

    pub fn run_ice_reactor_blocking(&mut self, total_ms: u64) {
        let deadline = Instant::now() + Duration::from_millis(total_ms);

        // Take a snapshot of sockets to avoid borrowing self.ice_agent during the loop
        let sockets: Vec<Arc<UdpSocket>> = self
            .ice_agent
            .local_candidates
            .iter()
            .filter_map(|c| c.socket.clone())
            .collect();

        // Make reads snappy
        for sock in &sockets {
            let _ = sock.set_read_timeout(Some(Duration::from_millis(40)));
        }

        let mut buf = [0u8; 1500];

        while Instant::now() < deadline && self.ice_agent.nominated_pair.is_none() {
            for sock in &sockets {
                match sock.recv_from(&mut buf) {
                    Ok((n, from)) => {
                        self.ice_agent.handle_incoming_packet(&buf[..n], from);
                        if self.ice_agent.nominated_pair.is_some() {
                            break;
                        }
                    }
                    Err(ref e)
                        if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut =>
                    {
                        // keep spinning
                    }
                    Err(e) => {
                        eprintln!("ICE reactor recv error: {e}");
                    }
                }
            }
        }
    }
    fn spawn_ice_worker(&mut self) {
        if self.ice_worker.is_some() {
            return;
        }
        self.ice_worker = Some(IceWorker::spawn(&self.ice_agent));
    }

    fn stop_ice_worker(&mut self) {
        if let Some(w) = &mut self.ice_worker {
            w.stop();
        }
        self.ice_worker = None;
    }

    pub fn drain_ice_events(&mut self) {
        if let Some(w) = &self.ice_worker {
            while let Some((pkt, from)) = w.try_recv() {
                self.ice_agent.handle_incoming_packet(&pkt, from);
            }
        }
        if self.ice_agent.nominated_pair.is_some() && !matches!(self.ice_phase, IcePhase::Nominated)
        {
            self.ice_phase = IcePhase::Nominated;
            self.stop_ice_worker(); // optional: stop once nominated
        }
    }

    pub fn remote_codecs(&self) -> &Vec<RtpCodec> {
        &self.remote_codecs
    }
}

// Heuristic to decide if an SDP looks like an "offer" when we need to disambiguate during glare.
// In strict O/A, "offer vs answer" is *context*, not content; this is best-effort only.
fn is_probably_offer(_sdp: &Sdp) -> bool {
    // Keep simple for now; we can refine (e.g., look at a=setup:actpass vs passive/active, or dtls role).
    false
}

// -----------------  helpers -----------------

fn get_mocked_media_description(
    conn_manager: &mut ConnectionManager,
) -> Result<SDPMedia, ConnectionError> {
    let mut media_desc = SDPMedia::new_blank();
    media_desc.set_kind(DEFAULT_MEDIA_KIND);
    let port_spec_sdp = SDPPortSpec::new(DEFAULT_PORT, None);
    media_desc.set_port(port_spec_sdp);
    media_desc.set_proto(DEAFULT_PROTO);
    let fmts = vec![DEFAULT_FMT.to_owned()];
    media_desc.set_fmts(fmts);
    let connection_sdp = SDPConnection::new(DEFAULT_NET_TYPE, DEFAULT_ADDR_TYPE, DEFAULT_CONN_ADDR);
    media_desc.set_connection(Some(connection_sdp));
    let mut attrs = get_local_candidates_as_attributes(conn_manager);
    let (ufrag, pwd) = conn_manager.ice_agent.local_credentials();
    attrs.push(SDPAttribute::new("ice-ufrag", ufrag));
    attrs.push(SDPAttribute::new("ice-pwd", pwd));
    // Codec lines, mux
    attrs.push(SDPAttribute::new("rtpmap", Some("96 VP8/90000".to_owned())));
    attrs.push(SDPAttribute::new("rtcp-mux", Some("".to_owned())));
    media_desc.set_attrs(attrs);
    Ok(media_desc)
}

fn get_local_candidates_as_attributes(conn_manager: &mut ConnectionManager) -> Vec<SDPAttribute> {
    gathering_service::gather_host_candidates()
        .into_iter()
        .map(|c| {
            let ice_cand_to_sdp = ICEAndSDP::new(c);
            let attr = SDPAttribute::new("candidate", ice_cand_to_sdp.to_string());
            conn_manager
                .ice_agent
                .add_local_candidate(ice_cand_to_sdp.candidate());
            attr
        })
        .collect::<Vec<SDPAttribute>>()
}

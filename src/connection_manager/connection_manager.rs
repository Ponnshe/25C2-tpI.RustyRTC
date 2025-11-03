use super::{
    connection_error::ConnectionError, ice_and_sdp::ICEAndSDP, ice_phase::IcePhase,
    outbound_sdp::OutboundSdp, rtp_map::RtpMap, signaling_state::SignalingState,
};
use crate::app::log_sink::LogSink;
use crate::connection_manager::config::{
    DEFAULT_ADDR_TYPE, DEFAULT_CONN_ADDR, DEFAULT_FMT, DEFAULT_MEDIA_KIND, DEFAULT_NET_TYPE,
    DEFAULT_PORT, DEFAULT_PROTO,
};
use crate::connection_manager::ice_worker::IceWorker;
use crate::ice::gathering_service;
use crate::ice::type_ice::ice_agent::{IceAgent, IceRole};
use crate::media_agent::codec_descriptor::CodecDescriptor;
use crate::rtp_session::rtp_codec::RtpCodec;
use crate::sdp::attribute::Attribute as SDPAttribute;
use crate::sdp::connection::Connection as SDPConnection;
use crate::sdp::media::Media as SDPMedia;
use crate::sdp::origin::Origin as SDPOrigin;
use crate::sdp::port_spec::PortSpec as SDPPortSpec;
use crate::sdp::sdpc::Sdp;
use crate::sdp::time_desc::TimeDesc as SDPTimeDesc;
use crate::sink_error;

use std::collections::HashSet;
use std::{
    io::ErrorKind,
    net::UdpSocket,
    sync::Arc,
    time::{Duration, Instant},
};

/// Manages ICE, SDP negotiation, and RTP codec configuration for a single peer connection.
///
/// Handles:
/// - Local and remote SDP storage
/// - ICE candidate gathering and connectivity checks
/// - RTP codec negotiation
/// - SDP offer/answer flow control
// ----------------- ConnectionManager --------------------
pub struct ConnectionManager {
    pub logger_handle: Arc<dyn LogSink>,
    pub ice_agent: IceAgent,
    /// Current signaling state of the connection (`Stable`, `HaveLocalOffer`, etc.)
    signaling: SignalingState,
    /// Local SDP description (offer or answer)
    local_description: Option<Sdp>,
    /// Remote SDP description (offer or answer)
    remote_description: Option<Sdp>,
    /// Current ICE state
    ice_phase: IcePhase,
    /// RTP codecs supported locally
    local_codecs: Vec<CodecDescriptor>,
    /// RTP codecs advertised by the remote peer
    remote_codecs: Vec<RtpCodec>,
    /// Background ICE worker handling connectivity asynchronously
    ice_worker: Option<IceWorker>,
}

impl ConnectionManager {
    /// Constructs a new `ConnectionManager` with default values.
    ///
    /// The ICE agent is initialized in the `Controlling` role.
    #[must_use]
    pub fn new(logger_handle: Arc<dyn LogSink>) -> Self {
        let ice_agent = IceAgent::with_logger(IceRole::Controlling, logger_handle.clone());
        Self {
            logger_handle,
            ice_agent,
            signaling: SignalingState::Stable,
            local_description: None,
            remote_description: None,
            ice_phase: IcePhase::Idle,
            local_codecs: Vec::new(),
            remote_codecs: vec![],
            ice_worker: None,
        }
    }

    /// Initiates a new SDP negotiation as an **offerer**.
    ///
    /// Returns an SDP `Offer` to be sent to the remote peer.
    ///
    /// # Errors
    ///
    /// - If already in the `HaveRemoteOffer` state, returns `ConnectionError::Negotiation`.
    /// - If the connection is closed.
    pub fn negotiate(&mut self) -> Result<OutboundSdp, ConnectionError> {
        match self.signaling {
            SignalingState::Stable => {
                let offer = self.build_local_sdp();
                self.local_description = Some(offer.clone());
                self.signaling = SignalingState::HaveLocalOffer;
                self.set_ice_role_from_signaling(true, false);
                Ok(OutboundSdp::Offer(offer))
            }
            SignalingState::HaveLocalOffer => Ok(OutboundSdp::None),
            SignalingState::HaveRemoteOffer => Err(ConnectionError::Negotiation(
                "cannot create offer while have-remote-offer".into(),
            )),
            SignalingState::Closed => Err(ConnectionError::Negotiation("connection closed".into())),
        }
    }

    /// Applies a remote SDP (offer or answer) received from the peer.
    ///
    /// Determines the type based on signaling state:
    /// - `Stable` → treat as **Offer** → generate and return **Answer**
    /// - `HaveLocalOffer` → treat as **Answer** → store and return None
    /// - `HaveRemoteOffer` → error
    ///
    /// # Errors
    ///
    /// - If SDP parsing fails
    /// - If negotiation state is invalid
    pub fn apply_remote_sdp(&mut self, remote: &str) -> Result<OutboundSdp, ConnectionError> {
        let sdp = Sdp::parse(remote).map_err(ConnectionError::Sdp)?;
        let out = match self.signaling {
            SignalingState::Stable => {
                let (remote_is_ice_lite, _ufrag, _pwd) =
                    self.extract_and_store_remote_ice_meta(&sdp)?;
                self.extract_and_store_rtp_meta(&sdp)?;
                self.remote_description = Some(sdp);
                self.signaling = SignalingState::HaveRemoteOffer;

                let answer = self.build_local_sdp();
                self.local_description = Some(answer.clone());
                self.set_ice_role_from_signaling(false, remote_is_ice_lite);

                self.signaling = SignalingState::Stable;
                Ok(OutboundSdp::Answer(answer))
            }
            SignalingState::HaveLocalOffer => {
                if is_probably_offer(&sdp) {
                    self.rollback_to_stable();
                    return self.apply_remote_sdp(remote);
                }
                let (_remote_is_ice_lite, _ufrag, _pwd) =
                    self.extract_and_store_remote_ice_meta(&sdp)?;
                self.extract_and_store_rtp_meta(&sdp)?;
                self.remote_description = Some(sdp);
                self.signaling = SignalingState::Stable;
                Ok(OutboundSdp::None)
            }
            SignalingState::HaveRemoteOffer => Err(ConnectionError::Negotiation(
                "unexpected SDP while have-remote-offer".into(),
            )),
            SignalingState::Closed => Err(ConnectionError::Negotiation("connection closed".into())),
        };

        if out.is_ok()
            && let Err(e) = self.maybe_start_ice()
        {
            sink_error!(&self.logger_handle, "ICE start failed: {e}");
        }
        out
    }

    /// Sets the local RTP codecs to advertise in SDP.
    pub fn set_local_rtp_codecs(&mut self, codecs: Vec<CodecDescriptor>) {
        self.local_codecs = codecs;
    }

    /// Extracts RTP payload types and parameters from a remote SDP and stores them internally.
    ///
    /// # Errors
    ///
    /// - Returns `ConnectionError::RtpMap` if the rtpmap attribute cannot be parsed.
    pub fn extract_and_store_rtp_meta(&mut self, remote_sdp: &Sdp) -> Result<(), ConnectionError> {
        let mut discovered: Vec<RtpCodec> = Vec::new();

        for m in remote_sdp.media() {
            if !m.proto().to_uppercase().contains("RTP") {
                continue;
            }

            let allowed_pts: HashSet<u8> = m
                .fmts()
                .iter()
                .filter_map(|fmt| fmt.parse::<u8>().ok())
                .collect();

            for a in m.attrs() {
                if a.key() != "rtpmap" {
                    continue;
                }
                let raw = a
                    .value()
                    .ok_or_else(|| ConnectionError::RtpMap("Wrong value".into()))?;
                let rm: RtpMap = raw
                    .parse()
                    .map_err(|_| ConnectionError::RtpMap("Failed parsing rtpmap".into()))?;
                if !allowed_pts.is_empty() && !allowed_pts.contains(&rm.payload_type) {
                    continue;
                }

                discovered.push(RtpCodec::with_name(
                    rm.payload_type,
                    rm.clock_rate,
                    rm.encoding_name.clone(),
                ));
            }
        }

        discovered.sort_by_key(|c| c.payload_type);
        discovered.dedup_by_key(|c| c.payload_type);

        self.remote_codecs = discovered;
        Ok(())
    }

    /// Apply a remote ICE trickle candidate (received during ICE gathering).
    ///
    /// # Errors
    ///
    /// - Returns `ConnectionError::IceAgent` if parsing the candidate fails.
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

    /// Replace the current ICE agent with a new one.
    pub fn set_ice_agent(&mut self, ice_agent: IceAgent) {
        self.ice_agent = ice_agent;
    }

    // ----------------- Internal helpers -----------------

    /// Constructs a local SDP description (offer or answer) based on current local codecs and ICE info.
    fn build_local_sdp(&mut self) -> Sdp {
        let media: Vec<SDPMedia> = vec![self.build_media_description()];
        Sdp::new(
            0,
            SDPOrigin::new_blank(),
            "demo_session".to_owned(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            None,
            Vec::new(),
            vec![SDPTimeDesc::new_blank()],
            Vec::new(),
            media,
            Vec::new(),
        )
    }

    /// Drops local offer and resets signaling state to `Stable`.
    fn rollback_to_stable(&mut self) {
        self.local_description = None;
        self.signaling = SignalingState::Stable;
    }

    /// Extracts ICE credentials and candidates from a remote SDP.
    ///
    /// Returns `(remote_is_ice_lite, ufrag, pwd)`.
    fn extract_and_store_remote_ice_meta(
        &mut self,
        remote: &Sdp,
    ) -> Result<(bool, Option<String>, Option<String>), ConnectionError> {
        let mut remote_is_ice_lite = false;
        let mut ufrag: Option<String> = None;
        let mut pwd: Option<String> = None;

        for a in remote.attrs() {
            match a.key() {
                "ice-lite" => remote_is_ice_lite = true,
                "ice-ufrag" => {
                    ufrag = a.value().map(ToOwned::to_owned);
                }
                "ice-pwd" => {
                    pwd = a.value().map(ToOwned::to_owned);
                }
                _ => {}
            }
        }

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
                            ufrag = a.value().map(ToOwned::to_owned);
                        }
                    }
                    "ice-pwd" => {
                        if pwd.is_none() {
                            pwd = a.value().map(ToOwned::to_owned);
                        }
                    }
                    "ice-lite" => remote_is_ice_lite = true,
                    _ => {}
                }
            }
        }

        if let Some(u) = &ufrag {
            self.ice_agent.set_remote_ufrag(u.clone());
        }
        if let Some(p) = &pwd {
            self.ice_agent.set_remote_pwd(p.clone());
        }

        Ok((remote_is_ice_lite, ufrag, pwd))
    }

    /// Starts ICE candidate connectivity checks if both local and remote SDPs are present.
    fn maybe_start_ice(&mut self) -> Result<(), ConnectionError> {
        let ready = self.local_description.is_some()
            && self.remote_description.is_some()
            && matches!(self.signaling, SignalingState::Stable);
        if !ready {
            return Ok(());
        }
        self.start_connectivity_checks()
    }

    /// Performs ICE candidate gathering, pair formation, and connectivity checks.
    ///
    /// Spawns a background worker for asynchronous packet handling.
    ///
    /// # Errors
    ///
    /// Returns a `ConnectionError::Negotiation` if either local or remote SDP is not set,
    /// meaning ICE cannot start without a complete SDP exchange.
    ///
    /// Returns a `ConnectionError::IceAgent` if candidate gathering fails inside the ICE agent.
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
        self.ice_agent.start_checks();
        self.spawn_ice_worker();
        Ok(())
    }

    /// Sets ICE role based on whether we are offerer and whether remote is ICE-Lite.
    const fn set_ice_role_from_signaling(
        &mut self,
        we_are_offerer: bool,
        remote_is_ice_lite: bool,
    ) {
        self.ice_agent.role = if remote_is_ice_lite || we_are_offerer {
            IceRole::Controlling
        } else {
            IceRole::Controlled
        };
    }

    /// Runs ICE connectivity reactor in a blocking loop until a pair is nominated or timeout.
    pub fn run_ice_reactor_blocking(&mut self, total_ms: u64) {
        let deadline = Instant::now() + Duration::from_millis(total_ms);
        let sockets: Vec<Arc<UdpSocket>> = self
            .ice_agent
            .local_candidates
            .iter()
            .filter_map(|c| c.socket.clone())
            .collect();

        for sock in &sockets {
            let _ = sock.set_read_timeout(Some(Duration::from_millis(40)));
        }
        let mut buf = [0u8; 1500];

        while Instant::now() < deadline && self.ice_agent.nominated_pair.is_none() {
            for sock in &sockets {
                match sock.recv_from(&mut buf) {
                    Ok((n, from)) => {
                        self.ice_agent.handle_incoming_packet(&buf[..n], from);
                    }
                    Err(ref e)
                        if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                    }
                    Err(e) => eprintln!("ICE reactor recv error: {e}"),
                }
            }
        }
    }

    /// Spawns an ICE worker for asynchronous packet handling.
    fn spawn_ice_worker(&mut self) {
        if self.ice_worker.is_some() {
            return;
        }
        self.ice_worker = Some(IceWorker::spawn(&self.ice_agent));
    }

    /// Stops the ICE worker and clears it.
    fn stop_ice_worker(&mut self) {
        if let Some(w) = &mut self.ice_worker {
            w.stop();
        }
        self.ice_worker = None;
    }

    /// Polls ICE events from the worker and updates state.
    pub fn drain_ice_events(&mut self) {
        if let Some(w) = &self.ice_worker {
            while let Some((pkt, from)) = w.try_recv() {
                self.ice_agent.handle_incoming_packet(&pkt, from);
            }
        }
        if self.ice_agent.nominated_pair.is_some() && !matches!(self.ice_phase, IcePhase::Nominated)
        {
            self.ice_phase = IcePhase::Nominated;
            self.stop_ice_worker();
        }
    }

    #[must_use]
    /// Returns the currently discovered remote RTP codecs.
    pub const fn remote_codecs(&self) -> &Vec<RtpCodec> {
        &self.remote_codecs
    }

    /// Builds a media description SDP with ICE candidates, codecs, and connection info.
    fn build_media_description(&mut self) -> SDPMedia {
        let mut media_desc = SDPMedia::new_blank();
        media_desc.set_kind(DEFAULT_MEDIA_KIND);
        media_desc.set_port(SDPPortSpec::new(DEFAULT_PORT, None));
        media_desc.set_proto(DEFAULT_PROTO);

        let formats = if self.local_codecs.is_empty() {
            vec![DEFAULT_FMT.to_owned()]
        } else {
            self.local_codecs
                .iter()
                .map(|c| c.rtp.payload_type.to_string())
                .collect()
        };
        media_desc.set_fmts(formats);
        media_desc.set_connection(Some(SDPConnection::new(
            DEFAULT_NET_TYPE,
            DEFAULT_ADDR_TYPE,
            DEFAULT_CONN_ADDR,
        )));

        let mut attrs = get_local_candidates_as_attributes(self);
        let (ufrag, pwd) = self.ice_agent.local_credentials();
        attrs.push(SDPAttribute::new("ice-ufrag", ufrag));
        attrs.push(SDPAttribute::new("ice-pwd", pwd));

        if self.local_codecs.is_empty() {
            attrs.push(SDPAttribute::new(
                "rtpmap",
                Some("96 H264/90000".to_owned()),
            ));
        } else {
            for descriptor in &self.local_codecs {
                let codec = &descriptor.rtp;
                let name = if codec.name.is_empty() {
                    "H264"
                } else {
                    &codec.name
                };
                let value = format!("{} {}/{}", codec.payload_type, name, codec.clock_rate);
                attrs.push(SDPAttribute::new("rtpmap", Some(value)));
                if let Some(fmtp) = &descriptor.fmtp {
                    attrs.push(SDPAttribute::new(
                        "fmtp",
                        Some(format!("{} {fmtp}", codec.payload_type)),
                    ));
                }
            }
        }

        attrs.push(SDPAttribute::new("rtcp-mux", None));
        media_desc.set_attrs(attrs);
        media_desc
    }
}

/// Determines if an SDP is probably an offer (heuristic for glare resolution).
const fn is_probably_offer(_sdp: &Sdp) -> bool {
    false
}

/// Collects local host ICE candidates and converts them into SDP attributes.
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

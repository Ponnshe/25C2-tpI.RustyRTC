use super::{connection_error::ConnectionError, ice_and_sdp::ICEAndSDP};
use crate::ice::gathering_service;
use crate::ice::type_ice::ice_agent::{IceAgent, IceRole};
use crate::sdp::addr_type::AddrType as SDPAddrType;
use crate::sdp::attribute::Attribute as SDPAttribute;
use crate::sdp::connection::Connection as SDPConnection;
use crate::sdp::media::Media as SDPMedia;
use crate::sdp::media::MediaKind as SDPMediaKind;
use crate::sdp::origin::Origin as SDPOrigin;
use crate::sdp::port_spec::PortSpec as SDPPortSpec;
use crate::sdp::sdpc::Sdp;
use crate::sdp::time_desc::TimeDesc as SDPTimeDesc;

const DEFAULT_PORT: u16 = 9;
const DEAFULT_PROTO: &str = "UDP/TLS/RTP/SAVPF";
const DEFAULT_FMT: &str = "99";
const DEFAULT_NET_TYPE: &str = "IN";
const DEFAULT_ADDR_TYPE: SDPAddrType = SDPAddrType::IP4;
const DEFAULT_CONN_ADDR: &str = "0.0.0.0";
const DEFAULT_MEDIA_KIND: SDPMediaKind = SDPMediaKind::Video;

// ----------------- New: signaling model -----------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalingState {
    Stable,
    HaveLocalOffer,
    HaveRemoteOffer,
    Closed,
}

#[derive(Debug)]
pub enum OutboundSdp {
    Offer(Sdp),
    Answer(Sdp),
    None,
}

// ----------------- ConnectionManager --------------------

pub struct ConnectionManager {
    pub ice_agent: IceAgent,
    signaling: SignalingState,
    local_description: Option<Sdp>,
    remote_description: Option<Sdp>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        let ice_agent = IceAgent::new(IceRole::Controlling);
        Self {
            ice_agent,
            signaling: SignalingState::Stable,
            local_description: None,
            remote_description: None,
        }
    }

    /// UI calls this to start (re)negotiation. Returns an SDP **Offer** to send.
    pub fn negotiate(&mut self) -> Result<OutboundSdp, ConnectionError> {
        match self.signaling {
            SignalingState::Stable => {
                let offer = self.build_local_sdp()?; // same builder for offer/answer
                self.local_description = Some(offer.clone());
                self.signaling = SignalingState::HaveLocalOffer;
                Ok(OutboundSdp::Offer(offer))
            }
            SignalingState::HaveLocalOffer => {
                // Already negotiating; nothing to do.
                Ok(OutboundSdp::None)
            }
            SignalingState::HaveRemoteOffer => {
                // We owe an answer; refuse to start a new offer.
                Err(ConnectionError::Negotiation(
                    "cannot create offer while have-remote-offer",
                ))
            }
            SignalingState::Closed => Err(ConnectionError::Negotiation("connection closed")),
        }
    }

    /// UI passes *any* remote SDP here (offer or answer). We decide what it is by state.
    /// - If Stable: treat it as **Offer**, store it, generate **Answer** and return it.
    /// - If HaveLocalOffer: treat it as **Answer**, store it, return None.
    /// - If HaveRemoteOffer: receiving another SDP is unexpected (unless you add rollbacks/pranswers).
    pub fn apply_remote_sdp(&mut self, remote: &str) -> Result<OutboundSdp, ConnectionError> {
        let sdp = Sdp::parse(remote).map_err(ConnectionError::Sdp)?;

        match self.signaling {
            SignalingState::Stable => {
                // Treat as remote offer.
                self.extract_and_store_remote_candidates(&sdp)?;
                self.remote_description = Some(sdp);
                self.signaling = SignalingState::HaveRemoteOffer;

                // Build and send local answer.
                let answer = self.build_local_sdp()?;
                self.local_description = Some(answer.clone());
                // After setting local answer + remote offer we are back to Stable
                self.signaling = SignalingState::Stable;
                Ok(OutboundSdp::Answer(answer))
            }

            SignalingState::HaveLocalOffer => {
                // Expecting an answer. If we got a new offer, that's glare → rollback.
                if is_probably_offer(&sdp) {
                    // --- Glare handling: rollback our local offer and accept their offer. ---
                    self.rollback_to_stable();
                    // Re-run as if we were Stable
                    return self.apply_remote_sdp(remote);
                }

                // Treat as answer
                self.extract_and_store_remote_candidates(&sdp)?;
                self.remote_description = Some(sdp);
                self.signaling = SignalingState::Stable;
                Ok(OutboundSdp::None)
            }

            SignalingState::HaveRemoteOffer => {
                // We have not sent our answer yet (in this design we answer immediately),
                // so getting an additional SDP here is an app-level error.
                Err(ConnectionError::Negotiation(
                    "unexpected SDP while have-remote-offer (answer was not sent yet)",
                ))
            }

            SignalingState::Closed => Err(ConnectionError::Negotiation("connection closed")),
        }
    }

    /// Optional: separate method if you ever need to apply only candidates from a remote trickle.
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

    fn extract_and_store_remote_candidates(&mut self, remote: &Sdp) -> Result<(), ConnectionError> {
        for m in remote.media() {
            for a in m.attrs() {
                if a.key() == "candidate" {
                    let value = a.value().ok_or(ConnectionError::IceAgent)?;
                    let ice_and_sdp: ICEAndSDP =
                        value.parse().map_err(|_| ConnectionError::IceAgent)?;
                    self.ice_agent.add_remote_candidate(ice_and_sdp.candidate());
                }
            }
        }
        Ok(())
    }
}

// Heuristic to decide if an SDP looks like an "offer" when we need to disambiguate during glare.
// In strict O/A, "offer vs answer" is *context*, not content; this is best-effort only.
fn is_probably_offer(_sdp: &Sdp) -> bool {
    // Keep simple for now; you can refine (e.g., look at a=setup:actpass vs passive/active, or dtls role).
    false
}

// ----------------- Your existing helpers (unchanged) -----------------

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

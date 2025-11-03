use crate::sdp::{addr_type::AddrType as SDPAddrType, media::MediaKind as SDPMediaKind};

pub(super) const DEFAULT_PORT: u16 = 9;
pub(super) const DEFAULT_PROTO: &str = "UDP/TLS/RTP/SAVPF";
pub(super) const DEFAULT_FMT: &str = "96";
pub(super) const DEFAULT_NET_TYPE: &str = "IN";
pub(super) const DEFAULT_ADDR_TYPE: SDPAddrType = SDPAddrType::IP4;
pub(super) const DEFAULT_CONN_ADDR: &str = "0.0.0.0";
pub(super) const DEFAULT_MEDIA_KIND: SDPMediaKind = SDPMediaKind::Video;

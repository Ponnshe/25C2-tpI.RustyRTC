/// RFC3550 generic header extension (profile-specific).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpHeaderExtension {
    /// 16-bit profile-specific identifier.
    pub profile: u16,
    /// Raw extension payload (not including the 4-byte header).
    pub data: Vec<u8>,
}

impl RtpHeaderExtension {
    pub fn new(profile: u16, data: Vec<u8>) -> Self {
        Self { profile, data }
    }
}

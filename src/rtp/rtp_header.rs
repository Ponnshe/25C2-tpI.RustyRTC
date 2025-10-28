/// RTP fixed header plus CSRC list and optional extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpHeader {
    pub version: u8,      // must be 2
    pub padding: bool,    // P bit
    pub extension: bool,  // X bit
    pub marker: bool,     // M bit
    pub payload_type: u8, // 7 bits
    pub sequence_number: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub csrcs: Vec<u32>,
    pub header_extension: Option<RtpHeaderExtension>,
}

impl RtpHeader {
    pub fn new(payload_type: u8, sequence_number: u16, timestamp: u32, ssrc: u32) -> Self {
        Self {
            version: RTP_VERSION,
            padding: false,
            extension: false,
            marker: false,
            payload_type,
            sequence_number,
            timestamp,
            ssrc,
            csrcs: Vec::new(),
            header_extension: None,
        }
    }

    pub fn with_marker(mut self, marker: bool) -> Self {
        self.marker = marker;
        self
    }
    pub fn with_csrcs(mut self, csrcs: Vec<u32>) -> Self {
        self.csrcs = csrcs;
        self
    }
    pub fn with_extension(mut self, ext: Option<RtpHeaderExtension>) -> Self {
        self.extension = ext.is_some();
        self.header_extension = ext;
        self
    }
}

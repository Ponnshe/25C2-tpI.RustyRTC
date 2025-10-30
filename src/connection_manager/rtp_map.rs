use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpMap {
    pub payload_type: u8,
    pub encoding_name: String, // leave as-is; case-insensitive in SDP
    pub clock_rate: u32,
    pub encoding_params: Option<u16>, // usually channels for audio
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RtpMapParseError {
    MissingParts,
    InvalidPayloadType,
    InvalidClockRate,
    PayloadTypeOutOfRange,
    TrailingGarbage,
}

impl std::fmt::Display for RtpMapParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use RtpMapParseError::*;
        match self {
            MissingParts => write!(f, "Missing required parts in rtpmap"),
            InvalidPayloadType => write!(f, "Invalid payload type"),
            InvalidClockRate => write!(f, "Invalid clock rate"),
            PayloadTypeOutOfRange => write!(f, "Payload type out of [0,127]"),
            TrailingGarbage => write!(f, "Unexpected trailing tokens after rtpmap"),
        }
    }
}
impl std::error::Error for RtpMapParseError {}

impl FromStr for RtpMap {
    type Err = RtpMapParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use RtpMapParseError::*;

        // Accept strings like: "96 opus/48000/2" or "0 PCMU/8000"
        // We expect: <pt> <encoding>/<clock>[/<params>]
        let s = s.trim();

        // Split by whitespace to get "<pt>" and "<rhs>"
        let mut it = s.split_whitespace();
        let pt_str = it.next().ok_or(MissingParts)?;
        let rhs = it.next().ok_or(MissingParts)?;

        // Extra tokens (beyond "<pt> <rhs>") are suspicious; fail explicitly
        if it.next().is_some() {
            return Err(TrailingGarbage);
        }

        // Parse PT
        let payload_type: u8 = pt_str.parse().map_err(|_| InvalidPayloadType)?;
        if payload_type > 127 {
            return Err(PayloadTypeOutOfRange);
        }

        // Split rhs by '/'
        let mut parts = rhs.splitn(3, '/');

        let encoding_name = parts.next().ok_or(MissingParts)?.trim().to_string();

        let clock_rate: u32 = parts
            .next()
            .ok_or(MissingParts)?
            .trim()
            .parse()
            .map_err(|_| InvalidClockRate)?;

        // Optional third part
        let encoding_params = match parts.next() {
            None => None,
            Some(p) => {
                let p = p.trim();
                if p.is_empty() {
                    None
                } else {
                    // channels are positive; we treat "0" as invalid and drop to None
                    let v: u16 = p.parse().map_err(|_| MissingParts)?;
                    if v == 0 { None } else { Some(v) }
                }
            }
        };

        Ok(RtpMap {
            payload_type,
            encoding_name,
            clock_rate,
            encoding_params,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_opus() {
        let rm: RtpMap = "96 opus/48000/2".parse().unwrap();
        assert_eq!(rm.payload_type, 96);
        assert_eq!(rm.encoding_name.to_lowercase(), "opus");
        assert_eq!(rm.clock_rate, 48000);
        assert_eq!(rm.encoding_params, Some(2));
    }

    #[test]
    fn parses_pcmu_no_params() {
        let rm: RtpMap = "0 PCMU/8000".parse().unwrap();
        assert_eq!(rm.payload_type, 0);
        assert_eq!(rm.encoding_name, "PCMU");
        assert_eq!(rm.clock_rate, 8000);
        assert_eq!(rm.encoding_params, None);
    }

    #[test]
    fn parses_vp8_video() {
        let rm: RtpMap = "96 VP8/90000".parse().unwrap();
        assert_eq!(rm.payload_type, 96);
        assert_eq!(rm.encoding_name, "VP8");
        assert_eq!(rm.clock_rate, 90_000);
        assert_eq!(rm.encoding_params, None);
    }

    #[test]
    fn multiple_spaces_and_tabs() {
        let rm: RtpMap = "  101\ttelephone-event/8000  ".parse().unwrap();
        assert_eq!(rm.payload_type, 101);
        assert_eq!(rm.encoding_name, "telephone-event");
        assert_eq!(rm.clock_rate, 8000);
        assert_eq!(rm.encoding_params, None);
    }

    #[test]
    fn channels_over_255_ok() {
        let rm: RtpMap = "97 L16/44100/10".parse().unwrap();
        assert_eq!(rm.payload_type, 97);
        assert_eq!(rm.clock_rate, 44100);
        assert_eq!(rm.encoding_params, Some(10));
    }

    #[test]
    fn invalid_missing_parts() {
        assert!("".parse::<RtpMap>().is_err());
        assert!("96".parse::<RtpMap>().is_err());
        assert!("opus/48000".parse::<RtpMap>().is_err()); // no PT
    }

    #[test]
    fn invalid_pt_and_rate() {
        assert!("x9 opus/48000/2".parse::<RtpMap>().is_err());
        assert!("96 opus/xx".parse::<RtpMap>().is_err());
    }

    #[test]
    fn pt_out_of_range() {
        assert!("200 opus/48000".parse::<RtpMap>().is_err());
        // 127 is OK
        assert!("127 opus/48000".parse::<RtpMap>().is_ok());
    }

    #[test]
    fn trailing_garbage_fails() {
        assert!("96 opus/48000/2 extra".parse::<RtpMap>().is_err());
    }

    #[test]
    fn zero_channels_becomes_none() {
        let rm: RtpMap = "98 opus/48000/0".parse().unwrap();
        assert_eq!(rm.encoding_params, None);
    }
}

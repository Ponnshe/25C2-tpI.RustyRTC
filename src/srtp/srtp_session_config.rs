use crate::srtp::{SrtpEndpointKeys, SrtpProfile};

#[derive(Debug, Clone)]
pub struct SrtpSessionConfig {
    pub profile: SrtpProfile,
    pub outbound: SrtpEndpointKeys,
    pub inbound: SrtpEndpointKeys,
}

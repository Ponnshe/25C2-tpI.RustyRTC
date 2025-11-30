#[derive(Debug, Clone)]
pub struct SrtpEndpointKeys {
    pub master_key: Vec<u8>,
    pub master_salt: Vec<u8>,
}

/// Represents the DTLS role in a handshake.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DtlsRole {
    /// The DTLS client role.
    Client,
    /// The DTLS server role.
    Server,
}

#[derive(Debug, Clone)]
pub struct SctpFileProperties {
    pub file_name: String,
    pub file_size: u64,
    pub transaction_id: u32,
}

#[derive(Debug, Clone)]
pub enum SctpEvents {
    SendAccept { id: u32 },
    SendCancel { id: u32 },
    SendChunk { file_id: u32, payload: Vec<u8> },
    SendOffer { file_properties: SctpFileProperties },
    SendReject { id: u32 },
    IncomingSctpPacket { sctp_packet: Vec<u8> },
    ReceivedOffer { file_properties: SctpFileProperties },
    ReceivedAccept { id: u32 },
    ReceivedReject { id: u32 },
    ReceivedCancel { id: u32 },
    ReceivedChunk { id: u32, seq: u32, payload: Vec<u8> },
    SctpErr(String),
    TransmitSctpPacket { payload: Vec<u8> },
}

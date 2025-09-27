enum AddressType {
    IP4,
    IP6,
}
struct Origin {
    username: String,
    session_id: i64,
    session_version: i64,
    net_type: String,
    address_type: AddressType,
    unicast_address: String,
}
struct MediaDescription {
    media: String,
    port: i64,
    protocol: String,
    format: String,
}
enum Attribute {
    Name(String),
    NameValue(String, String),
}
struct SdpC {
    version: i64,
    origin: Origin,
    session_name: String,
    time_active: (i64, i64),
    media_description: MediaDescription,
    attributes: Vec<Attribute>,
}

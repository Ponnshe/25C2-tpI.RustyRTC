use crate::ice::type_ice::candidate::Candidate;
use crate::ice::type_ice::candidate_type::CandidateType;
use std::fmt;

use std::net::{IpAddr, SocketAddr};
use std::num::ParseIntError;
use std::str::FromStr;

pub struct ICEAndSDP {
    candidate: Candidate,
}

impl ICEAndSDP {
    pub fn new(candidate: Candidate) -> Self {
        Self { candidate }
    }

    pub fn set_candidate(&mut self, candidate: Candidate) {
        self.candidate = candidate;
    }

    fn get_typ_as_sdp_string(&self) -> String {
        match self.candidate.cand_type {
            CandidateType::Host => "host".to_owned(),
            CandidateType::PeerReflexive => "prflx".to_owned(),
            CandidateType::Relayed => "relay".to_owned(),
            CandidateType::ServerReflexive => "srflx".to_owned(),
        }
    }

    pub fn candidate(self) -> Candidate {
        self.candidate
    }

    fn get_related_addr_as_sdp_string(&self) -> Option<String> {
        if let Some(s) = self.candidate.related_address {
            return Some(format!("raddr {} rport {}", s.ip(), s.port()));
        }
        None
    }
}

impl fmt::Display for ICEAndSDP {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let typ = self.get_typ_as_sdp_string(); // e.g. "host", "srflx"
        let rel = self.get_related_addr_as_sdp_string(); // e.g. Some("raddr 1.2.3.4 rport 5678")

        write!(
            f,
            "{} {} {} {} {} {} typ {}",
            self.candidate.foundation,
            self.candidate.component,
            self.candidate.transport,
            self.candidate.priority,
            self.candidate.address.ip(),
            self.candidate.address.port(),
            typ,
        )?;

        if let Some(s) = rel {
            write!(f, " {}", s)?;
        }

        Ok(())
    }
}

impl FromStr for ICEAndSDP {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let s = s.strip_prefix("candidate:").unwrap_or(s);

        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() < 8 {
            return Err(format!("Invalid candidate string: '{s}'"));
        }

        let foundation = parts[0].to_string();
        let component: u8 = parts[1].parse().map_err(|_| "Invalid component")?;
        let transport = parts[2].to_string();

        let priority: u32 = parts[3].parse::<u32>().map_err(|_| "Invalid priority")?;

        let ip: IpAddr = parts[4].parse().map_err(|_| "Invalid IP address")?;
        let port: u16 = parts[5].parse::<u16>().map_err(|_| "Invalid port")?;

        // Verify the "typ" token is where we expect
        if parts.get(6) != Some(&"typ") {
            return Err("Missing 'typ' token in candidate".into());
        }
        let cand_type = match parts.get(7).copied().ok_or("Missing candidate type")? {
            "host" => CandidateType::Host,
            "srflx" => CandidateType::ServerReflexive,
            "prflx" => CandidateType::PeerReflexive,
            "relay" => CandidateType::Relayed,
            other => return Err(format!("Unknown candidate type: {other}")),
        };

        let mut related_address = None;
        let mut i = 8;
        while i + 1 < parts.len() {
            match parts[i] {
                "raddr" if i + 1 < parts.len() => {
                    let rel_ip: IpAddr = parts[i + 1].parse().map_err(|_| "Invalid raddr IP")?;
                    // we'll fill port once/if we see rport
                    related_address = Some(SocketAddr::new(rel_ip, 0));
                    i += 2;
                }
                "rport" if i + 1 < parts.len() => {
                    let rel_port: u16 = parts[i + 1].parse().map_err(|_| "Invalid rport value")?;
                    if let Some(sa) = related_address {
                        related_address = Some(SocketAddr::new(sa.ip(), rel_port));
                    } else {
                        // rport before raddr: create with 0.0.0.0/ip unspecified if you want,
                        // or just ignore until raddr arrives; simplest is to require raddr first.
                    }
                    i += 2;
                }
                _ => i += 1,
            }
        }

        let candidate = Candidate {
            foundation,
            component,
            transport,
            priority,
            address: SocketAddr::new(ip, port),
            cand_type,
            related_address,
            socket: None,
        };

        Ok(Self { candidate })
    }
}

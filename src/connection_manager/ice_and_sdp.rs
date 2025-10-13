use crate::ice::type_ice::candidate::Candidate;
use crate::ice::type_ice::candidate_type::CandidateType;
use std::fmt;

use std::str::FromStr;
use std::net::{IpAddr, SocketAddr};
use std::num::ParseIntError;

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
        let parts: Vec<&str> = s.split_whitespace().collect();

        if parts.len() < 8 {
            return Err(format!("Invalid candidate string: '{}'", s));
        }

        // Parse sequentially, assuming correct order
        let foundation = parts[0].to_string();
        let component: u8 = parts[1].parse::<u8>().map_err(|_| "Invalid component")?;
        let transport = parts[2].to_string();
        let priority: u32 = parts[3].parse::<u32>().map_err(|_| "Invalid priority")?;
        let ip: IpAddr = parts[4].parse().map_err(|_| "Invalid IP address")?;
        let port: u16 = parts[5].parse::<u16>().map_err(|_| "Invalid port")?;
        let cand_type = match parts[7] {
            "host" => CandidateType::Host,
            "srflx" => CandidateType::ServerReflexive,
            "prflx" => CandidateType::PeerReflexive,
            "relay" => CandidateType::Relayed,
            _ => return Err(format!("Unknown candidate type: {}", parts[7])),
        };

        // Optional related address parsing
        let mut related_address = None;
        if parts.len() > 8 {
            // expecting: "raddr <ip> rport <port>"
            if parts.get(8) == Some(&"raddr") && parts.get(10) == Some(&"rport") {
                let rel_ip: IpAddr = parts[9].parse().map_err(|_| "Invalid raddr IP")?;
                let rel_port: u16 = parts[11].parse().map_err(|_| "Invalid rport value")?;
                related_address = Some(SocketAddr::new(rel_ip, rel_port));
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

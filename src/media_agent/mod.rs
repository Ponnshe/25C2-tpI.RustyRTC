use std::time::{Duration, Instant};

use std::sync::mpsc::Sender;

use crate::core::{events::EngineEvent, session::Session};
use crate::rtp_session::{outbound_track_handle::OutboundTrackHandle, rtp_codec::RtpCodec};

pub struct MockMediaAgent {
    tx_evt: Sender<EngineEvent>,
    outbound_tracks: Vec<OutboundTrackHandle>,
    last_frame_sent: Option<Instant>,
    frame_interval: Duration,
}

impl MockMediaAgent {
    pub fn new(tx_evt: Sender<EngineEvent>) -> Self {
        Self {
            tx_evt,
            outbound_tracks: Vec::new(),
            last_frame_sent: None,
            frame_interval: Duration::from_millis(1500),
        }
    }

    pub fn on_engine_event(&mut self, evt: &EngineEvent, session: Option<&Session>) {
        match evt {
            EngineEvent::Established => {
                if let Some(sess) = session {
                    self.ensure_track(sess);
                }
            }
            EngineEvent::Closed | EngineEvent::Closing { .. } => {
                self.outbound_tracks.clear();
                self.last_frame_sent = None;
            }
            _ => {}
        }
    }

    pub fn tick(&mut self, session: Option<&Session>) {
        let Some(sess) = session else { return };
        if self.outbound_tracks.is_empty() {
            return;
        }

        let now = Instant::now();
        let should_send = self
            .last_frame_sent
            .map(|last| now.duration_since(last) >= self.frame_interval)
            .unwrap_or(true);
        if !should_send {
            return;
        }

        for track in &self.outbound_tracks {
            // Dummy payload for testing.
            let payload = vec![track.payload_type(); 1200];
            match sess.send_media_frame(track, &payload) {
                Ok(()) => {
                    let _ = self.tx_evt.send(EngineEvent::Log(format!(
                        "[MockMediaAgent] sent {} bytes on ssrc={:#010x} (PT={})",
                        payload.len(),
                        track.local_ssrc,
                        track.payload_type()
                    )));
                }
                Err(e) => {
                    let _ = self.tx_evt.send(EngineEvent::Error(format!(
                        "[MockMediaAgent] send failed for ssrc={:#010x}: {e}",
                        track.local_ssrc
                    )));
                }
            }
        }

        self.last_frame_sent = Some(now);
    }

    fn ensure_track(&mut self, session: &Session) {
        if !self.outbound_tracks.is_empty() {
            return;
        }
        let codec = RtpCodec::new(96, 90_000);
        match session.register_outbound_track(codec) {
            Ok(handle) => {
                let _ = self.tx_evt.send(EngineEvent::Log(format!(
                    "[MockMediaAgent] registered outbound track ssrc={:#010x} PT={}",
                    handle.local_ssrc,
                    handle.payload_type()
                )));
                self.outbound_tracks.push(handle);
            }
            Err(e) => {
                let _ = self.tx_evt.send(EngineEvent::Error(format!(
                    "[MockMediaAgent] failed to register outbound track: {e}"
                )));
            }
        }
    }
}

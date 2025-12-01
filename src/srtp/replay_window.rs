use crate::srtp::constants::REPLAY_WINDOW_SIZE;

#[derive(Default)]
pub(crate) struct ReplayWindow {
    max_index: u64,
    window: u64,
}

impl ReplayWindow {
    #![allow(dead_code)]
    pub(crate) const fn new() -> Self {
        Self {
            max_index: 0,
            window: 0,
        }
    }

    pub(crate) const fn is_replay(&self, index: u64) -> bool {
        if index > self.max_index {
            return false;
        }
        let diff = self.max_index.saturating_sub(index);
        if diff >= REPLAY_WINDOW_SIZE {
            return true;
        }
        (self.window & (1u64 << diff)) != 0
    }

    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn record(&mut self, index: u64) {
        if index > self.max_index {
            let diff = index.saturating_sub(self.max_index);
            if diff < REPLAY_WINDOW_SIZE {
                self.window <<= diff;
            } else {
                self.window = 0;
            }
            self.window |= 1;
            self.max_index = index;
        } else {
            let diff = self.max_index.saturating_sub(index);
            if diff < REPLAY_WINDOW_SIZE {
                self.window |= 1u64 << diff;
            }
        }
    }
}

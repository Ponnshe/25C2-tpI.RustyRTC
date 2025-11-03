#[derive(Debug, Default, Clone)]
pub struct SeqExt {
    cycles: u32, // multiples of 2^16
    last: u16,   // last sequence number we saw
}

impl SeqExt {
    pub fn update(&mut self, seq: u16) -> u32 {
        // If we went "backwards" by more than half the space, it's a wrap
        if seq < self.last && self.last.wrapping_sub(seq) > 0x8000 {
            self.cycles = self.cycles.wrapping_add(1 << 16);
        }
        self.last = seq;
        self.cycles | u32::from(seq) // same as cycles + seq because cycles % 2^16 == 0
    }
}

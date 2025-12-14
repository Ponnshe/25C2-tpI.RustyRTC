/// Audio Codec module for G.711 u-law encoding and decoding.
///
/// This module provides pure functions to convert between raw PCM audio samples (f32)
/// and compressed G.711 u-law bytes (u8).
///
/// The internal algorithm works with 16-bit signed integers (i16), but the public API
/// uses `f32` to maintain consistency with the rest of the media pipeline.

const BIAS: i16 = 0x84;
const CLIP: i32 = 32635;

/// Encodes a slice of f32 PCM samples to G.711 u-law bytes.
///
/// The input samples are expected to be in the range [-1.0, 1.0].
/// They are converted to 14-bit signed integers (via i16) before u-law compression.
pub fn encode(pcm_samples: &[f32]) -> Vec<u8> {
    pcm_samples.iter().map(|&sample| {
        // Convert f32 [-1.0, 1.0] to i16 [-32768, 32767]
        let s = (sample * 32767.0) as i32;
        let clamped = s.clamp(-32768, 32767) as i16;
        linear_to_ulaw(clamped)
    }).collect()
}

/// Decodes a slice of G.711 u-law bytes to f32 PCM samples.
///
/// The resulting samples will be in the range [-1.0, 1.0].
pub fn decode(ulaw_bytes: &[u8]) -> Vec<f32> {
    ulaw_bytes.iter().map(|&byte| {
        let sample = ulaw_to_linear(byte);
        // Convert i16 to f32 [-1.0, 1.0]
        sample as f32 / 32767.0
    }).collect()
}

/// Converts a 16-bit linear PCM sample to 8-bit u-law.
fn linear_to_ulaw(sample: i16) -> u8 {
    let sign = (sample >> 8) & 0x80;
    let mut s = sample as i32;
    if s < 0 {
        s = -s;
    }
    if s > CLIP {
        s = CLIP;
    }
    
    s += BIAS as i32;
    
    // Better reference implementation:
    let mut mask = 0x4000;
    let mut exp = 7;
    while (s & mask) == 0 && exp > 0 {
        mask >>= 1;
        exp -= 1;
    }
    
    let mantissa = (s >> (exp + 3)) & 0x0F;
    let ulaw_byte = (sign as u8) | ((exp as u8) << 4) | (mantissa as u8);
    
    !ulaw_byte // Invert bits for u-law
}

/// Converts an 8-bit u-law sample to 16-bit linear PCM.
fn ulaw_to_linear(ulaw_byte: u8) -> i16 {
    let ulaw_byte = !ulaw_byte; // Invert bits back
    let sign = ulaw_byte & 0x80;
    let exponent = (ulaw_byte >> 4) & 0x07;
    let mantissa = ulaw_byte & 0x0F;
    
    let mut sample = (((mantissa as i32) << 3) + 132) << exponent;
    sample -= BIAS as i32;
    
    if sign != 0 {
        sample = -sample;
    }
    
    sample as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_cycle() {
        let original = 0.5f32;
        let encoded = encode(&[original]);
        let decoded = decode(&encoded);
        
        // G.711 is lossy, so we check if the result is close enough.
        let diff = (original - decoded[0]).abs();
        assert!(diff < 0.05, "Decoded value {} too far from original {}", decoded[0], original);
    }

    #[test]
    fn test_silence() {
        let original = 0.0f32;
        let encoded = encode(&[original]);
        let decoded = decode(&encoded);
        
        let diff = (original - decoded[0]).abs();
        assert!(diff < 0.01, "Silence should be preserved reasonably well");
    }

    #[test]
    fn test_clipping() {
        let original = 1.5f32; // > 1.0, should clip
        let encoded = encode(&[original]);
        let decoded = decode(&encoded);
        
        // Should be close to max value (~1.0)
        assert!(decoded[0] > 0.9, "Should be close to max positive value");
    }
}

//! Fast pseudo-random overwrite data: xoshiro256++ seeded through SplitMix64.
//!
//! Overwrite data doesn't need to be cryptographic, it needs to be fast and unrelated to what was
//! on the disk. xoshiro256++ fills buffers at several GB/s per core, so random passes run at disk
//! speed. Every pass gets a fresh seed; the seed is kept so a verify pass can regenerate the
//! exact same stream.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone)]
pub struct Rng {
    s: [u64; 4],
}

fn splitmix64(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Rng {
    pub fn seeded(seed: u64) -> Self {
        let mut x = seed;
        Self { s: [splitmix64(&mut x), splitmix64(&mut x), splitmix64(&mut x), splitmix64(&mut x)] }
    }

    /// A seed that differs between calls, threads and runs.
    pub fn fresh_seed() -> u64 {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
        let local = 0u8;
        let addr = &local as *const u8 as u64;
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut x = nanos ^ addr.rotate_left(32) ^ n.wrapping_mul(0xA24B_AED4_963E_E407) ^ (std::process::id() as u64) << 17;
        splitmix64(&mut x)
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.s;
        let result = s[0].wrapping_add(s[3]).rotate_left(23).wrapping_add(s[0]);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    pub fn fill(&mut self, buf: &mut [u8]) {
        let (chunks, rest) = buf.as_chunks_mut::<8>();
        for c in chunks {
            *c = self.next_u64().to_le_bytes();
        }
        if !rest.is_empty() {
            let v = self.next_u64().to_le_bytes();
            rest.copy_from_slice(&v[..rest.len()]);
        }
    }

    /// Random file/folder name of `len` characters (letters and digits).
    pub fn name(&mut self, len: usize) -> String {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
        (0..len.max(1)).map(|_| ALPHABET[(self.next_u64() % ALPHABET.len() as u64) as usize] as char).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let (mut a, mut b) = (Rng::seeded(42), Rng::seeded(42));
        let (mut x, mut y) = (vec![0u8; 1003], vec![0u8; 1003]);
        a.fill(&mut x);
        b.fill(&mut y);
        assert_eq!(x, y);
        assert!(x.iter().any(|&v| v != 0));
        assert_ne!(Rng::fresh_seed(), Rng::fresh_seed());
    }

    #[test]
    fn names() {
        let n = Rng::seeded(1).name(12);
        assert_eq!(n.len(), 12);
        assert!(n.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}

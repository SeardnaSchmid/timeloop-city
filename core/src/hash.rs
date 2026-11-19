//! Plattformunabhängiger Zustands-Hash (FNV-1a, 64 Bit).
//!
//! Bewusst nicht `std::hash`: dessen Ausgabe ist zwischen Rust-Versionen
//! nicht garantiert. Zahlen werden immer als Little-Endian geschrieben.

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Debug)]
pub struct StateHasher(u64);

impl Default for StateHasher {
    fn default() -> Self {
        Self(OFFSET)
    }
}

impl StateHasher {
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(PRIME);
        }
    }

    pub fn write_u32(&mut self, v: u32) {
        self.write_bytes(&v.to_le_bytes());
    }

    pub fn write_u64(&mut self, v: u64) {
        self.write_bytes(&v.to_le_bytes());
    }

    pub fn write_i64(&mut self, v: i64) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Länge zuerst, damit `[1,2]+[3]` und `[1]+[2,3]` verschieden hashen.
    pub fn write_u32_slice(&mut self, v: &[u32]) {
        self.write_u64(v.len() as u64);
        v.iter().for_each(|&x| self.write_u32(x));
    }

    pub fn write_i64_slice(&mut self, v: &[i64]) {
        self.write_u64(v.len() as u64);
        v.iter().for_each(|&x| self.write_i64(x));
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

/// Alles, was zum Weltzustand gehört, schreibt sich hier vollständig hinein.
pub trait StateHash {
    fn hash_into(&self, h: &mut StateHasher);

    fn state_hash(&self) -> u64 {
        let mut h = StateHasher::default();
        self.hash_into(&mut h);
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_reference_vector() {
        let mut h = StateHasher::default();
        h.write_bytes(b"a");
        assert_eq!(h.finish(), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn slice_boundaries_matter() {
        let mut a = StateHasher::default();
        a.write_u32_slice(&[1, 2]);
        a.write_u32_slice(&[3]);
        let mut b = StateHasher::default();
        b.write_u32_slice(&[1]);
        b.write_u32_slice(&[2, 3]);
        assert_ne!(a.finish(), b.finish());
    }
}

//! Zufall ohne Zustand: jeder Wurf ist `Hash(Seed, Agent, Ereignis-Nr.)`.
//!
//! Damit hängt kein Ergebnis davon ab, in welcher Reihenfolge Agents
//! rechnen oder ob ein neues System zusätzliche Würfe macht.

/// SplitMix64-Finalizer: gute Streuung, auf jeder Plattform gleich.
#[inline]
const fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Ein 64-Bit-Wurf für `agent` beim Ereignis `event` in der Welt `seed`.
#[inline]
pub const fn roll(seed: u64, agent: u32, event: u64) -> u64 {
    let a = mix(seed ^ 0x9e37_79b9_7f4a_7c15);
    let b = mix(a ^ agent as u64);
    mix(b ^ event)
}

/// Gleichverteilte Zahl in `0..n` (ohne Modulo-Verzerrung, Lemire).
#[inline]
pub const fn below(seed: u64, agent: u32, event: u64, n: u32) -> u32 {
    let r = roll(seed, agent, event) >> 32;
    ((r * n as u64) >> 32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_input_same_output() {
        assert_eq!(roll(1, 2, 3), roll(1, 2, 3));
    }

    #[test]
    fn each_input_changes_output() {
        let base = roll(1, 2, 3);
        assert_ne!(base, roll(2, 2, 3));
        assert_ne!(base, roll(1, 3, 3));
        assert_ne!(base, roll(1, 2, 4));
    }

    #[test]
    fn below_stays_in_range_and_spreads() {
        let mut seen = [0u32; 6];
        for e in 0..6000 {
            let v = below(7, 1, e, 6);
            assert!(v < 6);
            seen[v as usize] += 1;
        }
        assert!(seen.iter().all(|&c| (850..1150).contains(&c)), "{seen:?}");
    }

    /// Fester Wert: ändert sich nur, wenn sich der Algorithmus ändert.
    /// Bricht dieser Test, laufen alte Speicherstände anders.
    #[test]
    fn known_value() {
        assert_eq!(roll(1, 0, 0), 0x179f_01f0_d2fb_a97a);
    }
}

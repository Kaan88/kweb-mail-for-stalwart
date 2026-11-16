//! Stalwart's state strings, read only to see whether the server's state went backwards.
//!
//! A state is the letter `s` and a counter: the counter as a LEB128 number (seven bits a byte, the
//! high bit says another byte follows), the bytes written as a bit stream in groups of five bits
//! with the alphabet below. Found by reading states of the local 1.1 server before and after
//! single changes: they count up by one. A server restored from a backup, or upgraded, can answer
//! `/changes` since a state it has not reached with an empty list and its own (lower) new state,
//! which would leave the cache silently without what happened since. A string that is not in this
//! form (another server, a future format) decodes to nothing and is trusted as before.

const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz792013";

/// The counter of a state, or `None` for a string that is not a Stalwart state.
pub fn decode(state: &str) -> Option<u64> {
    let rest = state.strip_prefix('s')?;
    if rest.is_empty() || rest.len() > 16 {
        return None;
    }
    let mut bits: u64 = 0;
    let mut have = 0u32;
    let mut number: u64 = 0;
    let mut shift = 0u32;
    for c in rest.bytes() {
        let value = ALPHABET.iter().position(|a| *a == c)? as u64;
        bits = (bits << 5) | value;
        have += 5;
        while have >= 8 {
            let byte = ((bits >> (have - 8)) & 0xff) as u8;
            have -= 8;
            bits &= (1 << have) - 1;
            if shift >= 63 {
                return None;
            }
            number |= u64::from(byte & 0x7f) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                return Some(number);
            }
        }
    }
    None
}

/// True when `new` is a Stalwart state below `old`: the server is behind what the cache has seen.
/// False when either does not decode, and for equal or higher states.
pub fn went_backwards(old: &str, new: &str) -> bool {
    matches!((decode(old), decode(new)), (Some(old), Some(new)) if new < old)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_read_from_the_server_count_up_by_one() {
        // Seen on the local Stalwart 1.1 after a flag was set and taken off, again and again.
        let seen = ["syu2q", "syy2q", "sy22q", "sza2q", "sze2q"];
        let counters: Vec<u64> = seen.iter().map(|s| decode(s).unwrap()).collect();
        assert_eq!(counters, [7365, 7366, 7367, 7368, 7369]);
    }

    #[test]
    fn what_is_not_a_state_decodes_to_nothing() {
        for s in ["", "s", "es5", "ms7", "n", "s!!", "SYU2Q", "syu2", "sssssssssssssssssssss"] {
            assert_eq!(decode(s), None, "{s}");
        }
    }

    #[test]
    fn only_a_lower_stalwart_state_is_backwards() {
        assert!(went_backwards("sze2q", "syu2q"));
        assert!(!went_backwards("syu2q", "sze2q"));
        assert!(!went_backwards("syu2q", "syu2q"));
        // Not Stalwart's form: trusted.
        assert!(!went_backwards("es9", "es2"));
        assert!(!went_backwards("syu2q", "es2"));
    }
}

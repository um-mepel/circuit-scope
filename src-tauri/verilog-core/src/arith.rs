//! Verilog-style arithmetic right shift (`>>>`): sign-extend `width_bits`, then shift.

/// Sign-extend a `width_bits`-wide two's complement value to full `i64` range.
pub fn sign_extend_i64(v: i64, width_bits: u32) -> i64 {
    let w = width_bits.clamp(1, 63);
    let mask_u = (1u64 << w) - 1;
    let v = ((v as u64) & mask_u) as i64;
    (v << (64 - w)) >> (64 - w)
}

/// IEEE 1364 `>>>`: treat `v` as a signed `width_bits`-wide value, then arithmetic shift right.
pub fn arith_shr_i64(v: i64, sh: u32, width_bits: u32) -> i64 {
    let w = width_bits.clamp(1, 63);
    let sh = sh.min(63);
    let mask_u = (1u64 << w) - 1;
    let v = ((v as u64) & mask_u) as i64;
    let signed = (v << (64 - w)) >> (64 - w);
    signed >> sh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arith_shr_11_bit_negative_one_step() {
        let m11 = (1i64 << 11) - 1;
        let neg5 = m11 & (2048 - 5);
        let neg3 = m11 & (2048 - 3);
        let out = arith_shr_i64(neg5, 1, 11) & m11;
        assert_eq!(out, neg3);
    }

    #[test]
    fn sign_extend_11_bit_negative() {
        let bits = 2043i64;
        assert_eq!(sign_extend_i64(bits, 11), -5);
    }
}

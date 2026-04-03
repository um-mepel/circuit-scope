//! Exact `#delay` values as reduced rationals (Verilog time units).

/// `num` / `den` time-unit steps from the `` `timescale`` **unit** (first operand).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DelayRational {
    pub num: u128,
    pub den: u128,
}

fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

impl DelayRational {
    pub const ZERO: Self = Self { num: 0, den: 1 };

    pub fn new(num: u128, den: u128) -> Self {
        if den == 0 || num == 0 {
            return Self::ZERO;
        }
        let g = gcd_u128(num, den);
        Self {
            num: num / g,
            den: den / g,
        }
    }

    pub fn from_int(n: u64) -> Self {
        Self::new(n as u128, 1)
    }

    /// Parse `#lexeme` body (no `#`), e.g. `5`, `0.5`, `10.25`.
    pub fn from_delay_lexeme(s: &str) -> Self {
        let s = s.trim();
        if s.is_empty() {
            return Self::ZERO;
        }
        if let Ok(n) = s.parse::<u64>() {
            return Self::from_int(n);
        }
        if let Ok(n) = s.parse::<u128>() {
            return Self::new(n, 1);
        }
        if let Some(dot_pos) = s.find('.') {
            let int_part = s[..dot_pos].parse::<u128>().unwrap_or(0);
            let frac_raw = &s[dot_pos + 1..];
            let frac_trim = frac_raw.trim_end_matches('0');
            if frac_trim.is_empty() {
                return Self::new(int_part, 1);
            }
            if let Ok(frac_val) = frac_trim.parse::<u128>() {
                let pow10 = 10u128.pow(frac_trim.len() as u32);
                let num = int_part.saturating_mul(pow10).saturating_add(frac_val);
                return Self::new(num, pow10);
            }
        }
        if let Ok(f) = s.parse::<f64>() {
            if !f.is_finite() || f <= 0.0 {
                return Self::new(1, 1);
            }
            let c = f.ceil() as u128;
            return Self::new(c.max(1), 1);
        }
        Self::ZERO
    }

    pub fn add(self, b: Self) -> Self {
        if self.den == 0 || b.den == 0 {
            return Self::ZERO;
        }
        let num = self
            .num
            .saturating_mul(b.den)
            .saturating_add(b.num.saturating_mul(self.den));
        let den = self.den.saturating_mul(b.den);
        Self::new(num, den)
    }

    pub fn saturating_mul_u128(self, n: u128) -> Self {
        if n == 0 || self.num == 0 {
            return Self::ZERO;
        }
        Self::new(self.num.saturating_mul(n), self.den)
    }

    /// Event time in femtoseconds: `(num * unit_fs) / den`.
    pub fn to_femtoseconds(self, unit_fs: u128) -> u128 {
        if self.den == 0 {
            return 0;
        }
        self.num.saturating_mul(unit_fs) / self.den
    }

    /// Ceil of value in whole time units (for cycle budgeting heuristics).
    pub fn ceil_whole_time_units(self) -> usize {
        if self.den == 0 {
            return 0;
        }
        usize::try_from((self.num + self.den - 1) / self.den).unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_delay() {
        let d = DelayRational::from_delay_lexeme("0.5");
        assert_eq!(d.num, 1);
        assert_eq!(d.den, 2);
        let fs = 1_000_000_000_000_000u128;
        assert_eq!(d.to_femtoseconds(fs), 500_000_000_000_000);
    }

    #[test]
    fn add_halves() {
        let a = DelayRational::from_delay_lexeme("0.5");
        let s = a.add(a);
        assert_eq!(s.num, 1);
        assert_eq!(s.den, 1);
    }
}

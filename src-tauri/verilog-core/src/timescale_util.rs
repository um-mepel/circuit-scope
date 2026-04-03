//! Helpers for `` `timescale`` unit vs precision and the simulation clock grid.

/// Femtoseconds for one Verilog time-literal token (`1ns`, `100 ms`, …).
pub fn timescale_token_to_fs(s: &str) -> Option<u128> {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return None;
    }
    let idx = compact.find(|c: char| c.is_alphabetic())?;
    let n = compact[..idx].parse::<u128>().ok()?;
    if n == 0 {
        return None;
    }
    let unit = compact[idx..].to_ascii_lowercase();
    let mult: u128 = match unit.as_str() {
        "s" => 1_000_000_000_000_000u128,
        "ms" => 1_000_000_000_000,
        "us" => 1_000_000_000,
        "ns" => 1_000_000,
        "ps" => 1_000,
        "fs" => 1,
        _ => return None,
    };
    Some(n.saturating_mul(mult))
}

pub const ONE_SECOND_FS: u128 = 1_000_000_000_000_000;

/// How many `` `timescale`` **precision** steps fit in one **time unit**.
/// Returns `1` when operands are missing or invalid.
pub fn unit_per_precision_ratio(time_unit: &str, time_precision: &str) -> usize {
    let Some(uf) = timescale_token_to_fs(time_unit) else {
        return 1;
    };
    let Some(pf) = timescale_token_to_fs(time_precision) else {
        return 1;
    };
    if pf == 0 || uf < pf {
        return 1;
    }
    let q = uf / pf;
    if q == 0 {
        return 1;
    }
    usize::try_from(q).unwrap_or(usize::MAX)
}

/// Kernel clock half-period in **precision** (fine) ticks.
///
/// - Usually `clock_half_period_units * k` so the CLI default `5` still means “5 time units”.
/// - For **coarse** time units (≥ 1 real second) with finer precision, the implicit default uses
///   **5 precision steps** (e.g. `1s/100ms` → 500 ms half-period instead of 5 s).
pub fn clock_half_period_fine_ticks(
    clock_half_period_units: usize,
    k: usize,
    clock_half_period_explicit: bool,
    time_unit: &str,
) -> usize {
    let k = k.max(1);
    let user = clock_half_period_units.max(1);
    let coarse = timescale_token_to_fs(time_unit).unwrap_or(0) >= ONE_SECOND_FS;
    if !clock_half_period_explicit && coarse && k > 1 {
        5
    } else {
        user.saturating_mul(k)
    }
}

pub fn div_ceil(a: usize, b: usize) -> usize {
    if b == 0 {
        return a;
    }
    (a + b - 1) / b
}

/// Like [`crate::csverilog_pipeline::num_cycles_from_initial_delay_sum`] but delays are in **time units**
/// and the clock grid is in **precision** ticks (`h_fine`).
pub fn num_cycles_from_initial_delay_sum_fine(
    delay_sum_units: usize,
    k: usize,
    h_fine: usize,
) -> usize {
    if delay_sum_units == 0 {
        return 100;
    }
    let k = k.max(1);
    let h = h_fine.max(1);
    let delay_fine = delay_sum_units.saturating_mul(k);
    let period_fine = 2usize.saturating_mul(h);
    div_ceil(delay_fine, period_fine).max(1)
}

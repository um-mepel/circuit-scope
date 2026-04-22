//! Shared helpers for integration tests (VCD sampling).

pub fn find_var_code_in_scope(vcd: &str, scope_substr: &str, var_name: &str) -> Option<String> {
    let mut active = false;
    let mut scope_depth = 0i32;
    for line in vcd.lines() {
        let t = line.trim();
        if t.starts_with("$scope module ") {
            scope_depth += 1;
            if t.contains(scope_substr) {
                active = true;
            }
        } else if t == "$upscope $end" {
            scope_depth -= 1;
            if scope_depth <= 0 {
                active = false;
            }
        } else if active && scope_depth == 1 && t.starts_with("$var ") {
            let parts: Vec<&str> = t.split_whitespace().collect();
            if parts.len() >= 5 && parts[4] == var_name {
                return Some(parts[3].to_string());
            }
        }
    }
    None
}

pub fn parse_binary_changes_for_code(vcd: &str, code: &str) -> Vec<(u64, i64)> {
    let mut started = false;
    let mut t = 0u64;
    let mut out = Vec::new();
    for line in vcd.lines() {
        let line = line.trim();
        if !started {
            if line.starts_with("$enddefinitions") {
                started = true;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            if let Some(n) = rest.split_whitespace().next() {
                if let Ok(v) = n.parse::<u64>() {
                    t = v;
                }
            }
            continue;
        }
        if line.starts_with('b') {
            let mut it = line.split_whitespace();
            let b = it.next().unwrap_or("");
            let id = it.next().unwrap_or("");
            if id == code && b.len() > 1 {
                if let Ok(v) = i64::from_str_radix(&b[1..], 2) {
                    out.push((t, v));
                }
            }
        }
    }
    out
}

pub fn value_at_or_before(samples: &[(u64, i64)], t_query: u64) -> Option<i64> {
    let mut best: Option<(u64, i64)> = None;
    for &(t, v) in samples {
        if t <= t_query {
            best = Some((t, v));
        }
    }
    best.map(|(_, v)| v)
}

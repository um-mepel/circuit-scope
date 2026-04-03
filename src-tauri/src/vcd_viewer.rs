//! In-app VCD waveform: parse with `wellen` (same stack as tests), expose hierarchy + windowed traces to the UI.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use wellen::viewers::{read_body, read_header_from_file};
use wellen::{
    Hierarchy, LoadOptions, ScopeOrVarRef, ScopeRef, SignalRef, SignalSource, Time, TimescaleUnit,
};

pub struct VcdSessionHolder {
    pub inner: Mutex<Option<VcdSession>>,
}

impl Default for VcdSessionHolder {
    fn default() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

pub struct VcdSession {
    #[allow(dead_code)]
    pub project_root: String,
    pub path: String,
    pub hierarchy: Hierarchy,
    pub time_table: Vec<Time>,
    pub source: SignalSource,
}

fn normalize_path(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// `file` must be equal to or nested under `project` (after canonicalize when possible).
pub fn path_under_project(project: &str, file: &str) -> Result<PathBuf, String> {
    let proj = Path::new(project);
    let fp = Path::new(file);
    if !fp.is_absolute() {
        return Err("VCD path must be absolute".to_string());
    }
    let proj_n = normalize_path(proj);
    let file_n = normalize_path(fp);
    if !file_n.starts_with(&proj_n) {
        return Err(format!(
            "Path must be under project folder:\n  project: {}\n  file: {}",
            proj_n.display(),
            file_n.display()
        ));
    }
    if !file_n.is_file() {
        return Err(format!("Not a file: {}", file_n.display()));
    }
    let ext = file_n.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !ext.eq_ignore_ascii_case("vcd") {
        return Err("Only .vcd files are supported".to_string());
    }
    Ok(file_n)
}

fn timescale_unit_label(u: TimescaleUnit) -> &'static str {
    match u {
        TimescaleUnit::ZeptoSeconds => "zs",
        TimescaleUnit::AttoSeconds => "as",
        TimescaleUnit::FemtoSeconds => "fs",
        TimescaleUnit::PicoSeconds => "ps",
        TimescaleUnit::NanoSeconds => "ns",
        TimescaleUnit::MicroSeconds => "us",
        TimescaleUnit::MilliSeconds => "ms",
        TimescaleUnit::Seconds => "s",
        TimescaleUnit::Unknown => "unknown",
    }
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VcdVarNode {
    pub signal_id: u32,
    pub name: String,
    pub bits: u32,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VcdScopeNode {
    pub name: String,
    pub scopes: Vec<VcdScopeNode>,
    pub vars: Vec<VcdVarNode>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VcdOpenResponse {
    pub path: String,
    pub time_start: u64,
    pub time_end: u64,
    pub timescale_factor: Option<u32>,
    pub timescale_unit: Option<String>,
    pub hierarchy: Vec<VcdScopeNode>,
}

fn scope_subtree(h: &Hierarchy, sref: ScopeRef) -> VcdScopeNode {
    let s = &h[sref];
    let name = s.name(h).to_string();
    let mut scopes = Vec::new();
    let mut vars = Vec::new();
    for item in s.items(h) {
        match item {
            ScopeOrVarRef::Scope(child) => scopes.push(scope_subtree(h, child)),
            ScopeOrVarRef::Var(vr) => {
                let v = &h[vr];
                let bits = v.length().unwrap_or(1);
                vars.push(VcdVarNode {
                    signal_id: v.signal_ref().index() as u32,
                    name: v.name(h).to_string(),
                    bits,
                });
            }
        }
    }
    VcdScopeNode {
        name,
        scopes,
        vars,
    }
}

fn hierarchy_tree(h: &Hierarchy) -> Vec<VcdScopeNode> {
    h.scopes().map(|sref| scope_subtree(h, sref)).collect()
}

fn bits_for_signal(h: &Hierarchy, want: SignalRef) -> u32 {
    fn walk(h: &Hierarchy, sref: ScopeRef, want: SignalRef) -> Option<u32> {
        let s = &h[sref];
        for item in s.items(h) {
            match item {
                ScopeOrVarRef::Scope(ch) => {
                    if let Some(b) = walk(h, ch, want) {
                        return Some(b);
                    }
                }
                ScopeOrVarRef::Var(vr) => {
                    let v = &h[vr];
                    if v.signal_ref() == want {
                        return Some(v.length().unwrap_or(1));
                    }
                }
            }
        }
        None
    }
    for top in h.scopes() {
        if let Some(b) = walk(h, top, want) {
            return b;
        }
    }
    1
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EdgeKind {
    Any,
    Rising,
    Falling,
}

fn parse_bin_u128(s: &str) -> Option<u128> {
    let t = s.trim();
    if t.is_empty() || !t.chars().all(|c| c == '0' || c == '1') {
        return None;
    }
    u128::from_str_radix(t, 2).ok()
}

fn bit01(s: &str) -> Option<u8> {
    let t = s.trim();
    match t {
        "0" => Some(0),
        "1" => Some(1),
        _ => {
            if t.len() == 1 {
                match t.as_bytes()[0] {
                    b'0' => Some(0),
                    b'1' => Some(1),
                    _ => None,
                }
            } else {
                None
            }
        }
    }
}

fn edge_matches(prev: &str, new: &str, bits: u32, kind: EdgeKind) -> bool {
    if prev == new {
        return false;
    }
    match kind {
        EdgeKind::Any => true,
        EdgeKind::Rising => {
            if bits <= 1 {
                bit01(prev) == Some(0) && bit01(new) == Some(1)
            } else {
                match (parse_bin_u128(prev), parse_bin_u128(new)) {
                    (Some(a), Some(b)) => b > a,
                    _ => false,
                }
            }
        }
        EdgeKind::Falling => {
            if bits <= 1 {
                bit01(prev) == Some(1) && bit01(new) == Some(0)
            } else {
                match (parse_bin_u128(prev), parse_bin_u128(new)) {
                    (Some(a), Some(b)) => b < a,
                    _ => false,
                }
            }
        }
    }
}

/// Pad or trim MSB-first binary to `width` chars; LSB is last character.
fn normalize_bus_binary_msb(s: &str, width: u32) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || !t.chars().all(|c| c == '0' || c == '1') {
        return None;
    }
    let w = width as usize;
    if t.len() >= w {
        Some(t[t.len() - w..].to_string())
    } else {
        Some(format!("{:0>width$}", t, width = w))
    }
}

fn bit_at_lsb_index(bin_msb: &str, bit_lsb: u32, width: u32) -> Option<u8> {
    let i = (width as usize).checked_sub(1)?.checked_sub(bit_lsb as usize)?;
    match bin_msb.as_bytes().get(i)? {
        b'0' => Some(0),
        b'1' => Some(1),
        _ => None,
    }
}

fn edge_matches_lsb_bit(
    prev: &str,
    new: &str,
    width: u32,
    bit_lsb: u32,
    kind: EdgeKind,
) -> bool {
    let Some(pb) = normalize_bus_binary_msb(prev, width)
        .and_then(|b| bit_at_lsb_index(&b, bit_lsb, width))
    else {
        return false;
    };
    let Some(nb) =
        normalize_bus_binary_msb(new, width).and_then(|b| bit_at_lsb_index(&b, bit_lsb, width))
    else {
        return false;
    };
    if pb == nb {
        return false;
    }
    match kind {
        EdgeKind::Any => true,
        EdgeKind::Rising => pb == 0 && nb == 1,
        EdgeKind::Falling => pb == 1 && nb == 0,
    }
}

#[tauri::command]
pub fn vcd_open(
    holder: tauri::State<'_, VcdSessionHolder>,
    project_root: String,
    path: String,
) -> Result<VcdOpenResponse, String> {
    let fp = path_under_project(&project_root, &path)?;
    let opts = LoadOptions {
        multi_thread: false,
        remove_scopes_with_empty_name: false,
    };
    let header = read_header_from_file(&fp, &opts).map_err(|e| e.to_string())?;
    let hierarchy = header.hierarchy;
    let body = read_body(header.body, &hierarchy, None).map_err(|e| e.to_string())?;

    let time_start = body.time_table.first().copied().unwrap_or(0);
    let time_end = body.time_table.last().copied().unwrap_or(0);
    let ts = hierarchy.timescale();
    let (timescale_factor, timescale_unit) = match ts {
        Some(t) => (
            Some(t.factor),
            Some(timescale_unit_label(t.unit).to_string()),
        ),
        None => (None, None),
    };

    let tree = hierarchy_tree(&hierarchy);
    let path_str = fp.to_string_lossy().to_string();

    let session = VcdSession {
        project_root: project_root.clone(),
        path: path_str.clone(),
        hierarchy,
        time_table: body.time_table,
        source: body.source,
    };
    *holder.inner.lock().map_err(|e| e.to_string())? = Some(session);

    Ok(VcdOpenResponse {
        path: path_str,
        time_start,
        time_end,
        timescale_factor,
        timescale_unit,
        hierarchy: tree,
    })
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VcdTransition {
    pub signal_id: u32,
    pub time: u64,
    pub value: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VcdQueryResponse {
    pub transitions: Vec<VcdTransition>,
}

#[tauri::command]
pub fn vcd_query(
    holder: tauri::State<'_, VcdSessionHolder>,
    project_root: String,
    path: String,
    signal_ids: Vec<u32>,
    t_start: u64,
    t_end: u64,
    max_points_per_signal: Option<usize>,
) -> Result<VcdQueryResponse, String> {
    let fp = path_under_project(&project_root, &path)?;
    let path_str = fp.to_string_lossy().to_string();

    let mut guard = holder.inner.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or_else(|| "No VCD loaded; open a waveform first".to_string())?;
    if session.path != path_str || session.project_root != project_root {
        return Err("VCD session path mismatch; reload waveform".to_string());
    }

    let max_pts = max_points_per_signal.unwrap_or(2048).max(32);

    let tt_min = session.time_table.first().copied().unwrap_or(0);
    let tt_max = session.time_table.last().copied().unwrap_or(tt_min);
    let (raw0, raw1) = (t_start.min(t_end), t_start.max(t_end));
    let mut q0 = raw0.clamp(tt_min, tt_max);
    let mut q1 = raw1.clamp(tt_min, tt_max);
    if q1 < q0 {
        std::mem::swap(&mut q0, &mut q1);
    }
    if q1 <= q0 {
        q1 = (q0 + 1).min(tt_max);
    }
    if q1 <= q0 {
        q0 = q1.saturating_sub(1).max(tt_min);
    }
    let t_start = q0;
    let t_end = q1;

    let refs: Vec<SignalRef> = signal_ids
        .into_iter()
        .filter_map(|i| SignalRef::from_index(i as usize))
        .collect();
    if refs.is_empty() {
        return Ok(VcdQueryResponse {
            transitions: vec![],
        });
    }

    let signals = session
        .source
        .load_signals(&refs, &session.hierarchy, false);

    let tt = &session.time_table;
    let mut transitions: Vec<VcdTransition> = Vec::new();

    for (sig_ref, signal) in signals {
        let sid = sig_ref.index() as u32;
        let mut local: Vec<VcdTransition> = Vec::new();
        let mut last_before_start: Option<String> = None;
        for (time_idx, value) in signal.iter_changes() {
            let ti = time_idx as usize;
            if ti >= tt.len() {
                continue;
            }
            let t = tt[ti];
            let value_str = format!("{value}");
            if t < t_start {
                last_before_start = Some(value_str);
                continue;
            }
            if t > t_end {
                break;
            }
            local.push(VcdTransition {
                signal_id: sid,
                time: t,
                value: value_str,
            });
        }
        if local.is_empty() {
            if let Some(v) = last_before_start {
                local.push(VcdTransition {
                    signal_id: sid,
                    time: t_start,
                    value: v,
                });
            }
        } else if local[0].time > t_start {
            if let Some(v) = last_before_start {
                local.insert(
                    0,
                    VcdTransition {
                        signal_id: sid,
                        time: t_start,
                        value: v,
                    },
                );
            }
        }
        transitions.extend(decimate_transitions_slice(
            &local, max_pts, t_start, t_end, sid,
        ));
    }

    transitions.sort_by(|a, b| a.time.cmp(&b.time).then_with(|| a.signal_id.cmp(&b.signal_id)));

    Ok(VcdQueryResponse { transitions })
}

fn value_at_or_before(times: &[VcdTransition], t: u64) -> Option<&str> {
    if times.is_empty() {
        return None;
    }
    let idx = times.partition_point(|r| r.time <= t);
    if idx == 0 {
        None
    } else {
        Some(times[idx - 1].value.as_str())
    }
}

/// Subsample along **time** (not change-index) so step waveforms stay coherent when zoomed out.
/// Collapses consecutive samples with the same value.
fn decimate_transitions_slice(
    rows: &[VcdTransition],
    max: usize,
    t_start: u64,
    t_end: u64,
    sid: u32,
) -> Vec<VcdTransition> {
    if rows.is_empty() {
        return vec![];
    }
    if rows.len() <= max || max < 2 {
        return rows.to_vec();
    }

    let span_u = t_end.saturating_sub(t_start);
    let span = span_u.max(1) as u128;
    let denom = (max - 1).max(1) as u128;

    let mut samples: Vec<VcdTransition> = Vec::with_capacity(max);
    for k in 0..max {
        let t = if max <= 1 {
            t_start
        } else {
            let off = (span * (k as u128)) / denom;
            t_start.saturating_add(off as u64)
        };
        let value_str = value_at_or_before(rows, t)
            .unwrap_or_else(|| rows[0].value.as_str())
            .to_string();
        samples.push(VcdTransition {
            signal_id: sid,
            time: t,
            value: value_str,
        });
    }

    let mut collapsed: Vec<VcdTransition> = Vec::with_capacity(samples.len());
    for tr in samples {
        if let Some(last) = collapsed.last() {
            if last.value == tr.value {
                continue;
            }
        }
        collapsed.push(tr);
    }
    collapsed
}

/// Find next or previous transition for one signal matching `edge_kind`:
/// `"any"` | `"rising"` | `"falling"` (rising/falling use 0→1 / 1→0 for 1-bit, numeric compare for pure binary buses).
#[tauri::command]
pub fn vcd_find_edge(
    holder: tauri::State<'_, VcdSessionHolder>,
    project_root: String,
    path: String,
    signal_id: u32,
    from_time: u64,
    next: bool,
    edge_kind: String,
    bit_lsb: Option<u32>,
) -> Result<Option<u64>, String> {
    let fp = path_under_project(&project_root, &path)?;
    let path_str = fp.to_string_lossy().to_string();

    let mut guard = holder.inner.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or_else(|| "No VCD loaded; open a waveform first".to_string())?;
    if session.path != path_str || session.project_root != project_root {
        return Err("VCD session path mismatch; reload waveform".to_string());
    }

    let sig_ref = SignalRef::from_index(signal_id as usize)
        .ok_or_else(|| format!("Invalid signal id {}", signal_id))?;
    let bits = bits_for_signal(&session.hierarchy, sig_ref);

    let kind = match edge_kind.to_lowercase().as_str() {
        "rising" => EdgeKind::Rising,
        "falling" => EdgeKind::Falling,
        _ => EdgeKind::Any,
    };

    let signals = session
        .source
        .load_signals(&[sig_ref], &session.hierarchy, false);
    let (_, signal) = signals
        .into_iter()
        .next()
        .ok_or_else(|| "Signal not found".to_string())?;

    let tt = &session.time_table;
    let mut points: Vec<(u64, String)> = Vec::new();
    for (time_idx, value) in signal.iter_changes() {
        let ti = time_idx as usize;
        if ti >= tt.len() {
            continue;
        }
        let t = tt[ti];
        points.push((t, format!("{value}")));
    }

    if points.len() < 2 {
        return Ok(None);
    }

    if next {
        for i in 1..points.len() {
            let (t_edge, ref new_v) = points[i];
            let (_, ref prev_v) = points[i - 1];
            let ok = match bit_lsb {
                Some(b) if bits > 1 => edge_matches_lsb_bit(prev_v, new_v, bits, b, kind),
                _ => edge_matches(prev_v, new_v, bits, kind),
            };
            if t_edge > from_time && ok {
                return Ok(Some(t_edge));
            }
        }
        Ok(None)
    } else {
        let mut best: Option<u64> = None;
        for i in 1..points.len() {
            let (t_edge, ref new_v) = points[i];
            let (_, ref prev_v) = points[i - 1];
            let ok = match bit_lsb {
                Some(b) if bits > 1 => edge_matches_lsb_bit(prev_v, new_v, bits, b, kind),
                _ => edge_matches(prev_v, new_v, bits, kind),
            };
            if t_edge < from_time && ok {
                best = Some(t_edge);
            }
        }
        Ok(best)
    }
}

#[tauri::command]
pub fn vcd_close(holder: tauri::State<'_, VcdSessionHolder>) -> Result<(), String> {
    *holder.inner.lock().map_err(|e| e.to_string())? = None;
    Ok(())
}

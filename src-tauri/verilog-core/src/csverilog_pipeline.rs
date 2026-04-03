//! Shared logic for the `csverilog` binary and the Tauri app (no `cargo run` subprocess).
use std::path::{Path, PathBuf};

use crate::ir::sum_initial_delay_literals_for_source_file;
use crate::timescale_util::{
    clock_half_period_fine_ticks, num_cycles_from_initial_delay_sum_fine,
    unit_per_precision_ratio,
};
use crate::{
    build_ir_for_file, generate_vcd, optimize_project, Diagnostic, IrProject, SimConfig, VcdRunMeta,
    Severity,
};

/// Overrides for VCD time base and how long the fixed-step kernel runs (`num_cycles` clock periods).
#[derive(Debug, Clone, Default)]
pub struct CsVerilogOptions {
    /// Simulation length in **clock periods** (each period = two half-period steps in the kernel).
    /// If `None`, derived from `#delay` sums in `initial` (see pipeline).
    pub num_cycles: Option<usize>,
    /// Override **both** Verilog time unit and VCD `$timescale` string (same value for each; skips project `` `timescale`` scan). If `None`, require a single matching preamble `` `timescale`` across inputs (or default 1ns/1ns when none).
    pub timescale: Option<String>,
    /// Clock half-period in **timescale units** (integer). If `None`, default `5`.
    pub clock_half_period: Option<usize>,
}

/// First `` `timescale timeunit / timeprecision `` for the project.
#[derive(Debug, Clone)]
pub struct TimescaleScan {
    /// Path to the first file that contained the directive, if any (metadata).
    pub declaration_path: Option<PathBuf>,
    pub time_unit: String,
    pub time_precision: String,
    /// Every input file whose **preamble** declares this same (unique) `timescale`.
    pub timescale_source_files: Vec<PathBuf>,
}

impl TimescaleScan {
    /// Default `1ns/1ns` when no directive exists in the listed files.
    pub fn default_timescale() -> Self {
        Self {
            declaration_path: None,
            time_unit: "1ns".into(),
            time_precision: "1ns".into(),
            timescale_source_files: Vec::new(),
        }
    }
}

fn normalize_timescale_tokens(unit: &str, prec: &str) -> (String, String) {
    let u: String = unit.chars().filter(|c| !c.is_whitespace()).collect();
    let p: String = prec.chars().filter(|c| !c.is_whitespace()).collect();
    (u, p)
}

/// All preamble `` `timescale`` directives in the project must agree. Otherwise returns an error.
pub fn scan_timescale_project(paths: &[PathBuf]) -> Result<TimescaleScan, String> {
    let mut hits: Vec<(PathBuf, String, String)> = Vec::new();
    for p in paths {
        let Ok(contents) = std::fs::read_to_string(p) else {
            continue;
        };
        if let Some((u, pr)) = timescale_pair_from_source_preamble(&contents) {
            hits.push((p.clone(), u, pr));
        }
    }
    if hits.is_empty() {
        return Ok(TimescaleScan::default_timescale());
    }
    let mut groups: std::collections::HashMap<(String, String), (String, String, Vec<PathBuf>)> =
        std::collections::HashMap::new();
    let mut first_path: Option<PathBuf> = None;
    for (path, u, pr) in hits {
        let key = normalize_timescale_tokens(&u, &pr);
        if first_path.is_none() {
            first_path = Some(path.clone());
        }
        groups
            .entry(key)
            .or_insert_with(|| (u.clone(), pr.clone(), Vec::new()))
            .2
            .push(path);
    }
    if groups.len() > 1 {
        let mut parts: Vec<String> = groups
            .iter()
            .map(|(key, (_, _, ps))| {
                let files: Vec<String> = ps
                    .iter()
                    .map(|x| x.to_string_lossy().into_owned())
                    .collect();
                let (u, p) = key;
                format!("`timescale {}/{}` in {}", u, p, files.join(", "))
            })
            .collect();
        parts.sort();
        return Err(format!(
            "conflicting `` `timescale`` directives (must match project-wide):\n{}",
            parts.join("\n")
        ));
    }
    let (_, (time_unit, time_precision, paths_vec)) = groups.into_iter().next().unwrap();
    Ok(TimescaleScan {
        declaration_path: first_path,
        time_unit,
        time_precision,
        timescale_source_files: paths_vec,
    })
}

/// Sum of `#delay` literals in `initial` across all listed source files.
pub fn sum_initial_delays_for_source_files(project: &IrProject, files: &[PathBuf]) -> usize {
    let mut total = 0usize;
    for p in files {
        total += sum_initial_delay_literals_for_source_file(project, p.as_path());
    }
    total
}

/// Parse `` `timescale unit / prec `` from `line` (handles `//` line comments).
fn parse_timescale_directive_line(raw_line: &str) -> Option<(String, String)> {
    let line = raw_line.split("//").next().unwrap_or("").trim_start();
    let rest = line.strip_prefix("`timescale")?.trim();
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.splitn(2, '/');
    let unit = parts.next()?.trim();
    if unit.is_empty() {
        return None;
    }
    let prec = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(unit)
        .to_string();
    Some((unit.to_string(), prec))
}

/// Only lines **before** the first `module` / `macromodule` line (Verilog preamble).
fn timescale_pair_from_source_preamble(src: &str) -> Option<(String, String)> {
    for raw_line in src.lines() {
        let code = raw_line.split("//").next().unwrap_or("").trim_start();
        if code.starts_with("module ")
            || code.starts_with("macromodule ")
            || code == "module"
            || code == "macromodule"
        {
            break;
        }
        if let Some(pair) = parse_timescale_directive_line(raw_line) {
            return Some(pair);
        }
    }
    None
}

fn paths_equal_fs(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    if let (Ok(x), Ok(y)) = (a.canonicalize(), b.canonicalize()) {
        return x == y;
    }
    false
}

/// `num_cycles` so that simulation covers `delay_sum` (in **time units**) with a kernel grid of `k`
/// precision steps per unit and half-period `h_fine` (precision ticks).
pub fn num_cycles_from_initial_delay_sum(delay_sum: usize, clock_half_period: usize) -> usize {
    num_cycles_from_initial_delay_sum_fine(delay_sum, 1, clock_half_period.max(1))
}

/// Same pipeline as `src/bin/csverilog.rs`: merge IR from ordered paths, optimize, emit VCD string.
pub fn run_csverilog_pipeline(
    verilog_paths: &[PathBuf],
    out_vcd_path: &Path,
    command_line_for_meta: &str,
    options: CsVerilogOptions,
) -> Result<String, String> {
    if verilog_paths.is_empty() {
        return Err("no Verilog source files".into());
    }

    let mut all_modules = Vec::new();
    let mut all_diags = Vec::new();
    let mut top_module_name: Option<String> = None;

    for (i, file) in verilog_paths.iter().enumerate() {
        if !file.exists() {
            return Err(format!("file not found: {}", file.display()));
        }
        let src = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        let ir = build_ir_for_file(file.to_string_lossy(), &src);
        if i == 0 {
            if let Some(first) = ir.modules.first() {
                top_module_name = Some(first.name.clone());
            }
        }
        all_diags.extend(ir.diagnostics);
        all_modules.extend(ir.modules);
    }

    let errors: Vec<&Diagnostic> = all_diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    if !errors.is_empty() {
        let mut msg = String::from("Verilog compile failed:\n");
        for d in errors {
            msg.push_str(&d.format_line());
            msg.push('\n');
        }
        return Err(msg.trim_end().to_string());
    }

    if all_modules.is_empty() {
        return Err("no modules found in input files".into());
    }

    let mut project = IrProject {
        modules: all_modules,
        diagnostics: all_diags,
    };

    let top_name = {
        let mut instantiated = std::collections::HashSet::new();
        for m in &project.modules {
            for inst in &m.instances {
                instantiated.insert(inst.module_name.clone());
            }
        }
        let candidates: Vec<&str> = project
            .modules
            .iter()
            .map(|m| m.name.as_str())
            .filter(|n| !instantiated.contains(*n))
            .collect();

        if candidates.len() == 1 {
            candidates[0].to_string()
        } else if let Some(name) = top_module_name {
            name
        } else {
            project.modules[0].name.clone()
        }
    };

    let _metrics = optimize_project(&mut project);

    let output_display = out_vcd_path
        .to_str()
        .ok_or_else(|| "output path is not valid UTF-8".to_string())?
        .to_string();

    let top_mod_path = project
        .modules
        .iter()
        .find(|m| m.name == top_name)
        .map(|m| PathBuf::from(&m.path));

    let top_path_str = top_mod_path
        .as_ref()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| verilog_paths[0].to_string_lossy().into_owned());

    let additional: Vec<String> = verilog_paths
        .iter()
        .filter(|p| {
            top_mod_path
                .as_ref()
                .map(|tpak| !paths_equal_fs(p.as_path(), tpak.as_path()))
                .unwrap_or(true)
        })
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.into_os_string().into_string().ok());

    let vcd_meta = VcdRunMeta {
        top_source_file: Some(top_path_str),
        additional_source_files: additional,
        command_line: Some(command_line_for_meta.to_string()),
        output_vcd_path: Some(output_display),
        working_directory: cwd,
        ir_module_count: 0,
    };

    let clock_half_period_explicit = options.clock_half_period.is_some();
    let clock_half_period = options.clock_half_period.unwrap_or(5);

    let (time_unit, time_precision, delay_files): (String, String, Vec<PathBuf>) =
        if let Some(ref ots) = options.timescale {
            let u = ots.trim().to_string();
            (u.clone(), u, verilog_paths.to_vec())
        } else {
            let ts_decl = scan_timescale_project(verilog_paths)?;
            let files = if ts_decl.timescale_source_files.is_empty() {
                top_mod_path.clone().into_iter().collect()
            } else {
                ts_decl.timescale_source_files.clone()
            };
            (ts_decl.time_unit, ts_decl.time_precision, files)
        };

    let delay_sum = sum_initial_delays_for_source_files(&project, &delay_files);
    let k = unit_per_precision_ratio(&time_unit, &time_precision);
    let h_fine =
        clock_half_period_fine_ticks(clock_half_period, k, clock_half_period_explicit, &time_unit);
    let initial_delay_sum_units = if delay_sum > 0 { Some(delay_sum) } else { None };
    let num_cycles = options.num_cycles.unwrap_or_else(|| {
        num_cycles_from_initial_delay_sum_fine(delay_sum, k, h_fine)
    });

    let config = SimConfig {
        top_module: top_name,
        num_cycles,
        timescale: time_unit,
        timescale_precision: time_precision,
        clock_half_period,
        clock_half_period_is_explicit: clock_half_period_explicit,
        initial_delay_sum_units,
        vcd_meta: Some(vcd_meta),
        ..Default::default()
    };

    generate_vcd(&project, &config)
}

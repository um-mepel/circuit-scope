use std::path::{Path, PathBuf};
use std::process;

use verilog_core::{
    list_verilog_source_paths, run_csverilog_pipeline, CsVerilogOptions, Severity,
    CIRCUIT_SCOPE_PROJECT_ROOT_VAR, circuit_scope_project_root_for_scan,
};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("Usage: csverilog [options] <output> [--explicit <file.v> ...]");
        eprintln!();
        eprintln!("  output       VCD base name or path (.vcd added if missing). Example: lab4 → lab4.vcd");
        eprintln!("  (default)    All Verilog RTL files (.v / .sv = same IEEE-1364 subset) under the project");
        eprintln!("               folder in Circuit Scope (env {CIRCUIT_SCOPE_PROJECT_ROOT_VAR}), else cwd —");
        eprintln!("               same discovery as File → Generate VCD.");
        eprintln!("               Relative output paths use that project folder too, even after `cd` in the app.");
        eprintln!("               Extra words (e.g. *.v) are ignored.");
        eprintln!("  --explicit   Use only the listed files, in order (for scripts/tests).");
        eprintln!();
        eprintln!("Options (all optional):");
        eprintln!("  --cycles N           Run N clock periods (default 100). VCD ends at time N × 2 × half_period.");
        eprintln!("  --timescale UNIT     Override unit and VCD `$timescale` line (same string);");
        eprintln!("                       default: one matching preamble `` `timescale`` in all sources, else 1ns/1ns;");
        eprintln!("                       conflicting `` `timescale`` lines → error (use --timescale or fix files).");
        eprintln!("  --half-period UNITS  Clock step unit count between edges (default 5).");
        process::exit(1);
    }

    let mut options = CsVerilogOptions::default();
    while let Some(first) = args.first() {
        if first == "--explicit" || !first.starts_with("--") {
            break;
        }
        let flag = args.remove(0);
        match flag.as_str() {
            "--cycles" => {
                let Some(n) = args.first().and_then(|s| s.parse::<usize>().ok()) else {
                    eprintln!("error: --cycles requires a positive integer");
                    process::exit(1);
                };
                args.remove(0);
                if n == 0 {
                    eprintln!("error: --cycles must be >= 1");
                    process::exit(1);
                }
                options.num_cycles = Some(n);
            }
            "--timescale" => {
                let Some(s) = args.first() else {
                    eprintln!("error: --timescale requires a value (e.g. 1ns)");
                    process::exit(1);
                };
                let s = s.clone();
                args.remove(0);
                options.timescale = Some(s);
            }
            "--half-period" => {
                let Some(n) = args.first().and_then(|s| s.parse::<usize>().ok()) else {
                    eprintln!("error: --half-period requires a positive integer");
                    process::exit(1);
                };
                args.remove(0);
                if n == 0 {
                    eprintln!("error: --half-period must be >= 1");
                    process::exit(1);
                }
                options.clock_half_period = Some(n);
            }
            _ => {
                eprintln!("error: unknown option {flag}");
                process::exit(1);
            }
        }
    }

    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: could not read current directory: {e}");
            process::exit(1);
        }
    };

    let out_path_str = args.remove(0);

    let explicit_sources = !args.is_empty() && args[0] == "--explicit";

    let verilog_paths: Vec<PathBuf> = if explicit_sources {
        args.remove(0);
        if args.is_empty() {
            eprintln!("error: --explicit requires at least one source file");
            process::exit(1);
        }
        args.iter().map(PathBuf::from).collect()
    } else {
        let scan_root = circuit_scope_project_root_for_scan(&cwd);
        if let Ok(s) = std::env::var(CIRCUIT_SCOPE_PROJECT_ROOT_VAR) {
            let p = PathBuf::from(&s);
            if p.is_dir() && p != cwd {
                eprintln!(
                    "csverilog: using {CIRCUIT_SCOPE_PROJECT_ROOT_VAR}={s} for source scan and relative output paths"
                );
            }
        }
        match list_verilog_source_paths(&scan_root) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: could not scan for Verilog sources: {e}");
                process::exit(1);
            }
        }
    };

    if verilog_paths.is_empty() {
        eprintln!("error: no Verilog sources to compile");
        process::exit(1);
    }

    let out_path = if Path::new(&out_path_str).is_absolute() {
        PathBuf::from(&out_path_str)
    } else if explicit_sources {
        cwd.join(&out_path_str)
    } else {
        circuit_scope_project_root_for_scan(&cwd).join(&out_path_str)
    };
    let out_path = match out_path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("vcd") => out_path,
        _ => out_path.with_extension("vcd"),
    };

    let mut all_diags = Vec::new();
    for file in &verilog_paths {
        if !file.exists() {
            eprintln!("error: file not found: {}", file.display());
            process::exit(1);
        }
        let src = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: cannot read {}: {e}", file.display());
                process::exit(1);
            }
        };
        let ir = verilog_core::build_ir_for_file(file.to_string_lossy(), &src);
        all_diags.extend(ir.diagnostics);
    }

    for d in &all_diags {
        eprintln!("{}", d.format_line());
    }

    let has_errors = all_diags
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));
    if has_errors {
        eprintln!("compilation failed due to errors");
        process::exit(1);
    }

    let command_line = std::env::args().collect::<Vec<_>>().join(" ");
    let vcd = match run_csverilog_pipeline(&verilog_paths, &out_path, &command_line, options) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    if let Err(e) = std::fs::write(&out_path, &vcd) {
        eprintln!("error: cannot write {}: {e}", out_path.display());
        process::exit(1);
    }

    eprintln!(
        "wrote {} ({} source file{})",
        out_path.display(),
        verilog_paths.len(),
        if verilog_paths.len() == 1 { "" } else { "s" }
    );
}

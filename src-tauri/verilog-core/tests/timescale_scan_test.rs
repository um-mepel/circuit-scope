use std::io::Write;

use tempfile::NamedTempFile;
use verilog_core::scan_timescale_project;

#[test]
fn scan_finds_first_timescale_in_order() {
    let mut a = NamedTempFile::new().unwrap();
    writeln!(a, "module a; endmodule").unwrap();
    let mut b = NamedTempFile::new().unwrap();
    writeln!(b, "  `timescale 10us / 1ns").unwrap();
    writeln!(b, "module b; endmodule").unwrap();

    let paths = vec![a.path().to_path_buf(), b.path().to_path_buf()];
    let ts = scan_timescale_project(&paths).unwrap();
    assert_eq!(ts.time_unit, "10us");
    assert_eq!(ts.time_precision, "1ns");
    assert_eq!(ts.declaration_path.as_ref().map(|p| p.as_path()), Some(b.path()));
    assert_eq!(ts.timescale_source_files, vec![b.path().to_path_buf()]);
}

#[test]
fn scan_skips_line_comment_after_timescale_token() {
    let mut f = NamedTempFile::new().unwrap();
    writeln!(f, "`timescale 1ps/1fs  // tricky").unwrap();
    let paths = vec![f.path().to_path_buf()];
    let ts = scan_timescale_project(&paths).unwrap();
    assert_eq!(ts.time_unit, "1ps");
    assert_eq!(ts.time_precision, "1fs");
}

#[test]
fn scan_defaults_to_1ns_slash_1ns() {
    let mut f = NamedTempFile::new().unwrap();
    writeln!(f, "module m; endmodule").unwrap();
    let paths = vec![f.path().to_path_buf()];
    let ts = scan_timescale_project(&paths).unwrap();
    assert!(ts.declaration_path.is_none());
    assert_eq!(ts.time_unit, "1ns");
    assert_eq!(ts.time_precision, "1ns");
    assert!(ts.timescale_source_files.is_empty());
}

#[test]
fn timescale_after_module_is_ignored_use_default() {
    let mut f = NamedTempFile::new().unwrap();
    writeln!(f, "module early; endmodule").unwrap();
    writeln!(f, "`timescale 1us/1ns").unwrap();
    let paths = vec![f.path().to_path_buf()];
    let ts = scan_timescale_project(&paths).unwrap();
    assert!(ts.declaration_path.is_none());
    assert_eq!(ts.time_unit, "1ns");
    assert_eq!(ts.time_precision, "1ns");
    assert!(ts.timescale_source_files.is_empty());
}

#[test]
fn conflicting_preamble_timescales_are_error() {
    let mut rtl = NamedTempFile::new().unwrap();
    writeln!(rtl, "`timescale 1ns/1ps").unwrap();
    writeln!(rtl, "module dut; endmodule").unwrap();
    let mut tb = NamedTempFile::new().unwrap();
    writeln!(tb, "`timescale 1s/100ms").unwrap();
    writeln!(tb, "module tb; endmodule").unwrap();
    let paths = vec![rtl.path().to_path_buf(), tb.path().to_path_buf()];
    let err = scan_timescale_project(&paths).unwrap_err();
    assert!(
        err.contains("conflicting"),
        "expected conflict message, got: {err}"
    );
}

#[test]
fn same_timescale_in_two_files_ok_and_both_listed_for_delays() {
    let mut a = NamedTempFile::new().unwrap();
    writeln!(a, "`timescale 1ns / 1ps").unwrap();
    writeln!(a, "module dut; endmodule").unwrap();
    let mut b = NamedTempFile::new().unwrap();
    writeln!(b, "`timescale 1ns/1ps").unwrap();
    writeln!(b, "module tb; endmodule").unwrap();
    let paths = vec![a.path().to_path_buf(), b.path().to_path_buf()];
    let ts = scan_timescale_project(&paths).unwrap();
    assert_eq!(ts.time_unit, "1ns");
    assert_eq!(ts.time_precision, "1ps");
    let mut got = ts.timescale_source_files.clone();
    got.sort();
    let mut want = vec![a.path().to_path_buf(), b.path().to_path_buf()];
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn single_unit_timescale_duplicates_as_precision() {
    let mut f = NamedTempFile::new().unwrap();
    writeln!(f, "`timescale 1ns").unwrap();
    let paths = vec![f.path().to_path_buf()];
    let ts = scan_timescale_project(&paths).unwrap();
    assert_eq!(ts.time_unit, "1ns");
    assert_eq!(ts.time_precision, "1ns");
}

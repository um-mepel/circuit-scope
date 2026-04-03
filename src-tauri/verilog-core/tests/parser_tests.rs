use verilog_core::parse_file;

#[test]
fn parses_module_name_and_ports_with_directions_and_ranges() {
    let src = r#"
module foo #(parameter WIDTH = 16) (
    input [WIDTH-1:0] data_in,
    output logic ready,
    inout [3:0] bus
);
endmodule
"#;

    let res = parse_file("foo.v", src);
    assert!(
        res.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        res.diagnostics
    );

    assert_eq!(res.modules.len(), 1);
    let m = &res.modules[0];
    assert_eq!(m.name, "foo");
    assert_eq!(m.ports.len(), 3);

    assert_eq!(m.ports[0].direction.as_deref(), Some("input"));
    assert_eq!(m.ports[0].name, "data_in");

    assert_eq!(m.ports[1].direction.as_deref(), Some("output"));
    assert_eq!(m.ports[1].name, "ready");

    assert_eq!(m.ports[2].direction.as_deref(), Some("inout"));
    assert_eq!(m.ports[2].name, "bus");
}


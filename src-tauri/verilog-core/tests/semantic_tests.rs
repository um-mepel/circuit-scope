use std::fs;
use std::path::PathBuf;

use verilog_core::analyze_project;

fn write_temp_project(files: &[(&str, &str)]) -> PathBuf {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, contents) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(&path, contents).expect("write file");
    }
    dir.into_path()
}

#[test]
fn finds_top_module_and_instances() {
    let root = write_temp_project(&[
        (
            "child.v",
            r#"
module child(input a, output b);
  wire w;
  assign b = a;
endmodule
"#,
        ),
        (
            "top.v",
            r#"
module top(input x, output y);
  wire w;
  child u_child(.a(x), .b(y));
endmodule
"#,
        ),
    ]);

    let project = analyze_project(&root).expect("analyze_project");
    assert!(
        project.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        project.diagnostics
    );

    // Expect two modules.
    assert_eq!(project.modules.len(), 2);

    // top should be identified as a top-level module.
    assert!(
        project.top_modules.contains(&"top".to_string()),
        "expected 'top' to be a top module, got {:?}",
        project.top_modules
    );
    assert!(
        !project.top_modules.contains(&"child".to_string()),
        "child should not be a top module"
    );

    // Check that top has an instance of child.
    let top = project
        .modules
        .iter()
        .find(|m| m.name == "top")
        .expect("top module");
    assert_eq!(top.instances.len(), 1);
    assert_eq!(top.instances[0].module_name, "child");
}


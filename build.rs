use std::{env, path::PathBuf, process::Command};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    for name in [
        "home",
        "window",
        "link-card",
        "message",
        "authentication",
        "error",
    ] {
        let input = format!("data/{name}.blp");
        println!("cargo:rerun-if-changed={input}");
        let status = Command::new("blueprint-compiler")
            .arg("compile")
            .arg("--output")
            .arg(out.join(format!("{name}.ui")))
            .arg(&input)
            .status()
            .expect("Install blueprint-compiler to build the GTK interface");
        assert!(status.success(), "Blueprint compilation failed: {input}");
    }
}

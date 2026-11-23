use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    let target = env::var("TARGET").expect("Cargo target");
    if !target.contains("windows") || env::var_os("CARGO_FEATURE_SERVER").is_none() {
        return;
    }
    let source = "tools/server.rc";
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed={source}");
    println!("cargo:rerun-if-env-changed=WINDRES");
    let version = env!("CARGO_PKG_VERSION");
    let numeric = version.replace('.', ",") + ",0";
    let resource = fs::read_to_string(source).expect("Windows version resource");
    for field in [
        format!("FILEVERSION {numeric}"),
        format!("PRODUCTVERSION {numeric}"),
        format!(r#""FileVersion", "{version}""#),
        format!(r#""ProductVersion", "{version}""#),
    ] {
        assert!(resource.contains(&field), "Update {source} for {version}");
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo build directory"));
    let (object, result) = if target.ends_with("gnu") {
        let object = out.join("server-resource.o");
        let tool = env::var("WINDRES").unwrap_or_else(|_| {
            if cfg!(windows) {
                "windres".into()
            } else {
                "x86_64-w64-mingw32-windres".into()
            }
        });
        let status = Command::new(tool)
            .args(["-I.", "-i", source, "-O", "coff", "-o"])
            .arg(&object)
            .status();
        (object, status)
    } else {
        let object = out.join("server-resource.res");
        let status = Command::new("rc.exe")
            .arg("/nologo")
            .arg(format!("/fo{}", object.display()))
            .arg(source)
            .status();
        (object, status)
    };
    match result {
        Ok(status) if status.success() => println!("cargo:rustc-link-arg-bin=frontline-server={}",object.display()),
        _ => println!("cargo:warning=Windows resource compiler unavailable, building server without file metadata."),
    }
}

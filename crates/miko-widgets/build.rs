use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=native/gamepad.swift");
    if std::env::var("CARGO_CFG_TARGET_OS")?.as_str() != "macos" {
        return Ok(());
    }
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH")?.as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => return Err(format!("unsupported macOS gamepad architecture: {other}").into()),
    };
    let directory = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    let out = directory.join("miko-gamepad-native");
    let status = Command::new("xcrun")
        .args([
            "swiftc",
            "-target",
            &format!("{arch}-apple-macosx13.0"),
            "-module-cache-path",
        ])
        .arg(directory.join("swift-module-cache"))
        .args(["native/gamepad.swift", "-o"])
        .arg(out)
        .status()?;
    if !status.success() {
        return Err("compiling the native gamepad helper failed".into());
    }
    Ok(())
}

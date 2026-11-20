//! Embeds the Windows resources (icon + version information) when a resource
//! compiler is available.
//!
//! pdf-rust has no crate dependencies, so this script does not pull in `embed-resource`
//! or `winres`; it shells out to `windres` (GNU toolchains) or `rc.exe` plus `cvtres`
//! (MSVC). If neither is found the build continues, the binary simply has no icon.
//!
//! `PDFRUST_RESOURCE_COMPILER` can point at a specific `windres`/`rc` executable.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/pdf-rust.rc");
    println!("cargo:rerun-if-changed=packaging/windows/pdf-rust.exe.manifest");
    println!("cargo:rerun-if-changed=packaging/windows/pdf-rust.ico");
    println!("cargo:rerun-if-env-changed=PDFRUST_RESOURCE_COMPILER");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    match env::var("PDFRUST_RESOURCE_COMPILER") {
        Ok(tool) => {
            let tool = PathBuf::from(tool);
            let name = tool.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let ok = if name.eq_ignore_ascii_case("rc") {
                compile_msvc(&tool, &out)
            } else {
                compile_gnu(&tool, &out)
            };
            if !ok {
                println!("cargo:warning=pdf-rust: resource compiler failed, building without icon");
            }
        }
        Err(_) => {
            if let Some(tool) = which("windres") {
                if !compile_gnu(&tool, &out) {
                    println!("cargo:warning=pdf-rust: windres failed, building without icon");
                }
            } else if let Some(tool) = which("rc") {
                if !compile_msvc(&tool, &out) {
                    println!("cargo:warning=pdf-rust: rc.exe failed, building without icon");
                }
            } else {
                println!(
                    "cargo:warning=pdf-rust: no rc.exe/windres found; the .exe has no icon"
                );
            }
        }
    }
}

fn compile_gnu(tool: &Path, out: &Path) -> bool {
    let target = env::var("TARGET").unwrap_or_default();
    let obj = out.join("pdf-rust-res.o");
    let status = Command::new(tool)
        .arg("--input")
        .arg("packaging/windows/pdf-rust.rc")
        .arg("--output")
        .arg(&obj)
        .arg("--output-format=coff")
        .arg("-I")
        .arg("packaging/windows")
        .args(["--define", &format!("PDFRUST_TARGET=\"{}\"", target)])
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("cargo:rustc-link-arg={}", obj.display());
            true
        }
        _ => false,
    }
}

fn compile_msvc(tool: &Path, out: &Path) -> bool {
    let target = env::var("TARGET").unwrap_or_default();
    let machine = if target.starts_with("x86_64") {
        "X64"
    } else if target.starts_with("aarch64") {
        "ARM64"
    } else {
        "X86"
    };
    let res = out.join("pdf-rust-res.res");
    let obj = out.join("pdf-rust-res.obj");
    let ok = Command::new(tool)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res)
        .arg("/I")
        .arg("packaging/windows")
        .arg("packaging/windows/pdf-rust.rc")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return false;
    }
    let cvtres = match which("cvtres") {
        Some(path) => path,
        None => return false,
    };
    let ok = Command::new(cvtres)
        .arg(format!("/MACHINE:{machine}"))
        .arg("/NOLOGO")
        .arg(format!("/OUT:{}", obj.display()))
        .arg(&res)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg={}", obj.display());
    }
    ok
}

fn which(name: &str) -> Option<PathBuf> {
    let pathext = env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".into());
    let exts: Vec<String> = std::iter::once(String::new())
        .chain(pathext.split(';').map(|e| e.to_string()))
        .collect();
    for dir in env::split_paths(&env::var("PATH").ok()?) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

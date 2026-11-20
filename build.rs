//! Embeds the Windows resources (icon + version information) when a resource
//! compiler is available.
//!
//! pdf-rust has no crate dependencies, so this script does not pull in `embed-resource`
//! or `winres`; it shells out to `windres` (GNU toolchains) or `rc.exe` plus `cvtres`
//! (MSVC). If neither is found the build continues, the binary simply has no icon.
//!
//! `PDFRUST_RESOURCE_COMPILER` can point at a specific `windres`/`rc` executable.
//!
//! `rc.exe` ships with the Windows SDK and is not on `PATH` unless the build runs
//! from a "Developer Command Prompt", so the script looks in the standard
//! Windows Kits locations (and next to the MSVC linker) before giving up.

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
                if compile_gnu(&tool, &out) {
                    println!("cargo:warning=pdf-rust: resources embedded with windres");
                } else {
                    println!("cargo:warning=pdf-rust: windres failed, building without icon");
                }
            } else if let Some(tool) = find_rc() {
                if compile_msvc(&tool, &out) {
                    println!(
                        "cargo:warning=pdf-rust: resources embedded with {}",
                        tool.display()
                    );
                } else {
                    println!(
                        "cargo:warning=pdf-rust: rc.exe ({}) failed, building without icon",
                        tool.display()
                    );
                }
            } else {
                println!(
                    "cargo:warning=pdf-rust: no rc.exe/windres found; the .exe has no icon \
                     (install the Windows SDK or set PDFRUST_RESOURCE_COMPILER)"
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
    let cvtres = match find_cvtres() {
        Some(path) => path,
        None => {
            println!("cargo:warning=pdf-rust: cvtres.exe not found");
            return false;
        }
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

/// Architecture sub-directory used by the Windows SDK and MSVC toolchains.
fn arch_dir() -> &'static str {
    match env::var("TARGET").unwrap_or_default().as_str() {
        t if t.starts_with("x86_64") => "x64",
        t if t.starts_with("aarch64") => "arm64",
        _ => "x86",
    }
}

/// Highest version directory under `bin/`, e.g. `10.0.22621.0`.
fn newest_version_dir(bin: &Path) -> Option<PathBuf> {
    let mut versions: Vec<PathBuf> = std::fs::read_dir(bin)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.file_name().and_then(|n| n.to_str()).is_some())
        .collect();
    versions.sort_by(|a, b| version_key(a.as_path()).cmp(&version_key(b.as_path())));
    versions.pop()
}

fn version_key(path: &Path) -> Vec<u32> {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// Locates `rc.exe`: `PATH`, then the Windows SDK bin directories, then the MSVC
/// toolchain that `link.exe` came from.
fn find_rc() -> Option<PathBuf> {
    if let Some(tool) = which("rc") {
        return Some(tool);
    }
    // %WindowsSdkDir%in\<version>\<arch>c.exe
    if let Ok(sdk) = env::var("WindowsSdkDir") {
        let bin = Path::new(&sdk).join("bin");
        if let Ok(version) = env::var("WindowsSDKVersion") {
            let candidate = bin
                .join(version.trim_end_matches('\\'))
                .join(arch_dir())
                .join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if let Some(newest) = newest_version_dir(&bin) {
            let candidate = newest.join(arch_dir()).join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // ...\Windows Kitsin\<version>\<arch>c.exe
    for root in ["ProgramFiles(x86)", "ProgramFiles"] {
        let Ok(base) = env::var(root) else { continue };
        let bin = Path::new(&base).join("Windows Kits").join("10").join("bin");
        if let Some(newest) = newest_version_dir(&bin) {
            let candidate = newest.join(arch_dir()).join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Locates `cvtres.exe`, which turns the `.res` file into a COFF object.
fn find_cvtres() -> Option<PathBuf> {
    if let Some(tool) = which("cvtres") {
        return Some(tool);
    }
    // The MSVC toolchain ships cvtres.exe next to link.exe.
    if let Some(link) = which("link").or_else(|| which("link.exe")) {
        let candidate = link.with_file_name("cvtres.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Ok(vc) = env::var("VCToolsInstallDir") {
        let host = if cfg!(target_arch = "aarch64") {
            "Hostarm64"
        } else {
            "Hostx64"
        };
        let candidate = Path::new(&vc)
            .join("bin")
            .join(host)
            .join(arch_dir())
            .join("cvtres.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    for root in ["ProgramFiles", "ProgramFiles(x86)"] {
        let Ok(base) = env::var(root) else { continue };
        let msvc = Path::new(&base)
            .join("Microsoft Visual Studio")
            .join("2022")
            .join("BuildTools")
            .join("VC")
            .join("Tools")
            .join("MSVC");
        let mut installs: Vec<PathBuf> = std::fs::read_dir(&msvc)
            .map(|dir| dir.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        installs.sort_by(|a, b| version_key(a.as_path()).cmp(&version_key(b.as_path())));
        for install in installs.into_iter().rev() {
            for host in ["Hostx64", "Hostarm64", "Hostx86"] {
                let candidate = install.join("bin").join(host).join(arch_dir()).join("cvtres.exe");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn which(name: &str) -> Option<PathBuf> {
    if name.trim().is_empty() {
        return None;
    }
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

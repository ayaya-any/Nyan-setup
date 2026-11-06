use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_rc_exe() -> Option<PathBuf> {
    // 1. Check if rc.exe is in PATH
    if let Ok(out) = Command::new("rc.exe").arg("/?").output() {
        if out.status.success() || !out.stdout.is_empty() || !out.stderr.is_empty() {
            return Some(PathBuf::from("rc.exe"));
        }
    }

    // 2. Scan Windows Kits 10 bin directories
    let kits_bin = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if kits_bin.is_dir() {
        if let Ok(entries) = fs::read_dir(kits_bin) {
            let mut versions: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.is_dir()
                        && p.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .starts_with("10.")
                })
                .collect();
            versions.sort_by(|a, b| b.cmp(a));
            for v in versions {
                let x64_rc = v.join("x64").join("rc.exe");
                if x64_rc.is_file() {
                    return Some(x64_rc);
                }
                let x86_rc = v.join("x86").join("rc.exe");
                if x86_rc.is_file() {
                    return Some(x86_rc);
                }
            }
        }
    }

    None
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.cargo/config.toml");
    println!("cargo:rerun-if-changed=resources");

    let target_features = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    if !target_features.split(',').any(|f| f == "crt-static") {
        println!(
            "cargo:warning=Static CRT (crt-static) is NOT detected in CARGO_CFG_TARGET_FEATURE ({}). Make sure .cargo/config.toml has rustflags = [\"-C\", \"target-feature=+crt-static\"].",
            target_features
        );
    } else {
        println!("cargo:rustc-env=NYAN_CRT_LINKAGE=static (+crt-static)");
    }

    // Embed application icon (.ico) into Windows PE binary if building for Windows
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        let rc_file = Path::new("resources/app.rc");
        let ico_file = Path::new("resources/app.ico");
        if rc_file.exists() && ico_file.exists() {
            if let Some(rc_exe) = find_rc_exe() {
                let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
                let res_file = Path::new(&out_dir).join("app.res");
                // Run inside `resources/` so the .rc can reference the .ico by bare file name.
                let status = Command::new(&rc_exe)
                    .current_dir("resources")
                    .arg("/fo")
                    .arg(&res_file)
                    .arg("app.rc")
                    .status();
                if let Ok(st) = status {
                    if st.success() && res_file.exists() {
                        println!("cargo:rustc-link-arg={}", res_file.display());
                    }
                }
            }
        }
    }
}

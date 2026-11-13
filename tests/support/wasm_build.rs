//! Compile the core guest SDK and a real module without cargo-test lock reentry.
use std::{
    collections::BTreeSet,
    env,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

fn artifact_directories(path: &Path, directories: &mut BTreeSet<PathBuf>) {
    for entry in std::fs::read_dir(path).expect("SDK artifact directory") {
        let entry = entry.expect("SDK artifact entry");
        if entry.file_type().unwrap().is_dir() {
            artifact_directories(&entry.path(), directories);
        } else if entry.path().extension().is_some_and(|extension| {
            matches!(extension.to_str(), Some("rlib" | "so" | "dylib" | "dll"))
        }) {
            directories.insert(path.to_owned());
        }
    }
}

pub fn guest() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let target = root.join("target/wasm-test-fixtures");
        let result = Command::new(env!("CARGO"))
            .args([
                "build",
                "--locked",
                "--offline",
                "--release",
                "-p",
                "zharness",
                "--lib",
                "--no-default-features",
                "--target=wasm32-unknown-unknown",
                "--target-dir",
            ])
            .arg(&target)
            .current_dir(&root)
            .output()
            .expect("start cargo for independent core guest SDK");
        assert!(
            result.status.success(),
            "Wasm SDK build failed; install wasm32-unknown-unknown for this toolchain:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let sdk = target.join("wasm32-unknown-unknown/release");
        let module = target.join("zharness-fixture.wasm");
        let mut compiler = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
        compiler
            .args([
                "--edition=2024",
                "--crate-type=cdylib",
                "--crate-name=zharness_wasm_fixture",
                "--target=wasm32-unknown-unknown",
                "-Copt-level=3",
                "-Cpanic=abort",
                "-Ccodegen-units=1",
                "-Clto=thin",
                "-Clink-arg=-zstack-size=65536",
                "--extern",
            ])
            .arg(format!(
                "zharness={}",
                sdk.join("libzharness.rlib").display()
            ))
            .arg(root.join("examples/wasm/guest.rs"))
            .arg("-o")
            .arg(&module)
            .current_dir(root);
        let mut directories = BTreeSet::new();
        artifact_directories(&target, &mut directories);
        for directory in directories {
            compiler
                .arg("-L")
                .arg(format!("dependency={}", directory.display()));
        }
        let result = compiler
            .output()
            .expect("start rustc for independent guest");
        assert!(
            result.status.success(),
            "independent Wasm fixture build failed:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        std::fs::read(module).expect("compiled Wasm fixture")
    })
}

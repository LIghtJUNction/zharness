//! Build a real cdylib in a separate target tree to avoid cargo-test lock reentry.
use std::{env, path::PathBuf, process::Command};
use zharness::zcall::v1::{Json, Limits, TokenStorage};

pub fn build_example(name: &str) -> PathBuf {
    assert!(matches!(name, "native_plugin" | "plugin_fixture"));
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target = root.join("target/plugin-fixtures");
    let result = Command::new(env!("CARGO"))
        .args([
            "build",
            "--locked",
            "--offline",
            "--release",
            "--message-format=json",
            "--example",
            name,
            "--target-dir",
        ])
        .arg(target)
        .current_dir(root)
        .output()
        .expect("start cargo for independent native fixture");
    assert!(
        result.status.success(),
        "native fixture build failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8(result.stdout).expect("Cargo emits UTF-8 JSON");
    let identity = format!("\"name\":\"{name}\"");
    for line in stdout.lines() {
        if !line.contains("\"reason\":\"compiler-artifact\"") || !line.contains(&identity) {
            continue;
        }
        let Some((_, filenames)) = line.split_once("\"filenames\":") else {
            continue;
        };
        let mut filenames = filenames.trim_start().strip_prefix('[').unwrap();
        // Cargo's filenames array contains only JSON strings. Decode each string
        // with the existing parser so Windows escapes and Unicode paths survive.
        loop {
            filenames = filenames.trim_start_matches([',', ' ', '\t']);
            if filenames.starts_with(']') {
                break;
            }
            assert!(filenames.starts_with('"'));
            let mut escaped = false;
            let end = filenames.as_bytes()[1..]
                .iter()
                .position(|&byte| {
                    if escaped {
                        escaped = false;
                        false
                    } else if byte == b'\\' {
                        escaped = true;
                        false
                    } else {
                        byte == b'"'
                    }
                })
                .expect("closed artifact filename")
                + 2;
            let value = Json::parse(
                &filenames.as_bytes()[..end],
                &mut TokenStorage::<1>::new(),
                Limits::default(),
            )
            .expect("valid Cargo filename")
            .as_text()
            .expect("artifact filename string");
            let mut decoded = vec![0; value.decoded_len()];
            value.decode_into(&mut decoded).unwrap();
            let path = PathBuf::from(String::from_utf8(decoded).unwrap());
            if path.extension().is_some_and(|extension| {
                extension == env::consts::DLL_SUFFIX.trim_start_matches('.')
            }) {
                return path;
            }
            filenames = &filenames[end..];
        }
    }
    panic!("Cargo did not report a dynamic library for {name}");
}

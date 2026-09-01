//! End-to-end validation through the benchmark CLI boundary.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn benchmark_cli_exposes_stable_modes_and_checks_memory_artifact()
-> Result<(), Box<dyn std::error::Error>> {
    let help = Command::new(env!("CARGO_BIN_EXE_scry-benchmarks"))
        .arg("--help")
        .output()?;
    assert!(help.status.success());
    let help_text = String::from_utf8(help.stdout)?;
    assert!(help_text.contains("--add-only"));
    assert!(help_text.contains("--scale"));
    assert!(help_text.contains("--memory"));

    let invalid = Command::new(env!("CARGO_BIN_EXE_scry-benchmarks"))
        .args(["--scale", "--microbatch-size", "257"])
        .output()?;
    assert!(!invalid.status.success());
    let invalid_text = String::from_utf8(invalid.stderr)?;
    assert!(invalid_text.contains("microbatch-size must be at most 256"));

    let cache = env::var_os("SCRY_MODEL_CACHE")
        .map_or_else(|| env::temp_dir().join("scry-model-cache"), PathBuf::from);
    let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("benchmarks")
        .join("memory-v1.json");
    let before = fs::read(&artifact)?;
    let checked = Command::new(env!("CARGO_BIN_EXE_scry-benchmarks"))
        .args(["--cache"])
        .arg(&cache)
        .args(["--memory-check"])
        .arg(&artifact)
        .output()?;
    assert!(checked.status.success());
    let checked_text = String::from_utf8(checked.stdout)?;
    assert!(checked_text.contains("memory baseline matches"));
    assert_eq!(fs::read(&artifact)?, before);
    Ok(())
}

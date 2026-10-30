//! Exercises real CLI driver refusals without running Cargo or a compiler.

use assert_cmd::cargo::cargo_bin_cmd;
use dylint_internal::env;
use std::{env::consts, fs, path::Path};
use tempfile::{Builder, TempDir, tempdir, tempdir_in};

#[test]
fn missing_driver_refusal_reaches_get_without_starting_a_build() -> std::io::Result<()> {
    let tempdir = tempdir()?;
    let library = create_library(&tempdir)?;
    let empty_path = tempdir_in(&tempdir)?;
    let rustup_home = tempdir_in(&tempdir)?;
    let driver = driver_cache_path(tempdir.path());

    assert_refusal(
        list_command(&tempdir, &empty_path, &rustup_home, library.path(), false),
        "missing",
        &driver,
    )?;

    assert!(!driver.exists(), "refusal must not create a driver");
    assert_no_tool_provisioned(&empty_path)?;
    Ok(())
}

#[test]
fn quiet_unverifiable_cached_driver_refusal_preserves_the_probe_cause() -> std::io::Result<()> {
    let tempdir = tempdir()?;
    let library = create_library(&tempdir)?;
    let empty_path = tempdir_in(&tempdir)?;
    let rustup_home = tempdir_in(&tempdir)?;
    let driver = create_cached_driver(&tempdir, b"not an executable")?;
    let original_contents = fs::read(&driver)?;

    let stderr = assert_refusal(
        list_command(&tempdir, &empty_path, &rustup_home, library.path(), true),
        "unverifiable",
        &driver,
    )?;
    assert!(
        stderr.contains("Could not get output"),
        "quiet refusal must expose the original probe cause: {stderr}"
    );

    assert_eq!(original_contents, fs::read(&driver)?);
    assert_no_tool_provisioned(&empty_path)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn outdated_cached_driver_refusal_does_not_replace_the_driver() -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let tempdir = tempdir()?;
    let library = create_library(&tempdir)?;
    let empty_path = tempdir_in(&tempdir)?;
    let rustup_home = tempdir_in(&tempdir)?;
    let driver_contents = b"#!/bin/sh\nprintf 'dylint-driver 0.0.0\\n'\n";
    let driver = create_cached_driver(&tempdir, driver_contents)?;
    let mut permissions = fs::metadata(&driver)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&driver, permissions)?;

    assert_refusal(
        list_command(&tempdir, &empty_path, &rustup_home, library.path(), false),
        "outdated",
        &driver,
    )?;

    assert_eq!(driver_contents, fs::read(&driver)?.as_slice());
    assert_no_tool_provisioned(&empty_path)?;
    Ok(())
}

fn create_library(tempdir: &TempDir) -> std::io::Result<tempfile::NamedTempFile> {
    Builder::new()
        .prefix(&format!("{}refusal", consts::DLL_PREFIX))
        .suffix(&format!("@no-driver-build-test{}", consts::DLL_SUFFIX))
        .tempfile_in(tempdir)
}

fn create_cached_driver(tempdir: &TempDir, contents: &[u8]) -> std::io::Result<std::path::PathBuf> {
    let driver = driver_cache_path(tempdir.path());
    let parent = driver.parent().expect("driver path has a parent");
    fs::create_dir_all(parent)?;
    fs::write(&driver, contents)?;
    Ok(driver)
}

fn driver_cache_path(cache_root: &Path) -> std::path::PathBuf {
    cache_root.join("no-driver-build-test/dylint-driver")
}

fn list_command(
    tempdir: &TempDir,
    empty_path: &TempDir,
    rustup_home: &TempDir,
    library: &Path,
    quiet: bool,
) -> assert_cmd::Command {
    let mut command = cargo_bin_cmd!("cargo-dylint");
    command
        .current_dir(tempdir)
        .env(env::DYLINT_DRIVER_PATH, tempdir.path())
        .env("CARGO", empty_path.path().join("cargo"))
        .env("PATH", empty_path.path())
        .env("RUSTUP_HOME", rustup_home.path())
        .args(["dylint", "list"]);
    if quiet {
        command.arg("--quiet");
    }
    command
        .args(["--no-driver-build", "--no-metadata", "--lib-path"])
        .arg(library);
    command
}

fn assert_refusal(
    mut command: assert_cmd::Command,
    status: &str,
    driver: &Path,
) -> std::io::Result<String> {
    let output = command.output()?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!output.status.success(), "refusal should exit with failure");
    assert!(stderr.contains("Compiler driver"), "{stderr}");
    assert!(stderr.contains(status), "{stderr}");
    assert!(stderr.contains(&driver.display().to_string()), "{stderr}");
    assert!(
        stderr.contains("--no-driver-build requires a prebuilt compatible driver"),
        "{stderr}"
    );
    Ok(stderr)
}

fn assert_no_tool_provisioned(empty_path: &TempDir) -> std::io::Result<()> {
    let entries = fs::read_dir(empty_path)?;
    assert_eq!(
        0,
        entries.count(),
        "no tool should be provisioned in the isolated executable directory",
    );
    Ok(())
}

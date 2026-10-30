//! Exercises real CLI driver refusals without running Cargo or a compiler.

use assert_cmd::cargo::cargo_bin_cmd;
use dylint_internal::env;
use std::{
    env::consts,
    fs,
    path::{Path, PathBuf},
};
use tempfile::{Builder, NamedTempFile, TempDir, tempdir, tempdir_in};

/// Check that a missing cached driver fails before any tool is provisioned.
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

// leynos: All file mutations are confined to this test's fresh TempDir.
/// Check that quiet mode reports the original failure to probe an unusable cached driver.
#[cfg_attr(dylint_lib = "general", allow(non_thread_safe_call_in_test))]
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

    let current_contents = fs::read(&driver)?;
    assert_eq!(original_contents, current_contents);
    assert_no_tool_provisioned(&empty_path)?;
    Ok(())
}

// leynos: All file mutations are confined to this test's fresh TempDir.
/// Check that refusing an outdated driver leaves the cached file intact.
#[cfg(unix)]
#[cfg_attr(dylint_lib = "general", allow(non_thread_safe_call_in_test))]
#[test]
fn outdated_cached_driver_refusal_does_not_replace_the_driver() -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    const EXECUTABLE_DRIVER_MODE: u32 = 0o755;

    let tempdir = tempdir()?;
    let library = create_library(&tempdir)?;
    let empty_path = tempdir_in(&tempdir)?;
    let rustup_home = tempdir_in(&tempdir)?;
    let driver_contents = b"#!/bin/sh\nprintf 'dylint-driver 0.0.0\\n'\n";
    let driver = create_cached_driver(&tempdir, driver_contents)?;
    let metadata = fs::metadata(&driver)?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(EXECUTABLE_DRIVER_MODE);
    fs::set_permissions(&driver, permissions)?;

    assert_refusal(
        list_command(&tempdir, &empty_path, &rustup_home, library.path(), false),
        "outdated",
        &driver,
    )?;

    let current_contents = fs::read(&driver)?;
    assert_eq!(driver_contents, current_contents.as_slice());
    assert_no_tool_provisioned(&empty_path)?;
    Ok(())
}

fn create_library(tempdir: &TempDir) -> std::io::Result<NamedTempFile> {
    Builder::new()
        .prefix(&format!("{}refusal", consts::DLL_PREFIX))
        .suffix(&format!("@no-driver-build-test{}", consts::DLL_SUFFIX))
        .tempfile_in(tempdir)
}

/// Write a test driver into the isolated driver's cache and return its path.
fn create_cached_driver(tempdir: &TempDir, contents: &[u8]) -> std::io::Result<PathBuf> {
    let driver = driver_cache_path(tempdir.path());
    let parent = driver.parent().expect("driver path has a parent");
    fs::create_dir_all(parent)?;
    fs::write(&driver, contents)?;
    Ok(driver)
}

fn driver_cache_path(cache_root: &Path) -> PathBuf {
    cache_root.join("no-driver-build-test/dylint-driver")
}

/// Build a `cargo dylint list` command isolated from host tools and driver caches.
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

/// Run a command and check that driver refusal names the status, path, and cache requirement.
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

/// Check that the isolated executable directory remains empty after refusal.
fn assert_no_tool_provisioned(empty_path: &TempDir) -> std::io::Result<()> {
    let entries = fs::read_dir(empty_path)?;
    assert_eq!(
        0,
        entries.count(),
        "no tool should be provisioned in the isolated executable directory",
    );
    Ok(())
}

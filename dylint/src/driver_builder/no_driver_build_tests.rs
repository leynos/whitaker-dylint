//! Driver-build refusal tests that never run a compiler or cached driver.

use super::*;
use anyhow::Error;
use std::cell::Cell;

/// Check that refusing absent, outdated, or unverifiable drivers skips the build closure.
#[test]
fn refuses_missing_outdated_and_unverifiable_drivers_without_building() {
    for (status, reason) in [
        (Ok(DriverStatus::Missing), "missing"),
        (Ok(DriverStatus::Outdated), "outdated"),
        (Err(anyhow!("version query failed")), "unverifiable"),
    ] {
        let built = Cell::new(false);
        let opts = opts::Dylint {
            no_driver_build: true,
            ..Default::default()
        };
        let result = ensure_driver(&opts, Path::new("cached-driver"), status, || {
            built.set(true);
            Ok(())
        });
        assert!(!built.get(), "the {reason} driver must not be built");
        let message = result
            .err()
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        assert!(message.contains(reason), "{message}");
        assert!(message.contains("cached-driver"), "{message}");
        assert!(message.contains("--no-driver-build"), "{message}");
    }
}

/// Check that the default policy builds when the cached driver cannot be used.
#[test]
fn defaults_still_build_missing_outdated_and_unverifiable_drivers() {
    for status in [
        Ok(DriverStatus::Missing),
        Ok(DriverStatus::Outdated),
        Err(anyhow!("version query failed")),
    ] {
        let built = Cell::new(false);
        let result = ensure_driver(
            &opts::Dylint::default(),
            Path::new("cached-driver"),
            status,
            || {
                built.set(true);
                Ok(())
            },
        );
        assert!(
            result.is_ok(),
            "default behavior should rebuild the driver: {result:?}"
        );
        assert!(
            built.get(),
            "default behavior must invoke the build operation"
        );
    }
}

/// Check that a current cached driver is reused under either build policy.
#[test]
fn current_driver_is_reused_with_or_without_refusal() {
    for no_driver_build in [false, true] {
        let opts = opts::Dylint {
            no_driver_build,
            ..Default::default()
        };
        let built = Cell::new(false);
        let result = ensure_driver(
            &opts,
            Path::new("cached-driver"),
            Ok(DriverStatus::Current),
            || {
                built.set(true);
                Ok(())
            },
        );
        assert!(
            result.is_ok(),
            "a current driver must remain usable: {result:?}"
        );
        assert!(!built.get(), "a current driver must not be rebuilt");
    }
}

#[test]
fn default_mode_propagates_build_failures() {
    let result = ensure_driver(
        &opts::Dylint::default(),
        Path::new("cached-driver"),
        Ok(DriverStatus::Missing),
        || Err(anyhow!("build failed")),
    );
    assert_eq!(
        Some("build failed"),
        result.err().as_ref().map(ToString::to_string).as_deref()
    );
}

/// Check that a quiet refusal retains the probe error and never starts a build.
#[test]
fn quiet_unverifiable_refusal_preserves_the_probe_error_and_skips_build() -> Result<()> {
    let opts = opts::Dylint {
        no_driver_build: true,
        quiet: true,
        ..Default::default()
    };
    let original_error = transient_io_error(
        std::io::ErrorKind::PermissionDenied,
        "cache replacement in progress",
    );
    let built = Cell::new(false);
    let error = ensure_driver(
        &opts,
        Path::new("cached-driver"),
        Err(original_error),
        || {
            built.set(true);
            Ok(())
        },
    )
    .err()
    .ok_or_else(|| anyhow!("quiet refusal must still return its probe error"))?;

    assert!(error.to_string().contains("cached-driver"));
    assert!(error.to_string().contains("unverifiable"));
    assert!(error.to_string().contains("--no-driver-build"));
    assert_eq!(
        "cache replacement in progress",
        error.root_cause().to_string()
    );
    assert!(!built.get(), "refusal must not invoke the build operation");
    Ok(())
}

/// Check that transient missing and I/O probe results retry before reusing a current driver.
#[test]
fn transient_missing_and_io_probe_errors_retry_then_reuse_current_driver() -> Result<()> {
    for first_probe in [
        Ok(DriverStatus::Missing),
        Err(transient_io_error(
            std::io::ErrorKind::NotFound,
            "driver cache entry not present during replacement",
        )),
        Err(transient_io_error(
            std::io::ErrorKind::PermissionDenied,
            "driver cache entry locked during replacement",
        )),
    ] {
        assert_transient_probe_retries_and_reuses_current(first_probe)?;
    }
    Ok(())
}

/// Check that current, outdated, and non-I/O probe results are returned without retrying.
#[test]
fn current_outdated_and_non_io_probe_errors_are_not_retried() -> Result<()> {
    for (response, expected) in [
        (Ok(DriverStatus::Current), "current"),
        (Ok(DriverStatus::Outdated), "outdated"),
    ] {
        let calls = Cell::new(0);
        let mut response = Some(response);
        let result = retry_transient_driver_probe(|| -> Result<DriverStatus> {
            calls.set(calls.get() + 1);
            response
                .take()
                .ok_or_else(|| anyhow!("non-retryable results must probe only once"))?
        });

        assert_eq!(1, calls.get());
        assert!(
            matches!(
                (expected, &result),
                ("current", Ok(DriverStatus::Current)) | ("outdated", Ok(DriverStatus::Outdated))
            ),
            "unexpected probe result: {result:?}"
        );
    }

    let calls = Cell::new(0);
    let mut response = Some(Err(anyhow!("Could not parse driver version `invalid`")));
    let result = retry_transient_driver_probe(|| -> Result<DriverStatus> {
        calls.set(calls.get() + 1);
        response
            .take()
            .ok_or_else(|| anyhow!("non-retryable errors must probe only once"))?
    });
    let error = result
        .err()
        .ok_or_else(|| anyhow!("parse errors must propagate"))?;
    assert_eq!(1, calls.get());
    assert!(error.to_string().contains("Could not parse driver version"));
    Ok(())
}

/// Check that persistent absence stops after three probes and still refuses to build.
#[test]
fn persistent_missing_probe_stops_after_three_attempts_and_refuses_build() -> Result<()> {
    let calls = Cell::new(0);
    let status = retry_transient_driver_probe(|| {
        calls.set(calls.get() + 1);
        Ok(DriverStatus::Missing)
    });
    let built = Cell::new(false);
    let opts = opts::Dylint {
        no_driver_build: true,
        ..Default::default()
    };
    let error = ensure_driver(&opts, Path::new("cached-driver"), status, || {
        built.set(true);
        Ok(())
    })
    .err()
    .ok_or_else(|| anyhow!("persistent missing driver must be refused"))?;

    assert_eq!(3, calls.get());
    assert!(error.to_string().contains("is missing"));
    assert!(error.to_string().contains("--no-driver-build"));
    assert!(!built.get(), "refusal must not invoke the build operation");
    Ok(())
}

/// Retry a transient probe result and verify that the current driver is reused.
fn assert_transient_probe_retries_and_reuses_current(
    first_probe: Result<DriverStatus>,
) -> Result<()> {
    let calls = Cell::new(0);
    let mut responses = [first_probe, Ok(DriverStatus::Current)].into_iter();
    let status = retry_transient_driver_probe(|| -> Result<DriverStatus> {
        calls.set(calls.get() + 1);
        responses
            .next()
            .ok_or_else(|| anyhow!("the transient result must be followed by a current driver"))?
    });
    let built = Cell::new(false);
    let opts = opts::Dylint {
        no_driver_build: true,
        ..Default::default()
    };
    let result = ensure_driver(&opts, Path::new("cached-driver"), status, || {
        built.set(true);
        Ok(())
    });

    assert_eq!(2, calls.get());
    result.context("a current retry result must be reused")?;
    assert!(!built.get(), "reusing a current driver must not build");
    Ok(())
}

fn transient_io_error(kind: std::io::ErrorKind, message: &str) -> Error {
    Error::new(std::io::Error::new(kind, message)).context("driver status probe failed")
}

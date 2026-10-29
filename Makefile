# leynos: Check the stable CLI, library, and testing utility packages here;
# compiler-private workspaces retain their upstream toolchain-specific CI.
CARGO_PACKAGES := -p cargo-dylint -p dylint -p dylint_testing

.PHONY: check-fmt lint typecheck

check-fmt:
	cargo +nightly fmt --all -- --check

lint:
	cargo clippy $(CARGO_PACKAGES) --all-targets --all-features -- -D warnings

typecheck:
	cargo check $(CARGO_PACKAGES) --all-targets --all-features

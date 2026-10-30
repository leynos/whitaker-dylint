WIP

- Please prefix all non-rustdoc comments with your GitHub username.

  Examples:

  ```rust
  /// This is a rustdoc comment, so no username is required.
  fn foo() {}

  // smoelius: This is a non-rustdoc comment.
  fn bar() {}
  ```

  ```sh
  #! /bin/bash
  # smoelius: This is a Bash comment, and therefore non-rustdoc.
  ```

## Local checks

Run `make check-fmt`, `make lint`, and `make typecheck` to check the CLI, Dylint library, and testing utilities. `check-fmt` requires nightly `rustfmt`. Upstream CI covers the compiler-private workspaces, which use toolchain-specific checks.

On Windows, CI creates a detached worktree at `C:/dylint` to keep MSVC import-library paths short. The original checkout is retained, and the cache uses the same short path for build outputs.

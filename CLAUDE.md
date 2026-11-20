# virtuoso

## Tests

- Every change is test-first, red then green: a test that fails for the right reason, then the code that makes it pass.
- Test coverage target: 95% of the repo's source lines.
  - The lines a pull request adds or changes must be at least 95% covered. The `coverage` workflow checks this on every pull request with diff-cover.
  - The repo's line coverage must never fall below the floor in `.github/coverage-floor`. When a change raises coverage, raise the floor to the new figure (to one decimal place, rounded down) in the same pull request. Never lower it.
- Coverage is measured on Linux. It counts the workspace's tests and the real-Chrome web tests (`VIRTUOSO_WEB=1`). The Android and iOS device tests need a device, so they aren't counted. Code that only runs on a device still needs unit tests for the parts that can run without one.
- Measure locally with `cargo llvm-cov --workspace`, plus `VIRTUOSO_WEB=1 cargo llvm-cov -p virtuoso --test device_web` for the web code.

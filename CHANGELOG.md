# Changelog

## [Unreleased]

### Changed
- `egui` updated to version 0.37.
- Minimum Rust version updated to 1.95.
- Bumped `tokio-immediate-egui` to 0.3.0 due to egui 0.37 upgrade (semver-incompatible).

### Changed
- Updated `Plugin` trait implementation for egui 0.35+ (`input_hook`/`output_hook` now take `&Context` parameter).
- Updated `example-imgui-minimal` to use new `dear-app` API (`AppConfig`, `Application` trait, `run` function).

### Fixed
- Fixed various OS/HW-related portability issues in examples.
- Removed `winit` dependencies from `example-egui` and `example-egui-minimal` to avoid version conflicts.

### Added

## [0.2.0] - 2026-02-22

### Added
- Added `AsyncSerialRunner` for immediate execution of single queued async operations.
- Added `AsyncParallelRunner` for immediate execution of async operations in parallel.
- A lot of things renamed.

## [0.1.2] - 2026-02-20

### Added
- Added wrappers for `tokio::sync::broadcast::WeakSender`.
- Added wrappers for `tokio::sync::mpsc::WeakSender` and `tokio::sync::mpsc::WeakUnboundedSender`.
- Added `take()` and `take_current()` for sync `mpsc` receivers.
- Added `take()` and `take_current()` for sync `broadcast` receivers.

## [0.1.1] - 2026-02-19

### Fixed
- Task futures are dropped before `wake_up()` is invoked.

## [0.1.0] - 2026-02-18

### Added
- Initial release.

[Unreleased]: https://github.com/tara-zero/tokio-immediate/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/tara-zero/tokio-immediate/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/tara-zero/tokio-immediate/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/tara-zero/tokio-immediate/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/tara-zero/tokio-immediate/releases/tag/v0.1.0

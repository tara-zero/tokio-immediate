# Changelog

## [Unreleased]

### Added
### Fixed

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

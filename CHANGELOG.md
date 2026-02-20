# Changelog

## [Unreleased]

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

[Unreleased]: https://github.com/tara-zero/tokio-immediate/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/tara-zero/tokio-immediate/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/tara-zero/tokio-immediate/releases/tag/v0.1.0

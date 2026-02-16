// SPDX-License-Identifier: Apache-2.0 OR MIT

mod waker_binding;
mod waker_registration;

/// A viewport-aware wrapper around [`tokio::sync::broadcast`].
///
/// Works like the standard Tokio broadcast channel, but sending through the
/// [`im_send()`](broadcast::Sender::im_send) method additionally wakes up every
/// viewport that holds a registered [`Receiver`](broadcast::Receiver).
pub mod broadcast;

/// A viewport-aware wrapper around [`tokio::sync::mpsc`].
///
/// Works like the standard Tokio mpsc channel, but successful
/// [`im_send()`](mpsc::Sender::im_send) and
/// [`im_try_send()`](mpsc::Sender::im_try_send) calls additionally wake up the
/// viewport bound to the single [`Receiver`](mpsc::Receiver).
pub mod mpsc;

/// A viewport-aware wrapper around [`tokio::sync::watch`].
///
/// Works like the standard Tokio watch channel, but sending through the
/// `im_send*` methods additionally wakes up every viewport that holds a
/// registered [`Receiver`](watch::Receiver).
pub mod watch;

// TODO: oneshot

// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Primitives for calling asynchronous code from immediate mode GUIs.
//!
//! The [`single`] module contains [`AsyncCall`], which spawns a single
//! [`Future`] onto a Tokio runtime and exposes its result through a
//! poll-based API that fits naturally into an immediate mode update loop.
//! An [`AsyncViewport`] ties a GUI viewport to a wake-up callback so
//! that completed tasks automatically trigger a repaint.
//!
//! With the `sync` feature enabled, the [`sync`] and [`trigger`] modules
//! provide channel wrappers that wake viewports when values are sent,
//! enabling continuous progress reporting from async tasks to the UI.
//! The `parallel` module provides `AsyncParallelRunner`.
//! The `serial` module is enabled by the `sync` feature and provides
//! `AsyncSerialRunner`.
//!
//! ## Feature flags
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(
    feature = "document-features",
    cfg_attr(doc, doc = ::document_features::document_features!())
)]
//
// Clippy lints.
#![warn(clippy::pedantic)]
#![warn(clippy::cargo)]
#![warn(clippy::undocumented_unsafe_blocks)]

use ::std::mem::replace;
use ::std::ops::Deref;
use ::std::panic::resume_unwind;
use ::std::sync::atomic::{AtomicBool, Ordering};
use ::std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};

use ::tokio::runtime::Handle;
use ::tokio::task::{JoinHandle, JoinSet};

/// Re-export `tokio` crate.
pub use ::tokio;

/// Async parallel runner: schedule futures to run concurrently.
pub mod parallel;
/// Async serial runner: schedule futures to run one after another.
#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub mod serial;
/// Async call: spawn one [`Future`] and track its result.
pub mod single;
/// Wrappers around `tokio::sync` primitives that wake up viewports on send.
#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub mod sync;
/// A notification channel for signaling async tasks from the UI thread.
#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub mod trigger;

use parallel::AsyncParallelRunner;
#[cfg(feature = "sync")]
use serial::AsyncSerialRunner;
use single::AsyncCall;

/// Represents a single GUI viewport (window) that can be woken up from
/// asynchronous tasks.
///
/// An `AsyncViewport` owns a wake-up callback (typically one that
/// requests a repaint of the viewport) and hands out [`AsyncWaker`]
/// handles via [`new_waker()`](Self::new_waker).
///
/// The wake-up callback **must not block**, because it may be called from
/// inside an async executor.
#[derive(Clone)]
pub struct AsyncViewport {
    wake_up_requested: Arc<AtomicBool>,
    wake_up: Arc<AsyncWakeUpSlot>,
}

/// A thread-safe collection of [`AsyncWaker`]s that can all be woken up
/// at once.
///
/// This is primarily used by synchronisation primitives (e.g.
/// [`crate::sync::watch`]) that need to notify every viewport observing the same
/// shared value.
#[derive(Clone)]
pub struct AsyncWakerList {
    inner: Arc<RwLock<AsyncWakerListInner>>,
}

struct AsyncWakerListInner {
    wakers: Vec<Option<AsyncWaker>>,
    free: Vec<usize>,
}

/// A lightweight, cloneable handle that can request a repaint of the
/// [`AsyncViewport`] it was created from.
///
/// If the viewport has been dropped, [`AsyncWakeUp::wake_up`]
/// becomes a no-op and returns `false`.
#[derive(Clone)]
pub struct AsyncWaker {
    wake_up_requested: Arc<AtomicBool>,
    wake_up: Weak<AsyncWakeUpSlot>,
}

type AsyncWakeUpSlot = RwLock<Option<AsyncWakeUpCallback>>;
pub type AsyncWakeUpCallback = Arc<dyn Fn() + Send + Sync>;

/// RAII guard that wakes up on drop.
pub struct AsyncWakeUpGuard<W>
where
    W: AsyncWakeUp,
{
    waker: W,
}

/// Common interface for types that can request a viewport wake-up.
pub trait AsyncWakeUp {
    /// Creates a guard that calls [`AsyncWakeUp::wake_up`] when dropped.
    #[must_use]
    fn wake_up_guard(&self) -> AsyncWakeUpGuard<&Self>
    where
        Self: Sized,
    {
        AsyncWakeUpGuard { waker: self }
    }

    #[must_use]
    fn wake_up_guard_owned(self) -> AsyncWakeUpGuard<Self>
    where
        Self: Sized,
    {
        AsyncWakeUpGuard { waker: self }
    }

    /// Requests a wake-up.
    fn wake_up(&self);
}

/// Abstraction over how [`AsyncCall`] accesses a Tokio runtime.
///
/// Implemented for [`AsyncCurrentRuntime`] (thread-local context) and
/// [`Handle`] (explicit handle stored inside `AsyncCall`).
pub trait AsyncRuntime {
    /// Spawns a future onto the runtime, returning a [`JoinHandle`].
    ///
    /// # Panics
    ///
    /// Implementations may panic if their runtime access preconditions are not
    /// met.
    fn spawn<Fut, T>(&mut self, future: Fut) -> JoinHandle<T>
    where
        Fut: 'static + Send + Future<Output = T>,
        T: 'static + Send;

    /// Spawns a future onto the runtime and tracks it in a [`JoinSet`].
    ///
    /// # Panics
    ///
    /// Implementations may panic if their runtime access preconditions are not
    /// met.
    fn spawn_join_set<Fut, T>(&mut self, join_set: &mut JoinSet<T>, future: Fut)
    where
        Fut: 'static + Send + Future<Output = T>,
        T: 'static + Send;

    /// Blocks the current thread until the task completes.
    ///
    /// Returns `Some(value)` on success, or `None` if the task was
    /// cancelled. Re-raises the panic (via [`resume_unwind`]) if the task
    /// panicked.
    ///
    /// # Panics
    ///
    /// Implementations may panic if their runtime access preconditions are not
    /// met.
    ///
    /// Re-raises panics from the joined task in the calling thread.
    fn block_on<T>(&mut self, join_handle: JoinHandle<T>) -> Option<T>
    where
        T: 'static + Send;
}

/// The default [`AsyncRuntime`] for [`AsyncCall`].
///
/// Uses the Tokio runtime entered on the current thread (i.e. the one set
/// up by [`Runtime::enter()`](tokio::runtime::Runtime::enter)). This is
/// convenient when the runtime context is guaranteed to be available, but
/// will panic if it is not (e.g. in a deferred viewport callback running on
/// a different thread). In that case, pass a
/// [`Handle`] explicitly instead.
#[derive(Default)]
pub struct AsyncCurrentRuntime;

impl Default for AsyncViewport {
    fn default() -> Self {
        Self {
            wake_up_requested: Arc::new(AtomicBool::new(false)),
            wake_up: Arc::new(RwLock::new(None)),
        }
    }
}

impl AsyncWakeUp for AsyncViewport {
    fn wake_up(&self) {
        if self
            .wake_up_requested
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            let wake_up = self
                .wake_up
                .read()
                .expect("Failed to read-lock AsyncViewport callback: poisoned by panic")
                .clone();
            if let Some(wake_up) = wake_up {
                (wake_up)();
            }
        }
    }
}

impl AsyncViewport {
    /// Creates a new viewport with the given wake-up callback.
    ///
    /// `wake_up` is called whenever an async task associated with this
    /// viewport finishes or a synchronisation primitive is updated. It
    /// **must not block**.
    #[must_use]
    pub fn new_with_wake_up(wake_up: AsyncWakeUpCallback) -> Self {
        let viewport = Self::default();
        let _ = viewport.replace_wake_up(Some(wake_up));
        viewport
    }

    /// Replaces the viewport wake-up callback, returning the previous one.
    ///
    /// # Panics
    ///
    /// Panics if the callback lock is poisoned by another thread panicking
    /// while holding the write lock.
    #[must_use]
    pub fn replace_wake_up(
        &self,
        wake_up: Option<AsyncWakeUpCallback>,
    ) -> Option<AsyncWakeUpCallback> {
        replace(
            &mut *self
                .wake_up
                .write()
                .expect("Failed to write-lock AsyncViewport callback: poisoned by panic"),
            wake_up,
        )
    }

    /// Creates an [`AsyncCall`] wired to this viewport, using `A::default()`
    /// as the runtime.
    #[must_use]
    pub fn new_call<T, A>(&self) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        AsyncCall::new(self.new_waker())
    }

    /// Creates an [`AsyncCall`] wired to this viewport with an explicit
    /// runtime.
    #[must_use]
    pub fn new_call_with_runtime<T, A>(&self, runtime: A) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        AsyncCall::new_with_runtime(self.new_waker(), runtime)
    }

    /// Creates an [`AsyncSerialRunner`] wired to this viewport, using
    /// `A::default()` as the runtime.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner<T, A>(&self) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        AsyncSerialRunner::new(self.new_waker())
    }

    /// Creates an [`AsyncSerialRunner`] wired to this viewport with an
    /// explicit runtime.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner_with_runtime<T, A>(&self, runtime: A) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        AsyncSerialRunner::new_with_runtime(self.new_waker(), runtime)
    }

    /// Creates an [`AsyncParallelRunner`] wired to this viewport, using
    /// `A::default()` as the runtime.
    #[must_use]
    pub fn new_parallel_runner<T, A>(&self) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        AsyncParallelRunner::new(self.new_waker())
    }

    /// Creates an [`AsyncParallelRunner`] wired to this viewport with an
    /// explicit runtime.
    #[must_use]
    pub fn new_parallel_runner_with_runtime<T, A>(&self, runtime: A) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        AsyncParallelRunner::new_with_runtime(self.new_waker(), runtime)
    }

    /// Creates a new [`AsyncWaker`] that can request a repaint of this
    /// viewport.
    #[must_use]
    pub fn new_waker(&self) -> AsyncWaker {
        AsyncWaker {
            wake_up_requested: self.wake_up_requested.clone(),
            wake_up: Arc::downgrade(&self.wake_up),
        }
    }

    /// Acknowledges that the viewport has been repainted after a wake-up
    /// request, clearing the pending flag.
    ///
    /// Call this at the start of every frame (before polling any
    /// [`AsyncCall`] instances) so that subsequent wake-up requests are not
    /// swallowed.
    pub fn woke_up(&self) {
        self.wake_up_requested.store(false, Ordering::Relaxed);
    }

    /// Returns `true` if `self` and `other` represent the same viewport.
    #[must_use]
    pub fn is_same_viewport(&self, other: &Self) -> bool {
        self.wake_up_requested.as_ptr() == other.wake_up_requested.as_ptr()
    }
}

impl Default for AsyncWakerList {
    fn default() -> Self {
        Self::with_capacity(1)
    }
}

impl AsyncWakeUp for AsyncWakerList {
    fn wake_up(&self) {
        for waker in self.inner().wakers.iter().flatten() {
            waker.wake_up();
        }
    }
}

impl AsyncWakerList {
    /// Creates a new waker list pre-allocated for `capacity` wakers.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(AsyncWakerListInner {
                wakers: Vec::with_capacity(capacity),
                free: Vec::with_capacity(capacity),
            })),
        }
    }

    /// Registers a waker and returns its index.
    ///
    /// The returned index must later be passed to
    /// [`remove_waker()`](Self::remove_waker) exactly once when the waker is
    /// no longer needed.
    #[must_use]
    pub fn add_waker(&self, waker: AsyncWaker) -> usize {
        let mut inner = self.inner_mut();
        if let Some(idx) = inner.free.pop() {
            // SAFETY: We never shrink `wakers`, and `free` contains only indexes
            // previously produced by `add_waker`.
            let place = unsafe { inner.wakers.get_unchecked_mut(idx) };
            *place = Some(waker);
            idx
        } else {
            let idx = inner.wakers.len();
            inner.wakers.push(Some(waker));
            let free_vec_reserve = inner.wakers.capacity() - inner.free.len();
            inner.free.reserve_exact(free_vec_reserve);
            idx
        }
    }

    /// Removes a previously registered waker by index.
    ///
    /// # Safety
    ///
    /// `idx` must be a value previously returned by
    /// [`add_waker()`](Self::add_waker), and this method must be called
    /// **exactly once** per index.
    pub unsafe fn remove_waker(&self, idx: usize) {
        let mut inner = self.inner_mut();
        // SAFETY: `idx` must satisfy this function's safety contract.
        let place = unsafe { inner.wakers.get_unchecked_mut(idx) };
        *place = None;
        inner.free.push(idx);
    }

    fn inner(&'_ self) -> RwLockReadGuard<'_, AsyncWakerListInner> {
        self.inner
            .read()
            .expect("Failed to read-lock AsyncWakerList: poisoned by panic in another thread")
    }

    fn inner_mut(&'_ self) -> RwLockWriteGuard<'_, AsyncWakerListInner> {
        self.inner
            .write()
            .expect("Failed to write-lock AsyncWakerList: poisoned by panic in another thread")
    }
}

impl AsyncWakeUp for AsyncWaker {
    fn wake_up(&self) {
        if self
            .wake_up_requested
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            if let Some(wake_up) = self.wake_up.upgrade() {
                let wake_up = wake_up
                    .read()
                    .expect("Failed to read-lock AsyncWaker callback: poisoned by panic")
                    .clone();
                if let Some(wake_up) = wake_up {
                    (wake_up)();
                }
            } else {
                self.wake_up_requested.store(false, Ordering::Relaxed);
            }
        }
    }
}

impl AsyncWaker {
    /// Returns `true` if the owning [`AsyncViewport`] is still alive.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.wake_up.strong_count() > 0
    }

    /// Returns `true` if `self` and `other` belong to the same viewport.
    #[must_use]
    pub fn is_same_viewport(&self, other: &Self) -> bool {
        self.wake_up_requested.as_ptr() == other.wake_up_requested.as_ptr()
    }
}

impl<W> Deref for AsyncWakeUpGuard<W>
where
    W: AsyncWakeUp,
{
    type Target = W;

    fn deref(&self) -> &Self::Target {
        &self.waker
    }
}

impl<W, T> AsRef<T> for AsyncWakeUpGuard<W>
where
    W: AsyncWakeUp,
    <Self as Deref>::Target: AsRef<T>,
{
    fn as_ref(&self) -> &T {
        self.deref().as_ref()
    }
}

impl<W> Drop for AsyncWakeUpGuard<W>
where
    W: AsyncWakeUp,
{
    fn drop(&mut self) {
        self.waker.wake_up();
    }
}

impl<T> AsyncWakeUp for &T
where
    T: AsyncWakeUp,
{
    fn wake_up(&self) {
        (*self).wake_up();
    }
}

impl AsyncRuntime for AsyncCurrentRuntime {
    fn spawn<Fut, T>(&mut self, future: Fut) -> JoinHandle<T>
    where
        Fut: 'static + Send + Future<Output = T>,
        T: 'static + Send,
    {
        tokio::spawn(future)
    }

    fn spawn_join_set<Fut, T>(&mut self, join_set: &mut JoinSet<T>, future: Fut)
    where
        Fut: 'static + Send + Future<Output = T>,
        T: 'static + Send,
    {
        drop(join_set.spawn(future));
    }

    fn block_on<T>(&mut self, join_handle: JoinHandle<T>) -> Option<T>
    where
        T: 'static + Send,
    {
        match Handle::current().block_on(join_handle) {
            Ok(value) => Some(value),

            Err(error) => {
                if error.is_cancelled() {
                    None
                } else {
                    resume_unwind(error.into_panic());
                }
            }
        }
    }
}

impl AsyncRuntime for Handle {
    fn spawn<Fut, T>(&mut self, future: Fut) -> JoinHandle<T>
    where
        Fut: 'static + Send + Future<Output = T>,
        T: 'static + Send,
    {
        Handle::spawn(self, future)
    }

    fn spawn_join_set<Fut, T>(&mut self, join_set: &mut JoinSet<T>, future: Fut)
    where
        Fut: 'static + Send + Future<Output = T>,
        T: 'static + Send,
    {
        drop(join_set.spawn_on(future, self));
    }

    fn block_on<T>(&mut self, join_handle: JoinHandle<T>) -> Option<T>
    where
        T: 'static + Send,
    {
        match Handle::block_on(self, join_handle) {
            Ok(value) => Some(value),

            Err(error) => {
                if error.is_cancelled() {
                    None
                } else {
                    resume_unwind(error.into_panic());
                }
            }
        }
    }
}

// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Primitives for calling asynchronous code from immediate mode GUIs.
//!
//! The central type is [`AsyncGlue`], which spawns a [`Future`] onto a Tokio
//! runtime and exposes its result through a poll-based API that fits naturally
//! into an immediate mode update loop. An [`AsyncGlueViewport`] ties a GUI
//! viewport to a wake-up callback so that completed tasks automatically
//! trigger a repaint.
//!
//! With the `sync` feature enabled, the [`sync`] and [`trigger`] modules
//! provide channel wrappers that wake viewports when values are sent,
//! enabling continuous progress reporting from async tasks to the UI.
//!
//! ## Feature flags
#![cfg_attr(
    feature = "document-features",
    cfg_attr(doc, doc = ::document_features::document_features!())
)]
//
// Clippy lints.
#![warn(clippy::pedantic)]
#![warn(clippy::cargo)]

use ::std::hint::unreachable_unchecked;
use ::std::mem::replace;
use ::std::ops::Deref;
use ::std::panic::resume_unwind;
use ::std::sync::atomic::{AtomicBool, Ordering};
use ::std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};

use ::tokio::runtime::Handle;
use ::tokio::task::JoinHandle;

/// Re-export `tokio` crate.
pub use ::tokio;

/// Wrappers around `tokio::sync` primitives that wake up viewports on send.
#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub mod sync;
/// A notification channel for signaling async tasks from the UI thread.
#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub mod trigger;

/// Bridges an asynchronous task and an immediate mode UI update loop.
///
/// `AsyncGlue` spawns a [`Future`] onto a Tokio runtime and tracks its
/// lifecycle through three states: [`Stopped`](AsyncGlueState::Stopped),
/// [`Running`](AsyncGlueState::Running), and
/// [`Completed`](AsyncGlueState::Completed). Call [`poll()`](Self::poll)
/// every frame to check whether the spawned task has finished; when it has,
/// the state transitions to `Completed` and the result becomes available.
/// If the task was aborted (e.g. via [`JoinHandle::abort()`](tokio::task::JoinHandle::abort)),
/// `poll()` transitions the state back to `Stopped` instead.
///
/// When the task finishes it automatically requests a repaint of the
/// associated viewport through its [`AsyncGlueWaker`], so the UI is
/// guaranteed to run at least one more frame to observe the result.
///
/// Manual state changes through [`start()`](Self::start) and
/// [`take_state()`](Self::take_state) also request wake-ups by default.
/// This can be configured via
/// [`set_wake_up_on_manual_state_change()`](Self::set_wake_up_on_manual_state_change).
///
/// `T` is the output type of the spawned future. `A` controls how the
/// Tokio runtime is accessed: the default [`AsyncGlueCurrentRuntime`] uses
/// the thread-local runtime context (set by
/// [`Runtime::enter()`](tokio::runtime::Runtime::enter)), while passing a
/// [`Handle`] stores it inside the `AsyncGlue` so
/// it works on threads where the runtime context is not available.
///
/// Derefs to [`AsyncGlueState<T>`] so you can pattern-match directly on an
/// `AsyncGlue` value.
///
/// # Drop behaviour
///
/// Dropping an `AsyncGlue` whose task is still running will abort the task,
/// block until it finishes, and discard the result. The Tokio runtime
/// **must** still be alive at that point; otherwise the process will abort.
///
/// If the spawned future panicked and the panic was never observed through
/// [`poll()`](Self::poll), the drop implementation re-raises it with
/// [`resume_unwind`]. If the `AsyncGlue` itself
/// is being dropped during unwinding (e.g. due to another panic), this
/// causes a double panic and the process aborts.
pub struct AsyncGlue<T = (), A = AsyncGlueCurrentRuntime>
where
    T: 'static + Send,
    A: AsyncGlueRuntime,
{
    state: AsyncGlueState<T>,
    waker: AsyncGlueWaker,
    task_is_finishing: Arc<AtomicBool>,
    wake_up_on_manual_state_change: bool,
    runtime: A,
}

/// The lifecycle state of an [`AsyncGlue`] task.
#[derive(Debug)]
pub enum AsyncGlueState<T>
where
    T: 'static + Send,
{
    /// No task is running. This is the initial state, and the state after a
    /// task is aborted.
    Stopped,
    /// A task is running. The [`JoinHandle`] can be used to abort it.
    Running(JoinHandle<T>),
    /// The task finished and produced a value.
    Completed(T),
}

/// Represents a single GUI viewport (window) that can be woken up from
/// asynchronous tasks.
///
/// An `AsyncGlueViewport` owns a wake-up callback (typically one that
/// requests a repaint of the viewport) and hands out [`AsyncGlueWaker`]
/// handles via [`new_waker()`](Self::new_waker).
///
/// The wake-up callback **must not block**, because it may be called from
/// inside an async executor.
#[derive(Clone)]
pub struct AsyncGlueViewport {
    wake_up_requested: Arc<AtomicBool>,
    wake_up: Arc<AsyncGlueWakeUpSlot>,
}

/// A thread-safe collection of [`AsyncGlueWaker`]s that can all be woken up
/// at once.
///
/// This is primarily used by synchronisation primitives (e.g.
/// [`crate::sync::watch`]) that need to notify every viewport observing the same
/// shared value.
#[derive(Clone)]
pub struct AsyncGlueWakerList {
    inner: Arc<RwLock<AsyncGlueWakerListInner>>,
}

struct AsyncGlueWakerListInner {
    wakers: Vec<Option<AsyncGlueWaker>>,
    free: Vec<usize>,
}

/// A lightweight, cloneable handle that can request a repaint of the
/// [`AsyncGlueViewport`] it was created from.
///
/// If the viewport has been dropped, [`AsyncGlueWakeUp::wake_up`]
/// becomes a no-op and returns `false`.
#[derive(Clone)]
pub struct AsyncGlueWaker {
    wake_up_requested: Arc<AtomicBool>,
    wake_up: Weak<AsyncGlueWakeUpSlot>,
}

type AsyncGlueWakeUpSlot = RwLock<Option<AsyncGlueWakeUpCallback>>;
pub type AsyncGlueWakeUpCallback = Arc<dyn Fn() + Send + Sync>;

/// RAII guard that wakes up on drop.
pub struct AsyncGlueWakeUpGuard<W>
where
    W: AsyncGlueWakeUp,
{
    waker: W,
}

struct AsyncGlueTaskFinishNotifier {
    task_is_finishing: Arc<AtomicBool>,
    waker: AsyncGlueWaker,
}

/// Common interface for types that can request a viewport wake-up.
pub trait AsyncGlueWakeUp {
    /// Creates a guard that calls [`AsyncGlueWakeUp::wake_up`] when dropped.
    #[must_use]
    fn wake_up_guard(&self) -> AsyncGlueWakeUpGuard<&Self>
    where
        Self: Sized,
    {
        AsyncGlueWakeUpGuard { waker: self }
    }

    #[must_use]
    fn wake_up_guard_owned(self) -> AsyncGlueWakeUpGuard<Self>
    where
        Self: Sized,
    {
        AsyncGlueWakeUpGuard { waker: self }
    }

    /// Requests a wake-up.
    fn wake_up(&self);
}

/// Abstraction over how [`AsyncGlue`] accesses a Tokio runtime.
///
/// Implemented for [`AsyncGlueCurrentRuntime`] (thread-local context) and
/// [`Handle`] (explicit handle stored inside `AsyncGlue`).
pub trait AsyncGlueRuntime {
    /// Spawns a future onto the runtime, returning a [`JoinHandle`].
    fn spawn<Fut, T>(&mut self, future: Fut) -> JoinHandle<T>
    where
        Fut: 'static + Send + Future<Output = T>,
        T: 'static + Send;

    /// Blocks the current thread until the task completes.
    ///
    /// Returns `Some(value)` on success, or `None` if the task was
    /// cancelled. Re-raises the panic (via [`resume_unwind`]) if the task
    /// panicked.
    fn block_on<T>(&mut self, join_handle: JoinHandle<T>) -> Option<T>
    where
        T: 'static + Send;
}

/// The default [`AsyncGlueRuntime`] for [`AsyncGlue`].
///
/// Uses the Tokio runtime entered on the current thread (i.e. the one set
/// up by [`Runtime::enter()`](tokio::runtime::Runtime::enter)). This is
/// convenient when the runtime context is guaranteed to be available, but
/// will panic if it is not (e.g. in a deferred viewport callback running on
/// a different thread). In that case, pass a
/// [`Handle`] explicitly instead.
#[derive(Default)]
pub struct AsyncGlueCurrentRuntime;

impl<T, A> Drop for AsyncGlue<T, A>
where
    T: 'static + Send,
    A: AsyncGlueRuntime,
{
    fn drop(&mut self) {
        if let AsyncGlueState::Running(join_handle) =
            replace(&mut self.state, AsyncGlueState::Stopped)
        {
            join_handle.abort();

            // This will abort the whole process if already unwinding and spawned future panicked.
            // This will abort the whole process if already unwinding and runtime already stopped.
            self.runtime.block_on(join_handle);
        }
    }
}

impl<T, A, U> AsRef<U> for AsyncGlue<T, A>
where
    T: 'static + Send,
    A: AsyncGlueRuntime,
    <Self as Deref>::Target: AsRef<U>,
{
    fn as_ref(&self) -> &U {
        self.deref().as_ref()
    }
}

impl<T, A> Deref for AsyncGlue<T, A>
where
    T: 'static + Send,
    A: AsyncGlueRuntime,
{
    type Target = AsyncGlueState<T>;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl<T, A> AsyncGlue<T, A>
where
    T: 'static + Send,
    A: Default + AsyncGlueRuntime,
{
    /// Creates a new `AsyncGlue` in the [`Stopped`](AsyncGlueState::Stopped)
    /// state, using `A::default()` as the runtime.
    #[must_use]
    pub fn new(waker: AsyncGlueWaker) -> Self {
        Self::new_with_runtime(waker, A::default())
    }
}

impl<T, A> AsyncGlue<T, A>
where
    T: 'static + Send,
    A: AsyncGlueRuntime,
{
    /// Creates a new `AsyncGlue` in the [`Stopped`](AsyncGlueState::Stopped)
    /// state with an explicit runtime.
    #[must_use]
    pub fn new_with_runtime(waker: AsyncGlueWaker, runtime: A) -> Self {
        Self {
            state: AsyncGlueState::Stopped,
            waker,
            task_is_finishing: Arc::new(AtomicBool::new(false)),
            wake_up_on_manual_state_change: true,
            runtime,
        }
    }

    /// Spawns `future` on the runtime and moves the state to
    /// [`Running`](AsyncGlueState::Running).
    ///
    /// If [`wake_up_on_manual_state_change`](Self::wake_up_on_manual_state_change)
    /// is enabled (default), this requests a wake-up immediately after the
    /// state transition.
    ///
    /// Returns the **previous** state. If a task was already running, the
    /// caller is responsible for deciding what to do with the old
    /// [`JoinHandle`] (e.g. abort it).
    #[must_use]
    pub fn start<Fut>(&mut self, future: Fut) -> AsyncGlueState<T>
    where
        Fut: 'static + Send + Future<Output = T>,
    {
        if self.wake_up_on_manual_state_change {
            self.waker.wake_up();
        }

        self.task_is_finishing = Arc::new(AtomicBool::new(false));
        let wake_up_guard = AsyncGlueTaskFinishNotifier {
            task_is_finishing: self.task_is_finishing.clone(),
            waker: self.waker.clone(),
        }
        .wake_up_guard_owned();
        replace(
            &mut self.state,
            AsyncGlueState::Running(self.runtime.spawn(async move {
                let _wake_up_guard = wake_up_guard;
                future.await
            })),
        )
    }

    /// Takes the current state, leaving [`Stopped`](AsyncGlueState::Stopped)
    /// in its place.
    ///
    /// If [`wake_up_on_manual_state_change`](Self::wake_up_on_manual_state_change)
    /// is enabled (default), this requests a wake-up immediately after the
    /// state transition.
    #[must_use]
    pub fn take_state(&mut self) -> AsyncGlueState<T> {
        if self.wake_up_on_manual_state_change {
            self.waker.wake_up();
        }

        replace(&mut self.state, AsyncGlueState::Stopped)
    }

    /// Returns whether manual state changes should trigger wake-ups.
    ///
    /// When enabled, [`start()`](Self::start) and [`take_state()`](Self::take_state)
    /// call [`AsyncGlueWakeUp::wake_up`] on this glue's waker after changing
    /// state.
    #[must_use]
    pub fn wake_up_on_manual_state_change(&self) -> bool {
        self.wake_up_on_manual_state_change
    }

    /// Enables or disables wake-ups triggered by manual state changes.
    ///
    /// This affects [`start()`](Self::start) and [`take_state()`](Self::take_state).
    pub fn set_wake_up_on_manual_state_change(&mut self, wake_up_on_manual_state_change: bool) {
        self.wake_up_on_manual_state_change = wake_up_on_manual_state_change;
    }

    /// Checks whether the running task has finished and, if so, collects
    /// its result.
    ///
    /// Returns `true` if a state transition occurred (to
    /// [`Completed`](AsyncGlueState::Completed) on success, or back to
    /// [`Stopped`](AsyncGlueState::Stopped) if the task was aborted).
    /// Returns `false` if the task is still running or no task has been started.
    ///
    /// Call this once per frame, before inspecting the state.
    pub fn poll(&mut self) -> bool {
        if let AsyncGlueState::Running(join_handle) = &self.state {
            if !self.task_is_finishing.load(Ordering::Acquire) {
                return false;
            }

            if !join_handle.is_finished() {
                // Tokio runtime is still working with finishing task, will need another redraw.
                self.waker.wake_up();
                return false;
            }

            let AsyncGlueState::Running(join_handle) =
                replace(&mut self.state, AsyncGlueState::Stopped)
            else {
                unsafe {
                    // SAFETY: We already checked that state is `AsyncGlueState::Running`.
                    unreachable_unchecked()
                }
            };

            if let Some(value) = self.runtime.block_on(join_handle) {
                self.state = AsyncGlueState::Completed(value);
            }
            // Do nothing if task was aborted: state was already changed to
            // `AsyncGlueState::Stopped` above.

            true
        } else {
            false
        }
    }
}

impl<T> AsyncGlueState<T>
where
    T: 'static + Send,
{
    /// Returns `true` if the state is [`Stopped`](Self::Stopped).
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        matches!(self, AsyncGlueState::Stopped)
    }

    /// Returns `true` if the state is [`Running`](Self::Running).
    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self, AsyncGlueState::Running(_))
    }

    /// Returns `true` if the state is [`Completed`](Self::Completed).
    #[must_use]
    pub fn is_completed(&self) -> bool {
        matches!(self, AsyncGlueState::Completed(_))
    }
}

impl Default for AsyncGlueViewport {
    fn default() -> Self {
        Self {
            wake_up_requested: Arc::new(AtomicBool::new(false)),
            wake_up: Arc::new(RwLock::new(None)),
        }
    }
}

impl AsyncGlueWakeUp for AsyncGlueViewport {
    fn wake_up(&self) {
        if self
            .wake_up_requested
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            let wake_up = self
                .wake_up
                .read()
                .expect("Failed to read-lock AsyncGlueViewport callback: poisoned by panic")
                .clone();
            if let Some(wake_up) = wake_up {
                (wake_up)();
            }
        }
    }
}

impl AsyncGlueViewport {
    /// Creates a new viewport with the given wake-up callback.
    ///
    /// `wake_up` is called whenever an async task associated with this
    /// viewport finishes or a synchronisation primitive is updated. It
    /// **must not block**.
    #[must_use]
    pub fn new_with_wake_up(wake_up: AsyncGlueWakeUpCallback) -> Self {
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
        wake_up: Option<AsyncGlueWakeUpCallback>,
    ) -> Option<AsyncGlueWakeUpCallback> {
        replace(
            &mut *self
                .wake_up
                .write()
                .expect("Failed to write-lock AsyncGlueViewport callback: poisoned by panic"),
            wake_up,
        )
    }

    /// Creates an [`AsyncGlue`] wired to this viewport, using `A::default()`
    /// as the runtime.
    #[must_use]
    pub fn new_glue<T, A>(&self) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncGlueRuntime,
    {
        AsyncGlue::new(self.new_waker())
    }

    /// Creates an [`AsyncGlue`] wired to this viewport with an explicit
    /// runtime.
    #[must_use]
    pub fn new_glue_with_runtime<T, A>(&self, runtime: A) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: AsyncGlueRuntime,
    {
        AsyncGlue::new_with_runtime(self.new_waker(), runtime)
    }

    /// Creates a new [`AsyncGlueWaker`] that can request a repaint of this
    /// viewport.
    #[must_use]
    pub fn new_waker(&self) -> AsyncGlueWaker {
        AsyncGlueWaker {
            wake_up_requested: self.wake_up_requested.clone(),
            wake_up: Arc::downgrade(&self.wake_up),
        }
    }

    /// Acknowledges that the viewport has been repainted after a wake-up
    /// request, clearing the pending flag.
    ///
    /// Call this at the start of every frame (before polling any
    /// [`AsyncGlue`] instances) so that subsequent wake-up requests are not
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

impl Default for AsyncGlueWakerList {
    fn default() -> Self {
        Self::with_capacity(1)
    }
}

impl AsyncGlueWakeUp for AsyncGlueWakerList {
    fn wake_up(&self) {
        for waker in self.inner().wakers.iter().flatten() {
            waker.wake_up();
        }
    }
}

impl AsyncGlueWakerList {
    /// Creates a new waker list pre-allocated for `capacity` wakers.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(AsyncGlueWakerListInner {
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
    pub fn add_waker(&self, waker: AsyncGlueWaker) -> usize {
        let mut inner = self.inner_mut();
        if let Some(idx) = inner.free.pop() {
            let place = unsafe {
                // SAFETY: This is safe because we never shrink the vector and put valid indexes only into free list.
                inner.wakers.get_unchecked_mut(idx)
            };
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
        let place = unsafe {
            // SAFETY: This is safe because `idx` is a valid index returned by `AsyncGlueWakerList::add_waker()`.
            inner.wakers.get_unchecked_mut(idx)
        };
        *place = None;
        inner.free.push(idx);
    }

    fn inner(&'_ self) -> RwLockReadGuard<'_, AsyncGlueWakerListInner> {
        self.inner
            .read()
            .expect("Failed to read-lock AsyncGlueWakerList: poisoned by panic in another thread")
    }

    fn inner_mut(&'_ self) -> RwLockWriteGuard<'_, AsyncGlueWakerListInner> {
        self.inner
            .write()
            .expect("Failed to write-lock AsyncGlueWakerList: poisoned by panic in another thread")
    }
}

impl AsyncGlueWakeUp for AsyncGlueWaker {
    fn wake_up(&self) {
        if self
            .wake_up_requested
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            if let Some(wake_up) = self.wake_up.upgrade() {
                let wake_up = wake_up
                    .read()
                    .expect("Failed to read-lock AsyncGlueWaker callback: poisoned by panic")
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

impl AsyncGlueWaker {
    /// Returns `true` if the owning [`AsyncGlueViewport`] is still alive.
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

impl<W> Deref for AsyncGlueWakeUpGuard<W>
where
    W: AsyncGlueWakeUp,
{
    type Target = W;

    fn deref(&self) -> &Self::Target {
        &self.waker
    }
}

impl<W, T> AsRef<T> for AsyncGlueWakeUpGuard<W>
where
    W: AsyncGlueWakeUp,
    <Self as Deref>::Target: AsRef<T>,
{
    fn as_ref(&self) -> &T {
        self.deref().as_ref()
    }
}

impl<W> Drop for AsyncGlueWakeUpGuard<W>
where
    W: AsyncGlueWakeUp,
{
    fn drop(&mut self) {
        self.waker.wake_up();
    }
}

impl AsyncGlueWakeUp for AsyncGlueTaskFinishNotifier {
    fn wake_up(&self) {
        self.task_is_finishing.store(true, Ordering::Release);
        self.waker.wake_up();
    }
}

impl<T> AsyncGlueWakeUp for &T
where
    T: AsyncGlueWakeUp,
{
    fn wake_up(&self) {
        (*self).wake_up();
    }
}

impl AsyncGlueRuntime for AsyncGlueCurrentRuntime {
    fn spawn<Fut, T>(&mut self, future: Fut) -> JoinHandle<T>
    where
        Fut: 'static + Send + Future<Output = T>,
        T: 'static + Send,
    {
        tokio::spawn(future)
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

impl AsyncGlueRuntime for Handle {
    fn spawn<Fut, T>(&mut self, future: Fut) -> JoinHandle<T>
    where
        Fut: 'static + Send + Future<Output = T>,
        T: 'static + Send,
    {
        Handle::spawn(self, future)
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

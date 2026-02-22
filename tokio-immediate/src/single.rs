// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Async call: spawn one [`Future`] and track its result.
//!
//! [`AsyncCall`] spawns a single [`Future`] onto
//! a Tokio runtime and exposes its result through a poll-based API that fits
//! naturally into an immediate mode update loop.

use ::std::hint::unreachable_unchecked;
use ::std::mem::replace;
use ::std::ops::Deref;
use ::std::pin::Pin;
use ::std::sync::Arc;
use ::std::sync::atomic::{AtomicBool, Ordering};
use ::std::task::{Context, Poll};

use ::tokio::task::JoinHandle;

use crate::{AsyncCurrentRuntime, AsyncRuntime, AsyncWakeUp, AsyncWakeUpGuard, AsyncWaker};

/// Bridges a single asynchronous task and an immediate mode UI update loop.
///
/// `AsyncCall` spawns a [`Future`] onto a Tokio runtime and tracks its
/// lifecycle through three states: [`Stopped`](AsyncCallState::Stopped),
/// [`Running`](AsyncCallState::Running), and
/// [`Completed`](AsyncCallState::Completed). Call [`poll()`](Self::poll)
/// every frame to check whether the spawned task has finished; when it has,
/// the state transitions to `Completed` and the result becomes available.
/// If the task was aborted (e.g. via [`JoinHandle::abort()`](tokio::task::JoinHandle::abort)),
/// `poll()` transitions the state back to `Stopped` instead.
///
/// When the task finishes it automatically requests a repaint of the
/// associated viewport through its [`AsyncWaker`], so the UI is
/// guaranteed to run at least one more frame to observe the result.
///
/// Manual state changes through [`start()`](Self::start) and
/// [`take_state()`](Self::take_state) also request wake-ups by default.
/// This can be configured via
/// [`set_wake_up_on_manual_state_change()`](Self::set_wake_up_on_manual_state_change).
///
/// `T` is the output type of the spawned future. `A` controls how the
/// Tokio runtime is accessed: the default [`AsyncCurrentRuntime`] uses
/// the thread-local runtime context (set by
/// [`Runtime::enter()`](tokio::runtime::Runtime::enter)), while passing a
/// [`Handle`](tokio::runtime::Handle) stores it inside the `AsyncCall` so
/// it works on threads where the runtime context is not available.
///
/// Derefs to [`AsyncCallState<T>`] so you can pattern-match directly on an
/// `AsyncCall` value.
///
/// # Drop behaviour
///
/// Dropping an `AsyncCall` whose task is still running will abort the task,
/// block until it finishes, and discard the result. The Tokio runtime
/// **must** still be alive at that point; otherwise the process will abort.
/// To avoid this abort-and-wait-on-drop behaviour, call
/// [`take_state()`](Self::take_state) first and take ownership of the returned
/// [`AsyncCallState::Running`]([`JoinHandle`](tokio::task::JoinHandle)),
/// then decide its lifecycle yourself (for example: drop it, abort it, or await it).
///
/// If the spawned future panicked and the panic was never observed through
/// [`poll()`](Self::poll), the drop implementation re-raises it with
/// [`resume_unwind`](std::panic::resume_unwind). If the `AsyncCall` itself
/// is being dropped during unwinding (e.g. due to another panic), this
/// causes a double panic and the process aborts.
pub struct AsyncCall<T = (), A = AsyncCurrentRuntime>
where
    T: 'static + Send,
    A: AsyncRuntime,
{
    state: AsyncCallState<T>,
    waker: AsyncWaker,
    task_is_finishing: Arc<AtomicBool>,
    wake_up_on_manual_state_change: bool,
    runtime: A,
}

/// The lifecycle state of an [`AsyncCall`] task.
#[derive(Debug)]
pub enum AsyncCallState<T>
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

struct AsyncCallTask<Fut>
where
    Fut: Future,
{
    // Make sure that task future is dropped before wake-up notification is sent.
    // This minimizes the possibility of UI loop being woken up when JoinHandle::is_finished()
    // is still false.
    future: Fut,
    _wake_up_guard: AsyncWakeUpGuard<AsyncCallTaskFinishNotifier>,
}

struct AsyncCallTaskFinishNotifier {
    task_is_finishing: Arc<AtomicBool>,
    waker: AsyncWaker,
}

impl<T, A> Drop for AsyncCall<T, A>
where
    T: 'static + Send,
    A: AsyncRuntime,
{
    fn drop(&mut self) {
        if let AsyncCallState::Running(join_handle) =
            replace(&mut self.state, AsyncCallState::Stopped)
        {
            join_handle.abort();

            // This will abort the whole process if already unwinding and spawned future panicked.
            // This will abort the whole process if already unwinding and runtime already stopped.
            self.runtime.block_on(join_handle);
        }
    }
}

impl<T, A, U> AsRef<U> for AsyncCall<T, A>
where
    T: 'static + Send,
    A: AsyncRuntime,
    <Self as Deref>::Target: AsRef<U>,
{
    fn as_ref(&self) -> &U {
        self.deref().as_ref()
    }
}

impl<T, A> Deref for AsyncCall<T, A>
where
    T: 'static + Send,
    A: AsyncRuntime,
{
    type Target = AsyncCallState<T>;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl<T, A> AsyncCall<T, A>
where
    T: 'static + Send,
    A: Default + AsyncRuntime,
{
    /// Creates a new `AsyncCall` in the [`Stopped`](AsyncCallState::Stopped)
    /// state, using `A::default()` as the runtime.
    #[must_use]
    pub fn new(waker: AsyncWaker) -> Self {
        Self::new_with_runtime(waker, A::default())
    }
}

impl<T, A> AsyncCall<T, A>
where
    T: 'static + Send,
    A: AsyncRuntime,
{
    /// Creates a new `AsyncCall` in the [`Stopped`](AsyncCallState::Stopped)
    /// state with an explicit runtime.
    #[must_use]
    pub fn new_with_runtime(waker: AsyncWaker, runtime: A) -> Self {
        Self {
            state: AsyncCallState::Stopped,
            waker,
            task_is_finishing: Arc::new(AtomicBool::new(false)),
            wake_up_on_manual_state_change: true,
            runtime,
        }
    }

    /// Spawns `future` on the runtime and moves the state to
    /// [`Running`](AsyncCallState::Running).
    ///
    /// If [`wake_up_on_manual_state_change`](Self::wake_up_on_manual_state_change)
    /// is enabled (default), this requests a wake-up immediately after the
    /// state transition.
    ///
    /// Returns the **previous** state. If a task was already running, the
    /// caller is responsible for deciding what to do with the old
    /// [`JoinHandle`] (e.g. abort it).
    ///
    /// # Panics
    ///
    /// Panics if runtime access preconditions are not met by the selected
    /// [`AsyncRuntime`] implementation. For example, [`AsyncCurrentRuntime`]
    /// panics when called outside a Tokio runtime context.
    #[must_use]
    pub fn start<Fut>(&mut self, future: Fut) -> AsyncCallState<T>
    where
        Fut: 'static + Send + Future<Output = T>,
    {
        if self.wake_up_on_manual_state_change {
            self.waker.wake_up();
        }

        self.task_is_finishing = Arc::new(AtomicBool::new(false));
        let wake_up_guard = AsyncCallTaskFinishNotifier {
            task_is_finishing: self.task_is_finishing.clone(),
            waker: self.waker.clone(),
        }
        .wake_up_guard_owned();

        let task = AsyncCallTask {
            future,
            _wake_up_guard: wake_up_guard,
        };
        replace(
            &mut self.state,
            AsyncCallState::Running(self.runtime.spawn(task)),
        )
    }

    /// Takes the current state, leaving [`Stopped`](AsyncCallState::Stopped)
    /// in its place.
    ///
    /// If [`wake_up_on_manual_state_change`](Self::wake_up_on_manual_state_change)
    /// is enabled (default), this requests a wake-up immediately after the
    /// state transition.
    #[must_use]
    pub fn take_state(&mut self) -> AsyncCallState<T> {
        if self.wake_up_on_manual_state_change {
            self.waker.wake_up();
        }

        replace(&mut self.state, AsyncCallState::Stopped)
    }

    /// Returns whether manual state changes should trigger wake-ups.
    ///
    /// When enabled, [`start()`](Self::start) and [`take_state()`](Self::take_state)
    /// call [`AsyncWakeUp::wake_up`] on this call's waker after changing
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
    /// [`Completed`](AsyncCallState::Completed) on success, or back to
    /// [`Stopped`](AsyncCallState::Stopped) if the task was aborted).
    /// Returns `false` if the task is still running or no task has been started.
    ///
    /// Call this once per frame, before inspecting the state.
    ///
    /// # Panics
    ///
    /// Panics if runtime access preconditions are not met by the selected
    /// [`AsyncRuntime`] implementation. For example, [`AsyncCurrentRuntime`]
    /// panics when called outside a Tokio runtime context.
    ///
    /// Re-raises panics from the spawned future in the calling thread.
    pub fn poll(&mut self) -> bool {
        if let AsyncCallState::Running(join_handle) = &self.state {
            if !self.task_is_finishing.load(Ordering::Acquire) {
                return false;
            }

            if !join_handle.is_finished() {
                // Tokio runtime is still working with finishing task, will need another redraw.
                self.waker.wake_up();
                return false;
            }

            let AsyncCallState::Running(join_handle) =
                replace(&mut self.state, AsyncCallState::Stopped)
            else {
                // SAFETY: The `if let` above already established that `self.state` is `Running`.
                unsafe { unreachable_unchecked() }
            };

            if let Some(value) = self.runtime.block_on(join_handle) {
                self.state = AsyncCallState::Completed(value);
            }
            // Do nothing if task was aborted: state was already changed to
            // `AsyncCallState::Stopped` above.

            true
        } else {
            false
        }
    }
}

impl<T> AsyncCallState<T>
where
    T: 'static + Send,
{
    /// Returns `true` if the state is [`Stopped`](Self::Stopped).
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        matches!(self, AsyncCallState::Stopped)
    }

    /// Returns `true` if the state is [`Running`](Self::Running).
    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self, AsyncCallState::Running(_))
    }

    /// Returns `true` if the state is [`Completed`](Self::Completed).
    #[must_use]
    pub fn is_completed(&self) -> bool {
        matches!(self, AsyncCallState::Completed(_))
    }
}

impl<Fut> Future for AsyncCallTask<Fut>
where
    Fut: Future,
{
    type Output = Fut::Output;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: Once `AsyncCallTask` is pinned by the executor, `future` will not move again.
        // We only create a pinned projection to poll it in place.
        unsafe {
            let this = self.as_mut().get_unchecked_mut();
            Pin::new_unchecked(&mut this.future)
        }
        .poll(context)
    }
}

impl AsyncWakeUp for AsyncCallTaskFinishNotifier {
    fn wake_up(&self) {
        self.task_is_finishing.store(true, Ordering::Release);
        self.waker.wake_up();
    }
}

// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Async parallel runner: execute submitted futures concurrently.

use ::std::collections::VecDeque;
use ::std::future::Future;
use ::std::panic::resume_unwind;
use ::std::pin::Pin;
use ::std::sync::Arc;
use ::std::sync::atomic::{AtomicUsize, Ordering};
use ::std::task::{Context, Poll};

use ::tokio::task::JoinSet;

use crate::{AsyncCurrentRuntime, AsyncRuntime, AsyncWakeUp, AsyncWakeUpGuard, AsyncWaker};

/// Executes asynchronous tasks concurrently on a Tokio runtime.
///
/// `AsyncParallelRunner` accepts futures through [`run()`](Self::run) and
/// [`run_unit()`](Self::run_unit). Submitted futures are started immediately and
/// run in parallel.
///
/// Return values from submitted futures are delivered through an internal
/// FIFO queue.
pub struct AsyncParallelRunner<T = (), A = AsyncCurrentRuntime>
where
    T: 'static + Send,
    A: AsyncRuntime,
{
    join_set: JoinSet<Option<T>>,
    values: VecDeque<T>,
    finishing_tasks: Arc<AtomicUsize>,
    waker: AsyncWaker,
    runtime: A,
}

/// A non-blocking iterator over up to `n` completed return values from an
/// [`AsyncParallelRunner`].
pub struct AsyncParallelRunnerTake<'a, T, A>
where
    T: 'static + Send,
    A: AsyncRuntime,
{
    runner: &'a mut AsyncParallelRunner<T, A>,
    remaining: usize,
}

struct AsyncParallelRunnerTask<Fut>
where
    Fut: Future,
{
    future: Fut,
    _wake_up_guard: AsyncWakeUpGuard<AsyncParallelRunnerTaskFinishNotifier>,
}

struct AsyncParallelRunnerTaskFinishNotifier {
    finishing_tasks: Arc<AtomicUsize>,
    waker: AsyncWaker,
}

impl<T, A> AsyncParallelRunner<T, A>
where
    T: 'static + Send,
    A: Default + AsyncRuntime,
{
    /// Creates a new runner using `A::default()` as the runtime.
    #[must_use]
    pub fn new(waker: AsyncWaker) -> Self {
        Self::new_with_runtime(waker, A::default())
    }
}

impl<T, A> AsyncParallelRunner<T, A>
where
    T: 'static + Send,
    A: AsyncRuntime,
{
    /// Creates a new runner with an explicit runtime.
    #[must_use]
    pub fn new_with_runtime(waker: AsyncWaker, runtime: A) -> Self {
        Self {
            join_set: JoinSet::new(),
            values: VecDeque::new(),
            finishing_tasks: Arc::new(AtomicUsize::new(0)),
            waker,
            runtime,
        }
    }

    /// Schedules a future whose output can be retrieved later.
    ///
    /// # Panics
    ///
    /// Panics if runtime access preconditions are not met by the selected
    /// [`AsyncRuntime`] implementation. For example, [`AsyncCurrentRuntime`]
    /// panics when called outside a Tokio runtime context.
    pub fn run<Fut>(&mut self, future: Fut)
    where
        Fut: 'static + Send + Future<Output = T>,
    {
        self.submit(async move { Some(future.await) });
    }

    /// Schedules a future whose output is discarded.
    ///
    /// # Panics
    ///
    /// Panics if runtime access preconditions are not met by the selected
    /// [`AsyncRuntime`] implementation. For example, [`AsyncCurrentRuntime`]
    /// panics when called outside a Tokio runtime context.
    pub fn run_unit<Fut>(&mut self, future: Fut)
    where
        Fut: 'static + Send + Future<Output = ()>,
    {
        self.submit(async move {
            future.await;
            None
        });
    }

    /// Polls completed tasks and re-raises future panics.
    ///
    /// # Panics
    ///
    /// Re-raises panics from submitted futures in the calling thread.
    pub fn poll(&mut self) {
        let pending = self.finishing_tasks.load(Ordering::Acquire);
        if pending == 0 {
            return;
        }

        let mut finished = 0;
        for _ in 0..pending {
            if let Some(result) = self.join_set.try_join_next() {
                match result {
                    Ok(Some(value)) => {
                        self.values.push_back(value);
                    }

                    Ok(None) => {}

                    Err(error) => {
                        assert!(
                            !error.is_cancelled(),
                            "Spawned future was unexpectedly aborted"
                        );
                        resume_unwind(error.into_panic());
                    }
                }
                finished += 1;
            } else {
                // Tokio runtime is still working with finishing task, will need another redraw.
                self.waker.wake_up();
                break;
            }
        }
        self.finishing_tasks.fetch_sub(finished, Ordering::AcqRel);
    }

    /// Returns `true` if some submitted futures are still running.
    ///
    /// Inverse of [`Self::is_idle`].
    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.is_idle()
    }

    /// Returns `true` if no submitted futures are still running.
    ///
    /// Inverse of [`Self::is_running`].
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.join_set.is_empty()
    }

    /// Returns the number of submitted futures that are still running.
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.join_set.len()
    }

    /// Pops one completed return value, if available.
    #[must_use]
    pub fn take_value(&mut self) -> Option<T> {
        self.values.pop_front()
    }

    /// Creates a non-blocking iterator over currently buffered return values.
    ///
    /// Equivalent to `self.take_values(self.values_len())`.
    #[must_use]
    pub fn take_values_current(&mut self) -> AsyncParallelRunnerTake<'_, T, A> {
        self.take_values(self.values_len())
    }

    /// Creates a non-blocking iterator that yields up to `n` completed return values.
    #[must_use]
    pub fn take_values(&mut self, n: usize) -> AsyncParallelRunnerTake<'_, T, A> {
        AsyncParallelRunnerTake {
            runner: self,
            remaining: n,
        }
    }

    /// Returns the number of completed return values currently buffered.
    #[must_use]
    pub fn values_len(&self) -> usize {
        self.values.len()
    }

    fn submit<Fut>(&mut self, future: Fut)
    where
        Fut: 'static + Send + Future<Output = Option<T>>,
    {
        let task_finish_notifier = AsyncParallelRunnerTaskFinishNotifier {
            finishing_tasks: self.finishing_tasks.clone(),
            waker: self.waker.clone(),
        };
        let wake_up_guard = task_finish_notifier.wake_up_guard_owned();

        let task = AsyncParallelRunnerTask {
            future,
            _wake_up_guard: wake_up_guard,
        };

        let runtime = &mut self.runtime;
        let join_set = &mut self.join_set;
        runtime.spawn_join_set(join_set, task);
    }
}

impl<Fut> Future for AsyncParallelRunnerTask<Fut>
where
    Fut: Future,
{
    type Output = Fut::Output;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: Once `AsyncParallelRunnerTask` is pinned by the executor, `future` will not
        // move again. We only create a pinned projection to poll it in place.
        unsafe {
            let this = self.as_mut().get_unchecked_mut();
            Pin::new_unchecked(&mut this.future)
        }
        .poll(context)
    }
}

impl AsyncWakeUp for AsyncParallelRunnerTaskFinishNotifier {
    fn wake_up(&self) {
        self.finishing_tasks.fetch_add(1, Ordering::Release);
        self.waker.wake_up();
    }
}

impl<T, A> Iterator for AsyncParallelRunnerTake<'_, T, A>
where
    T: 'static + Send,
    A: AsyncRuntime,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        if let Some(value) = self.runner.take_value() {
            self.remaining -= 1;
            Some(value)
        } else {
            None
        }
    }
}

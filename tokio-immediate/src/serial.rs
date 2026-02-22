// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Async serial runner: execute submitted futures one after another.

use ::std::future::Future;
use ::std::pin::Pin;
use ::std::sync::Arc;
use ::std::sync::atomic::{AtomicUsize, Ordering};

use ::tokio::sync::mpsc as tokio_mpsc;

use crate::single::AsyncCall;
use crate::sync::mpsc;
use crate::{AsyncCurrentRuntime, AsyncRuntime, AsyncWakeUp, AsyncWaker};

/// Executes asynchronous tasks sequentially on a Tokio runtime.
///
/// `AsyncSerialRunner` accepts futures through [`run()`](Self::run) and
/// [`run_unit()`](Self::run_unit). Submitted futures are executed strictly in
/// FIFO order, one at a time.
///
/// Return values from submitted futures are delivered through an
/// internal unbounded [`mpsc`] channel which can be accesses via
/// convenience methods like [`take_value()`](Self::take_value).
pub struct AsyncSerialRunner<T = (), A = AsyncCurrentRuntime>
where
    T: 'static + Send,
    A: AsyncRuntime,
{
    worker: AsyncCall<(), A>,
    sender: tokio_mpsc::UnboundedSender<BoxSerialFuture<T>>,
    receiver: mpsc::UnboundedReceiver<T>,
    pending: Arc<AtomicUsize>,
}

type BoxSerialFuture<T> = Pin<Box<dyn Future<Output = Option<T>> + Send + 'static>>;

/// A non-blocking iterator over up to `n` completed return values from an
/// [`AsyncSerialRunner`].
pub struct AsyncSerialRunnerTake<'a, T, A>
where
    T: 'static + Send,
    A: AsyncRuntime,
{
    runner: &'a mut AsyncSerialRunner<T, A>,
    remaining: usize,
}

impl<T, A> AsyncSerialRunner<T, A>
where
    T: 'static + Send,
    A: Default + AsyncRuntime,
{
    /// Creates a new runner using `A::default()` as the runtime.
    ///
    /// # Panics
    ///
    /// Panics if runtime access preconditions are not met by the selected
    /// [`AsyncRuntime`] implementation. For example, [`AsyncCurrentRuntime`]
    /// panics when called outside a Tokio runtime context.
    #[must_use]
    pub fn new(waker: AsyncWaker) -> Self {
        Self::new_with_runtime(waker, A::default())
    }
}

impl<T, A> AsyncSerialRunner<T, A>
where
    T: 'static + Send,
    A: AsyncRuntime,
{
    /// Creates a new runner with an explicit runtime.
    ///
    /// # Panics
    ///
    /// Panics if runtime access preconditions are not met by `runtime` while
    /// starting the internal worker task.
    #[must_use]
    pub fn new_with_runtime(waker: AsyncWaker, runtime: A) -> Self {
        let (future_sender, future_receiver) = tokio_mpsc::unbounded_channel();
        let (retval_sender, retval_receiver) = mpsc::unbounded_channel_with_waker(waker.clone());
        let pending_tasks = Arc::new(AtomicUsize::new(0));

        let mut worker = AsyncCall::new_with_runtime(waker, runtime);
        worker.set_wake_up_on_manual_state_change(false);

        let _ = worker.start(worker_loop(
            future_receiver,
            retval_sender,
            pending_tasks.clone(),
        ));

        Self {
            worker,
            sender: future_sender,
            receiver: retval_receiver,
            pending: pending_tasks,
        }
    }

    /// Schedules a future whose output can be retrieved later.
    pub fn run<Fut>(&mut self, future: Fut)
    where
        Fut: 'static + Send + Future<Output = T>,
    {
        self.submit(Box::pin(async move { Some(future.await) }));
    }

    /// Schedules a future whose output is discarded.
    ///
    /// The viewport is still woken up when the task completes.
    pub fn run_unit<Fut>(&mut self, future: Fut)
    where
        Fut: 'static + Send + Future<Output = ()>,
    {
        self.submit(Box::pin(async move {
            future.await;
            None
        }));
    }

    /// Polls worker state.
    ///
    /// If the worker task or one of the submitted futures panicked, this re-raises the panic.
    ///
    /// # Panics
    ///
    /// Re-raises panics from the internal worker task or submitted futures in
    /// the calling thread.
    pub fn poll(&mut self) {
        let _ = self.worker.poll();
    }

    /// Returns `true` if at least one submitted task has not finished yet.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.pending_len() > 0
    }

    /// Returns `true` if there are no running or pending tasks.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.pending_len() == 0
    }

    /// Returns the number of submitted tasks that are not finished yet.
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.pending.load(Ordering::Relaxed)
    }

    /// Pops one completed return value, if available.
    ///
    /// # Panics
    ///
    /// Panics if internal channel was closed.
    pub fn take_value(&mut self) -> Option<T> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value),

            Err(tokio_mpsc::error::TryRecvError::Empty) => None,

            Err(tokio_mpsc::error::TryRecvError::Disconnected) => {
                panic!("Failed to receive return value: serial worker return channel is closed")
            }
        }
    }

    /// Creates a non-blocking iterator over currently buffered return values.
    ///
    /// Equivalent to `self.take_values(self.completed_len())`.
    #[must_use]
    pub fn take_values_current(&mut self) -> AsyncSerialRunnerTake<'_, T, A> {
        self.take_values(self.values_len())
    }

    /// Creates a non-blocking iterator that yields up to `n` completed return values.
    #[must_use]
    pub fn take_values(&mut self, n: usize) -> AsyncSerialRunnerTake<'_, T, A> {
        AsyncSerialRunnerTake {
            runner: self,
            remaining: n,
        }
    }

    /// Returns the number of completed return values currently buffered.
    #[must_use]
    pub fn values_len(&self) -> usize {
        self.receiver.len()
    }

    fn submit(&mut self, future: BoxSerialFuture<T>) {
        self.pending.fetch_add(1, Ordering::Relaxed);

        self.sender
            .send(future)
            .expect("Failed to submit task: serial worker command channel is closed");
    }
}

async fn worker_loop<T>(
    mut receiver: tokio_mpsc::UnboundedReceiver<BoxSerialFuture<T>>,
    sender: mpsc::UnboundedSender<T>,
    pending: Arc<AtomicUsize>,
) where
    T: 'static + Send,
{
    loop {
        let future = receiver
            .recv()
            .await
            .expect("Failed to receive task: serial worker command channel is closed");

        if let Some(retval) = future.await {
            sender
                .im_send(retval)
                .expect("Failed to send return value: serial worker return channel is closed");
        }
        pending.fetch_sub(1, Ordering::Relaxed);
        sender.wake_up();
    }
}

impl<T, A> Iterator for AsyncSerialRunnerTake<'_, T, A>
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

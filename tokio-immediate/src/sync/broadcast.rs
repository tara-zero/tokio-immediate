// SPDX-License-Identifier: Apache-2.0 OR MIT

use ::std::ops::{Deref, DerefMut};

use ::tokio::sync::broadcast;

use crate::sync::waker_registration::WakerRegistration;
use crate::{AsyncWakeUp, AsyncWaker, AsyncWakerList};

/// Creates a new broadcast channel, returning a [`Sender`] and a [`Receiver`]
/// that is already registered with the given [`AsyncWaker`].
///
/// The receiver's viewport will be woken up whenever a value is sent through
/// [`Sender::im_send`].
#[must_use]
pub fn channel_with_waker<T>(capacity: usize, waker: AsyncWaker) -> (Sender<T>, Receiver<T>)
where
    T: Clone,
{
    let sender = Sender::new(capacity);
    let receiver = sender.im_subscribe_with_waker(waker);

    (sender, receiver)
}

/// Creates a new broadcast channel, returning a [`Sender`] and a [`Receiver`]
/// without a waker.
///
/// The receiver can later be resubscribed with a waker via
/// [`Receiver::im_resubscribe_with_waker()`], or additional receivers with
/// wakers can be created via [`Sender::im_subscribe_with_waker()`].
#[must_use]
pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>)
where
    T: Clone,
{
    let sender = Sender::new(capacity);
    let receiver = sender.im_subscribe();

    (sender, receiver)
}

/// The sending half of a viewport-aware broadcast channel.
///
/// Wraps a [`tokio::sync::broadcast::Sender<T>`] and derefs to it, so the full
/// Tokio API is available. Use [`im_send()`](Self::im_send) to additionally
/// wake up all viewports that hold a registered [`Receiver`].
pub struct Sender<T> {
    sender: broadcast::Sender<T>,
    wakers: AsyncWakerList,
}

/// A weak sending handle for a viewport-aware broadcast channel.
///
/// Wraps a [`tokio::sync::broadcast::WeakSender<T>`] and derefs to it, so the
/// full Tokio API is available. Use [`im_upgrade()`](Self::im_upgrade) to
/// upgrade it back to a [`Sender`] that supports viewport-aware sending.
pub struct WeakSender<T> {
    sender: broadcast::WeakSender<T>,
    wakers: AsyncWakerList,
}

/// The receiving half of a viewport-aware broadcast channel.
///
/// Wraps a [`tokio::sync::broadcast::Receiver<T>`] and derefs to it, so the
/// full Tokio API is available (e.g.
/// [`recv()`](tokio::sync::broadcast::Receiver::recv)).
///
/// A receiver optionally carries an [`AsyncWaker`] that is registered in
/// the sender's waker list.
pub struct Receiver<T> {
    receiver: broadcast::Receiver<T>,
    inner: WakerRegistration,
}

/// A non-blocking iterator over up to `n` values from a [`Receiver`].
///
/// Yields:
/// - `Some(item)` when an item is available
/// - `None` when the channel is disconnected
///
/// Iteration stops early without yielding when the channel is currently empty.
pub struct ReceiverTake<'a, T> {
    receiver: &'a mut broadcast::Receiver<T>,
    remaining: usize,
    emitted_closed: bool,
}

/// Non-empty receive errors for [`Receiver::im_take`] and
/// [`Receiver::im_take_current`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryRecvError {
    /// The channel is closed and no further values are available.
    Closed,

    /// The receiver lagged too far behind and skipped `u64` messages.
    Lagged(u64),
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            wakers: self.wakers.clone(),
        }
    }
}

impl<T, U> AsRef<U> for Sender<T>
where
    <Self as Deref>::Target: AsRef<U>,
{
    fn as_ref(&self) -> &U {
        self.deref().as_ref()
    }
}

impl<T, U> AsMut<U> for Sender<T>
where
    <Self as Deref>::Target: AsMut<U>,
{
    fn as_mut(&mut self) -> &mut U {
        self.deref_mut().as_mut()
    }
}

impl<T> Deref for Sender<T> {
    type Target = broadcast::Sender<T>;

    fn deref(&self) -> &Self::Target {
        &self.sender
    }
}

impl<T> DerefMut for Sender<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sender
    }
}

impl<T> AsyncWakeUp for Sender<T> {
    fn wake_up(&self) {
        self.wakers.wake_up();
    }
}

impl<T> Sender<T>
where
    T: Clone,
{
    /// Creates a new [`Sender`] for a channel with the given capacity and an
    /// empty waker list.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _receiver) = broadcast::channel(capacity);

        Self {
            sender,
            wakers: AsyncWakerList::default(),
        }
    }

    /// Sends a value via the channel, notifying all receivers.
    ///
    /// # Errors
    ///
    /// Returns a [`SendError`](broadcast::error::SendError) if there are no
    /// active receivers.
    pub fn im_send(&self, value: T) -> Result<usize, broadcast::error::SendError<T>> {
        let result = self.sender.send(value);
        if result.is_ok() {
            self.wakers.wake_up();
        }
        result
    }

    /// Creates a new [`Receiver`] subscribed to this sender, registered with
    /// the given [`AsyncWaker`].
    #[must_use]
    pub fn im_subscribe_with_waker(&self, waker: AsyncWaker) -> Receiver<T> {
        Receiver::new_with_waker(self.sender.subscribe(), self.wakers.clone(), waker)
    }

    /// Creates a new [`Receiver`] subscribed to this sender, without a waker.
    #[must_use]
    pub fn im_subscribe(&self) -> Receiver<T> {
        Receiver::new(self.sender.subscribe(), self.wakers.clone())
    }

    /// Creates a [`WeakSender`] that does not keep the channel alive.
    #[must_use]
    pub fn im_downgrade(&self) -> WeakSender<T> {
        WeakSender {
            sender: self.sender.downgrade(),
            wakers: self.wakers.clone(),
        }
    }
}

impl<T> Clone for WeakSender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            wakers: self.wakers.clone(),
        }
    }
}

impl<T, U> AsRef<U> for WeakSender<T>
where
    <Self as Deref>::Target: AsRef<U>,
{
    fn as_ref(&self) -> &U {
        self.deref().as_ref()
    }
}

impl<T, U> AsMut<U> for WeakSender<T>
where
    <Self as Deref>::Target: AsMut<U>,
{
    fn as_mut(&mut self) -> &mut U {
        self.deref_mut().as_mut()
    }
}

impl<T> Deref for WeakSender<T> {
    type Target = broadcast::WeakSender<T>;

    fn deref(&self) -> &Self::Target {
        &self.sender
    }
}

impl<T> DerefMut for WeakSender<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sender
    }
}

impl<T> WeakSender<T> {
    /// Attempts to upgrade this weak sender to a strong [`Sender`].
    #[must_use]
    pub fn im_upgrade(&self) -> Option<Sender<T>> {
        self.sender.upgrade().map(|sender| Sender {
            sender,
            wakers: self.wakers.clone(),
        })
    }
}

impl<T, U> AsRef<U> for Receiver<T>
where
    <Self as Deref>::Target: AsRef<U>,
{
    fn as_ref(&self) -> &U {
        self.deref().as_ref()
    }
}

impl<T, U> AsMut<U> for Receiver<T>
where
    <Self as Deref>::Target: AsMut<U>,
{
    fn as_mut(&mut self) -> &mut U {
        self.deref_mut().as_mut()
    }
}

impl<T> Deref for Receiver<T> {
    type Target = broadcast::Receiver<T>;

    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

impl<T> DerefMut for Receiver<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.receiver
    }
}

impl<T> Receiver<T> {
    /// Resubscribes this receiver and registers the given [`AsyncWaker`] on
    /// the new receiver.
    #[must_use]
    pub fn im_resubscribe_with_waker(&self, waker: AsyncWaker) -> Self
    where
        T: Clone,
    {
        Self {
            receiver: self.receiver.resubscribe(),
            inner: self.inner.clone_with_waker(waker),
        }
    }

    /// Resubscribes this receiver without registering a waker.
    #[must_use]
    pub fn im_resubscribe(&self) -> Self
    where
        T: Clone,
    {
        Self {
            receiver: self.receiver.resubscribe(),
            inner: self.inner.clone(),
        }
    }

    /// Creates a non-blocking iterator that yields at most `n` elements.
    ///
    /// The iterator uses [`try_recv()`](tokio::sync::broadcast::Receiver::try_recv):
    /// it stops early when the channel is empty and yields
    /// [`TryRecvError`] values for non-empty errors.
    #[must_use]
    pub fn im_take(&mut self, n: usize) -> ReceiverTake<'_, T>
    where
        T: Clone,
    {
        ReceiverTake {
            receiver: &mut self.receiver,
            remaining: n,
            emitted_closed: false,
        }
    }

    /// Creates a non-blocking iterator over currently buffered elements.
    ///
    /// Equivalent to `self.im_take(self.len())`.
    #[must_use]
    pub fn im_take_current(&mut self) -> ReceiverTake<'_, T>
    where
        T: Clone,
    {
        self.im_take(self.receiver.len())
    }

    fn new_with_waker(
        receiver: broadcast::Receiver<T>,
        wakers: AsyncWakerList,
        waker: AsyncWaker,
    ) -> Self {
        Self {
            receiver,
            inner: WakerRegistration::new_with_waker(wakers, waker),
        }
    }

    fn new(receiver: broadcast::Receiver<T>, wakers: AsyncWakerList) -> Self {
        Self {
            receiver,
            inner: WakerRegistration::new(wakers),
        }
    }
}

impl<T> Iterator for ReceiverTake<'_, T>
where
    T: Clone,
{
    type Item = Result<T, TryRecvError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 || self.emitted_closed {
            return None;
        }

        match self.receiver.try_recv() {
            Ok(item) => {
                self.remaining -= 1;
                Some(Ok(item))
            }
            Err(broadcast::error::TryRecvError::Empty) => None,
            Err(broadcast::error::TryRecvError::Closed) => {
                self.emitted_closed = true;
                Some(Err(TryRecvError::Closed))
            }
            Err(broadcast::error::TryRecvError::Lagged(count)) => {
                Some(Err(TryRecvError::Lagged(count)))
            }
        }
    }
}

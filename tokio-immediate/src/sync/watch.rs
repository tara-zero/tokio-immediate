// SPDX-License-Identifier: Apache-2.0 OR MIT

use ::std::ops::{Deref, DerefMut};

use ::tokio::sync::watch;

use crate::sync::waker_registration::WakerRegistration;
use crate::{AsyncGlueWakeUp, AsyncGlueWaker, AsyncGlueWakerList};

/// Creates a new watch channel, returning a [`Sender`] and a [`Receiver`]
/// that is already registered with the given [`AsyncGlueWaker`].
///
/// The receiver's viewport will be woken up whenever a value is sent
/// through any of the `im_send*` methods on the sender.
#[must_use]
pub fn channel_with_waker<T>(init: T, waker: AsyncGlueWaker) -> (Sender<T>, Receiver<T>) {
    let sender = Sender::new(init);
    let receiver = sender.im_subscribe_with_waker(waker);

    (sender, receiver)
}

/// Creates a new watch channel, returning a [`Sender`] and a [`Receiver`]
/// without a waker.
///
/// The receiver can later be cloned with a waker via
/// [`Receiver::im_clone_with_waker()`], or additional receivers with wakers
/// can be created via [`Sender::im_subscribe_with_waker()`].
#[must_use]
pub fn channel<T>(init: T) -> (Sender<T>, Receiver<T>) {
    let sender = Sender::new(init);
    let receiver = sender.im_subscribe();

    (sender, receiver)
}

/// The sending half of a viewport-aware watch channel.
///
/// Wraps a [`tokio::sync::watch::Sender<T>`] and derefs to it, so the
/// full Tokio API is available. Use the `im_send*` methods instead of the
/// underlying `send*` methods to additionally wake up all viewports that
/// hold a registered [`Receiver`].
pub struct Sender<T> {
    sender: watch::Sender<T>,
    wakers: AsyncGlueWakerList,
}

/// The receiving half of a viewport-aware watch channel.
///
/// Wraps a [`tokio::sync::watch::Receiver<T>`] and derefs to it, so the
/// full Tokio API is available (e.g. [`borrow()`](tokio::sync::watch::Receiver::borrow)).
///
/// A receiver optionally carries an [`AsyncGlueWaker`] that is registered
/// in the sender's waker list. Cloning a receiver via [`Clone`] produces a
/// copy **without** a waker; use [`im_clone_with_waker()`](Self::im_clone_with_waker)
/// to get a clone that wakes a specific viewport.
pub struct Receiver<T> {
    receiver: watch::Receiver<T>,
    inner: WakerRegistration,
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
    type Target = watch::Sender<T>;

    fn deref(&self) -> &Self::Target {
        &self.sender
    }
}

impl<T> DerefMut for Sender<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sender
    }
}

impl<T> AsyncGlueWakeUp for Sender<T> {
    fn wake_up(&self) {
        self.wakers.wake_up();
    }
}

impl<T> Sender<T> {
    /// Creates a new [`Sender`] with the given initial value and an empty
    /// waker list.
    #[must_use]
    pub fn new(init: T) -> Self {
        Self {
            sender: watch::Sender::new(init),
            wakers: AsyncGlueWakerList::default(),
        }
    }

    /// Sends a new value via the channel, notifying all receivers.
    ///
    /// # Errors
    ///
    /// Returns a [`SendError`](watch::error::SendError) if there are no
    /// active receivers.
    pub fn im_send(&self, value: T) -> Result<(), watch::error::SendError<T>> {
        let result = self.sender.send(value);
        if result.is_ok() {
            self.wakers.wake_up();
        }
        result
    }

    /// Modifies the watched value unconditionally, notifying all receivers.
    pub fn im_send_modify<F>(&self, modify: F)
    where
        F: FnOnce(&mut T),
    {
        self.sender.send_modify(modify);
        self.wakers.wake_up();
    }

    /// Modifies the watched value conditionally, notifying all receivers if modified.
    pub fn im_send_if_modified<F>(&self, modify: F) -> bool
    where
        F: FnOnce(&mut T) -> bool,
    {
        let modified = self.sender.send_if_modified(modify);
        if modified {
            self.wakers.wake_up();
        }
        modified
    }

    /// Replaces the watched value, returning the old value and notifying all receivers.
    pub fn im_send_replace(&self, value: T) -> T {
        let old_value = self.sender.send_replace(value);
        self.wakers.wake_up();
        old_value
    }

    /// Creates a new [`Receiver`] subscribed to this sender, registered
    /// with the given [`AsyncGlueWaker`].
    #[must_use]
    pub fn im_subscribe_with_waker(&self, waker: AsyncGlueWaker) -> Receiver<T> {
        Receiver::new_with_waker(self.sender.subscribe(), self.wakers.clone(), waker)
    }

    /// Creates a new [`Receiver`] subscribed to this sender, without a
    /// waker.
    #[must_use]
    pub fn im_subscribe(&self) -> Receiver<T> {
        Receiver::new(self.sender.subscribe(), self.wakers.clone())
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        Self {
            receiver: self.receiver.clone(),
            inner: self.inner.clone(),
        }
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
    type Target = watch::Receiver<T>;

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
    /// Clones this receiver and registers the given [`AsyncGlueWaker`] on
    /// the new copy.
    ///
    /// Unlike [`Clone::clone()`], the returned receiver will wake its
    /// viewport when the sender uses an `im_send*` method.
    #[must_use]
    pub fn im_clone_with_waker(&self, waker: AsyncGlueWaker) -> Self {
        Self {
            receiver: self.receiver.clone(),
            inner: self.inner.clone_with_waker(waker),
        }
    }

    fn new_with_waker(
        receiver: watch::Receiver<T>,
        wakers: AsyncGlueWakerList,
        waker: AsyncGlueWaker,
    ) -> Self {
        Self {
            receiver,
            inner: WakerRegistration::new_with_waker(wakers, waker),
        }
    }

    fn new(receiver: watch::Receiver<T>, wakers: AsyncGlueWakerList) -> Self {
        Self {
            receiver,
            inner: WakerRegistration::new(wakers),
        }
    }
}

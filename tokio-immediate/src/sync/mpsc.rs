// SPDX-License-Identifier: Apache-2.0 OR MIT

use ::std::ops::{Deref, DerefMut};

use ::tokio::sync::mpsc;

use crate::sync::waker_binding::WakerBinding;
use crate::{AsyncGlueWakeUp, AsyncGlueWaker};

/// Creates a new mpsc channel, returning a [`Sender`] and a [`Receiver`]
/// that is already bound to the given [`AsyncGlueWaker`].
///
/// The receiver's viewport will be woken up whenever a value is sent through
/// [`Sender::im_send`] or [`Sender::im_try_send`] successfully.
#[must_use]
pub fn channel_with_waker<T>(capacity: usize, waker: AsyncGlueWaker) -> (Sender<T>, Receiver<T>) {
    let (sender, receiver) = mpsc::channel(capacity);
    let binding = WakerBinding::default();
    binding.set_waker(waker);

    (
        Sender {
            sender,
            binding: binding.clone(),
        },
        Receiver { receiver, binding },
    )
}

/// Creates a new mpsc channel, returning a [`Sender`] and a [`Receiver`]
/// without a viewport binding.
///
/// You can later bind the receiver to a viewport via
/// [`Receiver::im_set_waker`].
#[must_use]
pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let (sender, receiver) = mpsc::channel(capacity);
    let binding = WakerBinding::default();

    (
        Sender {
            sender,
            binding: binding.clone(),
        },
        Receiver { receiver, binding },
    )
}

/// Creates a new unbounded mpsc channel, returning an [`UnboundedSender`]
/// and an [`UnboundedReceiver`] that is already bound to the given
/// [`AsyncGlueWaker`].
///
/// The receiver's viewport will be woken up whenever a value is sent through
/// [`UnboundedSender::im_send`] successfully.
#[must_use]
pub fn unbounded_channel_with_waker<T>(
    waker: AsyncGlueWaker,
) -> (UnboundedSender<T>, UnboundedReceiver<T>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let binding = WakerBinding::default();
    binding.set_waker(waker);

    (
        UnboundedSender {
            sender,
            binding: binding.clone(),
        },
        UnboundedReceiver { receiver, binding },
    )
}

/// Creates a new unbounded mpsc channel, returning an [`UnboundedSender`]
/// and an [`UnboundedReceiver`] without a viewport binding.
///
/// You can later bind the receiver to a viewport via
/// [`UnboundedReceiver::im_set_waker`].
#[must_use]
pub fn unbounded_channel<T>() -> (UnboundedSender<T>, UnboundedReceiver<T>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let binding = WakerBinding::default();

    (
        UnboundedSender {
            sender,
            binding: binding.clone(),
        },
        UnboundedReceiver { receiver, binding },
    )
}

/// The sending half of a viewport-aware mpsc channel.
///
/// Wraps a [`tokio::sync::mpsc::Sender<T>`] and derefs to it, so the full
/// Tokio API is available. Use [`im_send()`](Self::im_send) and
/// [`im_try_send()`](Self::im_try_send) to additionally wake up the bound
/// receiver viewport.
pub struct Sender<T> {
    sender: mpsc::Sender<T>,
    binding: WakerBinding,
}

/// A weak sending handle for a viewport-aware mpsc channel.
///
/// Wraps a [`tokio::sync::mpsc::WeakSender<T>`] and derefs to it, so the full
/// Tokio API is available. Use [`im_upgrade()`](Self::im_upgrade) to
/// upgrade it back to a [`Sender`].
pub struct WeakSender<T> {
    sender: mpsc::WeakSender<T>,
    binding: WakerBinding,
}

/// The receiving half of a viewport-aware mpsc channel.
///
/// Wraps a [`tokio::sync::mpsc::Receiver<T>`] and derefs to it, so the full
/// Tokio API is available (e.g. [`recv()`](tokio::sync::mpsc::Receiver::recv)).
///
/// Unlike broadcast/watch channels, mpsc has exactly one receiver, so there is
/// only one viewport binding. Rebind it via [`im_set_waker()`](Self::im_set_waker)
/// or remove it via [`im_clear_waker()`](Self::im_clear_waker).
pub struct Receiver<T> {
    receiver: mpsc::Receiver<T>,
    binding: WakerBinding,
}

/// The sending half of a viewport-aware unbounded mpsc channel.
///
/// Wraps a [`tokio::sync::mpsc::UnboundedSender<T>`] and derefs to it, so the
/// full Tokio API is available. Use [`im_send()`](Self::im_send) to
/// additionally wake up the bound receiver viewport.
pub struct UnboundedSender<T> {
    sender: mpsc::UnboundedSender<T>,
    binding: WakerBinding,
}

/// A weak sending handle for a viewport-aware unbounded mpsc channel.
///
/// Wraps a [`tokio::sync::mpsc::WeakUnboundedSender<T>`] and derefs to it, so
/// the full Tokio API is available. Use [`im_upgrade()`](Self::im_upgrade) to
/// upgrade it back to an [`UnboundedSender`].
pub struct WeakUnboundedSender<T> {
    sender: mpsc::WeakUnboundedSender<T>,
    binding: WakerBinding,
}

/// The receiving half of a viewport-aware unbounded mpsc channel.
///
/// Wraps a [`tokio::sync::mpsc::UnboundedReceiver<T>`] and derefs to it, so the
/// full Tokio API is available (e.g.
/// [`recv()`](tokio::sync::mpsc::UnboundedReceiver::recv)).
///
/// Since unbounded mpsc has exactly one receiver, there is only one viewport
/// binding. Rebind it via [`im_set_waker()`](Self::im_set_waker) or remove it
/// via [`im_clear_waker()`](Self::im_clear_waker).
pub struct UnboundedReceiver<T> {
    receiver: mpsc::UnboundedReceiver<T>,
    binding: WakerBinding,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            binding: self.binding.clone(),
        }
    }
}

impl<T> Clone for WeakSender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            binding: self.binding.clone(),
        }
    }
}

impl<T> Clone for UnboundedSender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            binding: self.binding.clone(),
        }
    }
}

impl<T> Clone for WeakUnboundedSender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            binding: self.binding.clone(),
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
    type Target = mpsc::Sender<T>;

    fn deref(&self) -> &Self::Target {
        &self.sender
    }
}

impl<T> DerefMut for Sender<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sender
    }
}

impl<T> Sender<T> {
    /// Sends a value via the channel, notifying the bound receiver viewport.
    ///
    /// # Errors
    ///
    /// Returns a [`SendError`](mpsc::error::SendError) if the receiver has
    /// been dropped.
    pub async fn im_send(&self, value: T) -> Result<(), mpsc::error::SendError<T>> {
        let result = self.sender.send(value).await;
        if result.is_ok() {
            self.binding.wake_up();
        }
        result
    }

    /// Attempts to send a value immediately, notifying the bound receiver
    /// viewport on success.
    ///
    /// # Errors
    ///
    /// Returns a [`TrySendError`](mpsc::error::TrySendError) when the channel
    /// is full or closed.
    pub fn im_try_send(&self, value: T) -> Result<(), mpsc::error::TrySendError<T>> {
        let result = self.sender.try_send(value);
        if result.is_ok() {
            self.binding.wake_up();
        }
        result
    }

    /// Creates a [`WeakSender`] that does not keep the channel alive.
    #[must_use]
    pub fn im_downgrade(&self) -> WeakSender<T> {
        WeakSender {
            sender: self.sender.downgrade(),
            binding: self.binding.clone(),
        }
    }
}

impl<T> AsyncGlueWakeUp for Sender<T> {
    fn wake_up(&self) {
        self.binding.wake_up();
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
    type Target = mpsc::WeakSender<T>;

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
            binding: self.binding.clone(),
        })
    }
}

impl<T, U> AsRef<U> for UnboundedSender<T>
where
    <Self as Deref>::Target: AsRef<U>,
{
    fn as_ref(&self) -> &U {
        self.deref().as_ref()
    }
}

impl<T, U> AsMut<U> for UnboundedSender<T>
where
    <Self as Deref>::Target: AsMut<U>,
{
    fn as_mut(&mut self) -> &mut U {
        self.deref_mut().as_mut()
    }
}

impl<T> Deref for UnboundedSender<T> {
    type Target = mpsc::UnboundedSender<T>;

    fn deref(&self) -> &Self::Target {
        &self.sender
    }
}

impl<T> DerefMut for UnboundedSender<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sender
    }
}

impl<T> UnboundedSender<T> {
    /// Sends a value via the channel, notifying the bound receiver viewport.
    ///
    /// # Errors
    ///
    /// Returns a [`SendError`](mpsc::error::SendError) if the receiver has
    /// been dropped.
    pub fn im_send(&self, value: T) -> Result<(), mpsc::error::SendError<T>> {
        let result = self.sender.send(value);
        if result.is_ok() {
            self.binding.wake_up();
        }
        result
    }

    /// Creates a [`WeakUnboundedSender`] that does not keep the channel alive.
    #[must_use]
    pub fn im_downgrade(&self) -> WeakUnboundedSender<T> {
        WeakUnboundedSender {
            sender: self.sender.downgrade(),
            binding: self.binding.clone(),
        }
    }
}

impl<T> AsyncGlueWakeUp for UnboundedSender<T> {
    fn wake_up(&self) {
        self.binding.wake_up();
    }
}

impl<T, U> AsRef<U> for WeakUnboundedSender<T>
where
    <Self as Deref>::Target: AsRef<U>,
{
    fn as_ref(&self) -> &U {
        self.deref().as_ref()
    }
}

impl<T, U> AsMut<U> for WeakUnboundedSender<T>
where
    <Self as Deref>::Target: AsMut<U>,
{
    fn as_mut(&mut self) -> &mut U {
        self.deref_mut().as_mut()
    }
}

impl<T> Deref for WeakUnboundedSender<T> {
    type Target = mpsc::WeakUnboundedSender<T>;

    fn deref(&self) -> &Self::Target {
        &self.sender
    }
}

impl<T> DerefMut for WeakUnboundedSender<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sender
    }
}

impl<T> WeakUnboundedSender<T> {
    /// Attempts to upgrade this weak sender to a strong [`UnboundedSender`].
    #[must_use]
    pub fn im_upgrade(&self) -> Option<UnboundedSender<T>> {
        self.sender.upgrade().map(|sender| UnboundedSender {
            sender,
            binding: self.binding.clone(),
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
    type Target = mpsc::Receiver<T>;

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
    /// Binds this receiver to a viewport waker.
    ///
    /// Future successful `im_send*` calls on associated senders will request a
    /// wake-up of this viewport.
    pub fn im_set_waker(&self, waker: AsyncGlueWaker) {
        self.binding.set_waker(waker);
    }

    /// Clears the current viewport binding, if any.
    pub fn im_clear_waker(&self) {
        self.binding.clear_waker();
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.binding.clear_waker();
    }
}

impl<T, U> AsRef<U> for UnboundedReceiver<T>
where
    <Self as Deref>::Target: AsRef<U>,
{
    fn as_ref(&self) -> &U {
        self.deref().as_ref()
    }
}

impl<T, U> AsMut<U> for UnboundedReceiver<T>
where
    <Self as Deref>::Target: AsMut<U>,
{
    fn as_mut(&mut self) -> &mut U {
        self.deref_mut().as_mut()
    }
}

impl<T> Deref for UnboundedReceiver<T> {
    type Target = mpsc::UnboundedReceiver<T>;

    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

impl<T> DerefMut for UnboundedReceiver<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.receiver
    }
}

impl<T> UnboundedReceiver<T> {
    /// Binds this receiver to a viewport waker.
    ///
    /// Future successful [`UnboundedSender::im_send`] calls on associated
    /// senders will request a wake-up of this viewport.
    pub fn im_set_waker(&self, waker: AsyncGlueWaker) {
        self.binding.set_waker(waker);
    }

    /// Clears the current viewport binding, if any.
    pub fn im_clear_waker(&self) {
        self.binding.clear_waker();
    }
}

impl<T> Drop for UnboundedReceiver<T> {
    fn drop(&mut self) {
        self.binding.clear_waker();
    }
}

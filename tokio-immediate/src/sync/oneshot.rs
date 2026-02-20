// SPDX-License-Identifier: Apache-2.0 OR MIT

use ::std::future::Future;
use ::std::ops::{Deref, DerefMut};
use ::std::pin::Pin;
use ::std::task::{Context, Poll};

use ::tokio::sync::oneshot;

use crate::sync::waker_binding::WakerBinding;
use crate::{AsyncGlueWakeUp, AsyncGlueWaker};

/// Creates a new oneshot channel, returning a [`Sender`] and a [`Receiver`]
/// that is already bound to the given [`AsyncGlueWaker`].
///
/// The receiver's viewport will be woken up whenever a value is sent through
/// [`Sender::im_send`] successfully.
#[must_use]
pub fn channel_with_waker<T>(waker: AsyncGlueWaker) -> (Sender<T>, Receiver<T>) {
    let (sender, receiver) = oneshot::channel();
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

/// Creates a new oneshot channel, returning a [`Sender`] and a [`Receiver`]
/// without a viewport binding.
///
/// You can later bind the receiver to a viewport via
/// [`Receiver::im_set_waker`].
#[must_use]
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let (sender, receiver) = oneshot::channel();
    let binding = WakerBinding::default();

    (
        Sender {
            sender,
            binding: binding.clone(),
        },
        Receiver { receiver, binding },
    )
}

/// The sending half of a viewport-aware oneshot channel.
///
/// Wraps a [`tokio::sync::oneshot::Sender<T>`] and derefs to it, so the full
/// Tokio API is available. Use [`im_send()`](Self::im_send) to additionally
/// wake up the bound receiver viewport.
pub struct Sender<T> {
    sender: oneshot::Sender<T>,
    binding: WakerBinding,
}

/// The receiving half of a viewport-aware oneshot channel.
///
/// Wraps a [`tokio::sync::oneshot::Receiver<T>`], derefs to it for access to
/// methods like [`try_recv()`](tokio::sync::oneshot::Receiver::try_recv), and
/// can also be directly awaited like the Tokio receiver.
///
/// Since oneshot has exactly one receiver, there is only one viewport binding.
/// Rebind it via [`im_set_waker()`](Self::im_set_waker) or remove it via
/// [`im_clear_waker()`](Self::im_clear_waker).
pub struct Receiver<T> {
    receiver: oneshot::Receiver<T>,
    binding: WakerBinding,
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
    type Target = oneshot::Sender<T>;

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
        self.binding.wake_up();
    }
}

impl<T> Sender<T> {
    /// Sends a value through this oneshot channel, notifying the bound
    /// receiver viewport.
    ///
    /// # Errors
    ///
    /// Returns the original value if the receiver has been dropped.
    pub fn im_send(self, value: T) -> Result<(), T> {
        let result = self.sender.send(value);
        if result.is_ok() {
            self.binding.wake_up();
        }
        result
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
    type Target = oneshot::Receiver<T>;

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
    /// Future successful [`Sender::im_send`] calls will request a wake-up of
    /// this viewport.
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
        // Senders has nothing to wake up as this type of channel is single-receiver.
        self.binding.clear_waker();
    }
}

impl<T> Future for Receiver<T> {
    type Output = Result<T, oneshot::error::RecvError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.receiver).poll(context)
    }
}

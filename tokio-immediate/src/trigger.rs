// SPDX-License-Identifier: Apache-2.0 OR MIT

use ::std::future::pending;

use ::tokio::sync::watch;

/// Creates a new trigger channel, returning the handle and trigger pair.
///
/// The returned [`AsyncTrigger`] is initially in the triggered state, so
/// the first call to [`triggered()`](AsyncTrigger::triggered) will
/// complete immediately.
#[must_use]
pub fn channel() -> (AsyncTriggerHandle, AsyncTrigger) {
    let handle = AsyncTriggerHandle::default();
    let trigger = handle.subscribe();

    (handle, trigger)
}

/// The sending (notifying) side of a trigger channel.
///
/// Use [`trigger()`](Self::trigger) to notify all associated
/// [`AsyncTrigger`] receivers that an event of interest has occurred and
/// they should take action.
///
/// Multiple [`AsyncTrigger`] receivers can be created from a single handle
/// via [`subscribe()`](Self::subscribe). Cloning the handle creates another
/// handle backed by the same underlying channel.
#[derive(Clone)]
pub struct AsyncTriggerHandle {
    sender: watch::Sender<()>,
}

/// The receiving (waiting) side of a trigger channel.
///
/// An `AsyncTrigger` can asynchronously wait for a notification sent by the
/// associated [`AsyncTriggerHandle`]. It is typically used inside an async
/// task's `select!` loop to know when it should take some action.
///
/// Newly created or cloned triggers start in the triggered state so that the
/// owning task performs an initial action before waiting for further
/// notifications.
pub struct AsyncTrigger {
    receiver: watch::Receiver<()>,
}

impl Default for AsyncTriggerHandle {
    fn default() -> Self {
        Self {
            sender: watch::Sender::new(()),
        }
    }
}

impl AsyncTriggerHandle {
    /// Creates a new [`AsyncTrigger`] that listens for notifications from
    /// this handle.
    ///
    /// The returned trigger is initially in the triggered state, so its first
    /// call to [`triggered()`](AsyncTrigger::triggered) will complete
    /// immediately. This ensures the receiving task performs an initial action
    /// right away.
    #[must_use]
    pub fn subscribe(&self) -> AsyncTrigger {
        let mut receiver = self.sender.subscribe();
        receiver.mark_changed();
        AsyncTrigger { receiver }
    }

    /// Sends a trigger notification to all associated [`AsyncTrigger`]
    /// receivers.
    pub fn trigger(&self) {
        self.sender.send_replace(());
    }

    /// Returns `true` if `self` and `other` refer to the same underlying
    /// trigger channel.
    #[must_use]
    pub fn same_trigger(&self, other: &Self) -> bool {
        self.sender.same_channel(&other.sender)
    }
}

impl Clone for AsyncTrigger {
    fn clone(&self) -> Self {
        let mut receiver = self.receiver.clone();
        receiver.mark_changed();
        Self { receiver }
    }
}

impl AsyncTrigger {
    /// Returns `true` if the trigger has been activated since the last call to
    /// [`triggered()`](Self::triggered) or [`mark_not_triggered()`](Self::mark_not_triggered).
    ///
    /// This is a non-blocking check. If all associated
    /// [`AsyncTriggerHandle`] have been dropped, this returns `false`.
    #[must_use]
    pub fn has_triggered(&self) -> bool {
        self.receiver.has_changed().unwrap_or(false)
    }

    /// Manually marks this trigger as triggered without waiting for a
    /// notification from the handle.
    ///
    /// The next call to [`triggered()`](Self::triggered) will complete
    /// immediately.
    pub fn mark_triggered(&mut self) {
        self.receiver.mark_changed();
    }

    /// Marks this trigger as not triggered, clearing any pending notification.
    ///
    /// After calling this, [`has_triggered()`](Self::has_triggered) will return
    /// `false` until the next notification is sent from the handle.
    pub fn mark_not_triggered(&mut self) {
        self.receiver.mark_unchanged();
    }

    /// Asynchronously waits until the trigger is activated.
    ///
    /// If all associated [`AsyncTriggerHandle`] have been dropped (meaning
    /// no further triggers can ever arrive), this future will wait forever.
    pub async fn triggered(&mut self) {
        if self.receiver.changed().await.is_err() {
            pending::<()>().await;
        }
    }

    /// Returns `true` if `self` and `other` are associated with the same
    /// underlying trigger channel.
    #[must_use]
    pub fn same_trigger(&self, other: &Self) -> bool {
        self.receiver.same_channel(&other.receiver)
    }
}

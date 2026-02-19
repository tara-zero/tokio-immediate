// SPDX-License-Identifier: Apache-2.0 OR MIT

use ::std::sync::Arc;
use ::std::sync::atomic::{AtomicUsize, Ordering};

use ::tokio_immediate::AsyncGlueViewport;
use ::tokio_immediate::sync::mpsc;

#[tokio::test]
async fn channel_with_waker_wakes_on_im_send() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, mut receiver) = mpsc::channel_with_waker(4, viewport.new_waker());

    sender
        .im_send(10_u32)
        .await
        .expect("send should succeed while receiver is alive");
    assert_eq!(receiver.recv().await, Some(10));
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn receiver_can_set_waker_after_channel_creation() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, receiver) = mpsc::channel::<u32>(4);

    sender
        .im_try_send(1)
        .expect("send should succeed without a waker bound");
    assert_eq!(wake_count.load(Ordering::Relaxed), 0);

    receiver.im_set_waker(viewport.new_waker());
    viewport.woke_up();

    sender
        .im_try_send(2)
        .expect("send should succeed with a waker bound");
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn receiver_can_clear_waker() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, receiver) = mpsc::channel_with_waker::<u32>(4, viewport.new_waker());

    sender
        .im_try_send(1)
        .expect("send should succeed with initial waker");
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);

    receiver.im_clear_waker();
    viewport.woke_up();

    sender
        .im_try_send(2)
        .expect("send should still succeed after clearing waker");
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn unbounded_channel_with_waker_wakes_on_im_send() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, mut receiver) = mpsc::unbounded_channel_with_waker::<u32>(viewport.new_waker());

    sender
        .im_send(10_u32)
        .expect("send should succeed while receiver is alive");
    assert_eq!(receiver.recv().await, Some(10));
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn unbounded_receiver_can_set_waker_after_channel_creation() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, receiver) = mpsc::unbounded_channel::<u32>();

    sender
        .im_send(1)
        .expect("send should succeed without a waker bound");
    assert_eq!(wake_count.load(Ordering::Relaxed), 0);

    receiver.im_set_waker(viewport.new_waker());
    viewport.woke_up();

    sender
        .im_send(2)
        .expect("send should succeed with a waker bound");
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn unbounded_receiver_can_clear_waker() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, receiver) = mpsc::unbounded_channel_with_waker::<u32>(viewport.new_waker());

    sender
        .im_send(1)
        .expect("send should succeed with initial waker");
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);

    receiver.im_clear_waker();
    viewport.woke_up();

    sender
        .im_send(2)
        .expect("send should still succeed after clearing waker");
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn unbounded_failed_send_does_not_wake() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, receiver) = mpsc::unbounded_channel_with_waker::<u32>(viewport.new_waker());
    drop(receiver);

    let result = sender.im_send(1);
    assert!(
        result.is_err(),
        "send should fail after receiver is dropped"
    );
    if let Err(error) = result {
        assert_eq!(error.0, 1);
    }
    assert_eq!(wake_count.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn weak_sender_upgrade_preserves_im_send_wake_behavior() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, mut receiver) = mpsc::channel_with_waker(4, viewport.new_waker());
    let weak_sender = sender.im_downgrade();

    let upgraded_sender = weak_sender
        .im_upgrade()
        .expect("upgrade should succeed while a strong sender exists");

    upgraded_sender
        .im_send(10_u32)
        .await
        .expect("send should succeed while receiver is alive");
    assert_eq!(receiver.recv().await, Some(10));
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn weak_sender_upgrade_fails_without_strong_senders() {
    let (sender, _receiver) = mpsc::channel::<u32>(4);
    let weak_sender = sender.im_downgrade();
    drop(sender);

    assert!(
        weak_sender.im_upgrade().is_none(),
        "upgrade should fail once all strong senders are dropped"
    );
}

#[tokio::test]
async fn weak_unbounded_sender_upgrade_preserves_im_send_wake_behavior() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, mut receiver) = mpsc::unbounded_channel_with_waker::<u32>(viewport.new_waker());
    let weak_sender = sender.im_downgrade();

    let upgraded_sender = weak_sender
        .im_upgrade()
        .expect("upgrade should succeed while a strong sender exists");

    upgraded_sender
        .im_send(10_u32)
        .expect("send should succeed while receiver is alive");
    assert_eq!(receiver.recv().await, Some(10));
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn weak_unbounded_sender_upgrade_fails_without_strong_senders() {
    let (sender, _receiver) = mpsc::unbounded_channel::<u32>();
    let weak_sender = sender.im_downgrade();
    drop(sender);

    assert!(
        weak_sender.im_upgrade().is_none(),
        "upgrade should fail once all strong senders are dropped"
    );
}

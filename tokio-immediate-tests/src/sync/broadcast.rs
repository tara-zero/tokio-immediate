// SPDX-License-Identifier: Apache-2.0 OR MIT

use ::std::sync::Arc;
use ::std::sync::atomic::{AtomicUsize, Ordering};

use ::tokio_immediate::AsyncGlueViewport;
use ::tokio_immediate::sync::broadcast;

#[tokio::test]
async fn channel_with_waker_wakes_on_im_send() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, mut receiver) = broadcast::channel_with_waker(4, viewport.new_waker());

    sender
        .im_send(10_u32)
        .expect("send should succeed while receiver is alive");
    assert_eq!(
        receiver
            .recv()
            .await
            .expect("receive should succeed while sender is alive"),
        10_u32
    );
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn failed_send_does_not_wake() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, receiver) = broadcast::channel_with_waker::<u32>(4, viewport.new_waker());
    drop(receiver);

    let result = sender.im_send(1_u32);
    assert!(
        result.is_err(),
        "send should fail after receiver is dropped"
    );
    if let Err(error) = result {
        assert_eq!(error.0, 1_u32);
    }
    assert_eq!(wake_count.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn resubscribe_with_waker_wakes_after_plain_channel_creation() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, mut receiver) = broadcast::channel::<u32>(4);

    sender
        .im_send(1_u32)
        .expect("send should succeed while receiver is alive");
    assert_eq!(wake_count.load(Ordering::Relaxed), 0);

    let mut receiver_with_waker = receiver.im_resubscribe_with_waker(viewport.new_waker());
    let _ = receiver.recv().await;

    sender
        .im_send(2_u32)
        .expect("send should succeed while receiver is alive");
    assert_eq!(
        receiver_with_waker
            .recv()
            .await
            .expect("receive should succeed"),
        2_u32
    );
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn resubscribe_without_waker_does_not_wake() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, receiver) = broadcast::channel_with_waker::<u32>(4, viewport.new_waker());
    let mut receiver_without_waker = receiver.im_resubscribe();
    drop(receiver);
    viewport.woke_up();

    sender
        .im_send(1_u32)
        .expect("send should succeed while receiver is alive");
    assert_eq!(
        receiver_without_waker
            .recv()
            .await
            .expect("receive should succeed"),
        1_u32
    );
    assert_eq!(wake_count.load(Ordering::Relaxed), 0);
}

#[test]
fn resubscribe_with_waker_unregisters_on_drop() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, receiver) = broadcast::channel::<u32>(4);

    let receiver_with_waker = receiver.im_resubscribe_with_waker(viewport.new_waker());
    sender
        .im_send(1_u32)
        .expect("send should succeed while receiver is alive");
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);

    drop(receiver_with_waker);
    viewport.woke_up();

    sender
        .im_send(2_u32)
        .expect("send should succeed while receiver is alive");
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

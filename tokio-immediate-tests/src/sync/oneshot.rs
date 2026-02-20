// SPDX-License-Identifier: Apache-2.0 OR MIT

use ::std::sync::Arc;
use ::std::sync::atomic::{AtomicUsize, Ordering};

use ::tokio_immediate::AsyncViewport;
use ::tokio_immediate::sync::oneshot;

#[tokio::test]
async fn channel_with_waker_wakes_on_im_send() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, receiver) = oneshot::channel_with_waker::<u32>(viewport.new_waker());

    sender
        .im_send(10_u32)
        .expect("send should succeed while receiver is alive");
    assert_eq!(receiver.await, Ok(10));
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn receiver_can_set_waker_after_channel_creation() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, receiver) = oneshot::channel::<u32>();
    receiver.im_set_waker(viewport.new_waker());

    sender
        .im_send(1_u32)
        .expect("send should succeed with a waker bound");
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn receiver_can_clear_waker() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, receiver) = oneshot::channel_with_waker::<u32>(viewport.new_waker());
    receiver.im_clear_waker();

    sender
        .im_send(2_u32)
        .expect("send should succeed after clearing waker");
    assert_eq!(wake_count.load(Ordering::Relaxed), 0);
}

#[test]
fn failed_send_does_not_wake() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncViewport::new_with_wake_up({
        let wake_count = wake_count.clone();
        Arc::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        })
    });

    let (sender, receiver) = oneshot::channel_with_waker::<u32>(viewport.new_waker());
    drop(receiver);

    assert_eq!(sender.im_send(3_u32), Err(3));
    assert_eq!(wake_count.load(Ordering::Relaxed), 0);
}

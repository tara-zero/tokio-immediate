// SPDX-License-Identifier: Apache-2.0 OR MIT

use ::std::sync::Arc;
use ::std::sync::atomic::{AtomicUsize, Ordering};

use ::tokio_immediate::AsyncGlueViewport;
use ::tokio_immediate::sync::watch;

#[test]
fn channel_with_waker_wakes_on_im_send() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new({
        let wake_count = wake_count.clone();
        move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        }
    });

    let (sender, mut receiver) = watch::channel_with_waker(0_u32, viewport.new_waker());

    sender
        .im_send(10_u32)
        .expect("send should succeed while receiver is alive");
    assert_eq!(*receiver.borrow_and_update(), 10);
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn failed_send_does_not_wake() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new({
        let wake_count = wake_count.clone();
        move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        }
    });

    let (sender, receiver) = watch::channel_with_waker(0_u32, viewport.new_waker());
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

#[test]
fn im_send_modify_wakes_and_updates_value() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new({
        let wake_count = wake_count.clone();
        move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        }
    });

    let (sender, mut receiver) = watch::channel_with_waker(5_u32, viewport.new_waker());

    sender.im_send_modify(|value| {
        *value += 1;
    });
    assert_eq!(*receiver.borrow_and_update(), 6);
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn im_send_if_modified_only_wakes_when_modified() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new({
        let wake_count = wake_count.clone();
        move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        }
    });

    let (sender, mut receiver) = watch::channel_with_waker(5_u32, viewport.new_waker());

    let modified = sender.im_send_if_modified(|_value| false);
    assert!(!modified);
    assert_eq!(wake_count.load(Ordering::Relaxed), 0);

    let modified = sender.im_send_if_modified(|value| {
        *value = 42;
        true
    });
    assert!(modified);
    assert_eq!(*receiver.borrow_and_update(), 42);
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn im_send_replace_wakes_and_returns_old_value() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new({
        let wake_count = wake_count.clone();
        move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        }
    });

    let (sender, mut receiver) = watch::channel_with_waker(7_u32, viewport.new_waker());

    let old_value = sender.im_send_replace(9_u32);
    assert_eq!(old_value, 7_u32);
    assert_eq!(*receiver.borrow_and_update(), 9);
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

#[test]
fn clone_without_waker_does_not_wake() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let _viewport = AsyncGlueViewport::new({
        let wake_count = wake_count.clone();
        move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        }
    });

    let (sender, receiver) = watch::channel(0_u32);
    let _cloned_receiver = receiver.clone();

    sender.im_send_replace(1_u32);
    assert_eq!(wake_count.load(Ordering::Relaxed), 0);
}

#[test]
fn clone_with_waker_wakes_and_unregisters_on_drop() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let viewport = AsyncGlueViewport::new({
        let wake_count = wake_count.clone();
        move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        }
    });

    let (sender, receiver) = watch::channel(0_u32);

    let receiver_with_waker = receiver.im_clone_with_waker(viewport.new_waker());
    sender.im_send_replace(1_u32);
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);

    drop(receiver_with_waker);
    viewport.woke_up();

    sender.im_send_replace(2_u32);
    assert_eq!(wake_count.load(Ordering::Relaxed), 1);
}

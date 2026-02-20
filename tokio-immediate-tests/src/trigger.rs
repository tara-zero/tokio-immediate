// SPDX-License-Identifier: Apache-2.0 OR MIT

use ::std::time::Duration;

use ::tokio::time::timeout;
use ::tokio_immediate::trigger;

const SHORT_TIMEOUT: Duration = Duration::from_millis(50);

#[test]
fn subscribe_creates_initially_triggered_trigger() {
    let handle = trigger::AsyncTriggerHandle::default();
    let trigger = handle.subscribe();
    assert!(trigger.has_triggered());
}

#[test]
fn has_triggered_returns_false_after_handle_dropped_and_cleared() {
    let (handle, mut trigger) = trigger::channel();
    trigger.mark_not_triggered();
    drop(handle);
    assert!(!trigger.has_triggered());
}

#[test]
fn handle_trigger_sets_triggered_state() {
    let (handle, mut trigger) = trigger::channel();
    trigger.mark_not_triggered();
    assert!(!trigger.has_triggered());

    handle.trigger();
    assert!(trigger.has_triggered());
}

#[test]
fn clone_starts_triggered() {
    let (_handle, mut trigger) = trigger::channel();
    trigger.mark_not_triggered();

    let cloned = trigger.clone();
    assert!(cloned.has_triggered());
    // Original is unaffected.
    assert!(!trigger.has_triggered());
}

#[tokio::test]
async fn triggered_completes_immediately_when_already_triggered() {
    let (_handle, mut trigger) = trigger::channel();
    assert!(trigger.has_triggered());

    // Must complete within the short timeout.
    timeout(SHORT_TIMEOUT, trigger.triggered())
        .await
        .expect("triggered() should complete immediately when already triggered");
}

#[tokio::test]
async fn triggered_hangs_forever_after_handle_dropped() {
    let (handle, mut trigger) = trigger::channel();

    // Consume the initial trigger.
    trigger.mark_not_triggered();
    drop(handle);

    // Should never complete.
    let result = timeout(SHORT_TIMEOUT, trigger.triggered()).await;
    assert!(
        result.is_err(),
        "triggered() should hang forever after handle is dropped"
    );
}

#[test]
fn drop_trigger_while_handle_alive() {
    let (handle, trigger) = trigger::channel();
    drop(trigger);

    // Handle can still trigger without panic.
    handle.trigger();
}

// SPDX-License-Identifier: Apache-2.0 OR MIT

// TODO: Most of the tests are LLM-generated. Needs review.

#![warn(clippy::undocumented_unsafe_blocks)]

#[cfg(test)]
mod common;
#[cfg(test)]
mod parallel;
#[cfg(all(test, feature = "sync"))]
mod serial;
#[cfg(test)]
mod single;
#[cfg(all(test, feature = "sync"))]
mod sync;
#[cfg(all(test, feature = "sync"))]
mod trigger;

#[cfg(test)]
mod tests {
    use ::std::sync::Arc;
    use ::std::sync::atomic::{AtomicUsize, Ordering};
    use ::tokio_immediate::{AsyncViewport, AsyncWakeUp, AsyncWakerList};

    #[test]
    fn waker_requires_woke_up_reset_before_second_callback() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let viewport = AsyncViewport::new_with_wake_up({
            let wake_count = wake_count.clone();
            Arc::new(move || {
                wake_count.fetch_add(1, Ordering::Relaxed);
            })
        });
        let waker = viewport.new_waker();

        waker.wake_up();
        assert_eq!(wake_count.load(Ordering::Relaxed), 1);

        waker.wake_up();
        assert_eq!(wake_count.load(Ordering::Relaxed), 1);

        viewport.woke_up();
        waker.wake_up();
        assert_eq!(wake_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn dropped_viewport_makes_waker_inactive() {
        let viewport = AsyncViewport::default();
        let waker = viewport.new_waker();

        assert!(waker.is_alive());
        drop(viewport);

        assert!(!waker.is_alive());
    }

    #[test]
    fn waker_list_can_add_and_remove_wakers() {
        let wake_count_one = Arc::new(AtomicUsize::new(0));
        let wake_count_two = Arc::new(AtomicUsize::new(0));

        let viewport_one = AsyncViewport::new_with_wake_up({
            let wake_count_one = wake_count_one.clone();
            Arc::new(move || {
                wake_count_one.fetch_add(1, Ordering::Relaxed);
            })
        });
        let viewport_two = AsyncViewport::new_with_wake_up({
            let wake_count_two = wake_count_two.clone();
            Arc::new(move || {
                wake_count_two.fetch_add(1, Ordering::Relaxed);
            })
        });

        let waker_list = AsyncWakerList::with_capacity(2);
        let index_one = waker_list.add_waker(viewport_one.new_waker());
        let _index_two = waker_list.add_waker(viewport_two.new_waker());

        waker_list.wake_up();
        assert_eq!(wake_count_one.load(Ordering::Relaxed), 1);
        assert_eq!(wake_count_two.load(Ordering::Relaxed), 1);

        viewport_one.woke_up();
        viewport_two.woke_up();

        // SAFETY: `index_one` was returned by `add_waker` above and this test removes it once.
        unsafe {
            waker_list.remove_waker(index_one);
        }
        waker_list.wake_up();
        assert_eq!(wake_count_one.load(Ordering::Relaxed), 1);
        assert_eq!(wake_count_two.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn wake_up_guard_triggers_on_drop() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let viewport = AsyncViewport::new_with_wake_up({
            let wake_count = wake_count.clone();
            Arc::new(move || {
                wake_count.fetch_add(1, Ordering::Relaxed);
            })
        });
        let waker = viewport.new_waker();

        {
            let _guard = waker.wake_up_guard();
            assert_eq!(wake_count.load(Ordering::Relaxed), 0);
        }

        assert_eq!(wake_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn viewport_implements_wake_up_trait() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let viewport = AsyncViewport::new_with_wake_up({
            let wake_count = wake_count.clone();
            Arc::new(move || {
                wake_count.fetch_add(1, Ordering::Relaxed);
            })
        });

        viewport.wake_up();
        assert_eq!(wake_count.load(Ordering::Relaxed), 1);

        viewport.wake_up();
        assert_eq!(wake_count.load(Ordering::Relaxed), 1);

        viewport.woke_up();
        viewport.wake_up();
        assert_eq!(wake_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn viewport_can_replace_wake_up_callback() {
        let wake_count_one = Arc::new(AtomicUsize::new(0));
        let wake_count_two = Arc::new(AtomicUsize::new(0));
        let viewport = AsyncViewport::new_with_wake_up({
            let wake_count_one = wake_count_one.clone();
            Arc::new(move || {
                wake_count_one.fetch_add(1, Ordering::Relaxed);
            })
        });
        let waker = viewport.new_waker();

        waker.wake_up();
        assert_eq!(wake_count_one.load(Ordering::Relaxed), 1);
        assert_eq!(wake_count_two.load(Ordering::Relaxed), 0);

        let _previous = viewport.replace_wake_up(Some(Arc::new({
            let wake_count_two = wake_count_two.clone();
            move || {
                wake_count_two.fetch_add(1, Ordering::Relaxed);
            }
        })));
        viewport.woke_up();

        waker.wake_up();
        assert_eq!(wake_count_one.load(Ordering::Relaxed), 1);
        assert_eq!(wake_count_two.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cleared_callback_keeps_waker_alive() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let viewport = AsyncViewport::new_with_wake_up({
            let wake_count = wake_count.clone();
            Arc::new(move || {
                wake_count.fetch_add(1, Ordering::Relaxed);
            })
        });
        let waker = viewport.new_waker();

        waker.wake_up();
        assert_eq!(wake_count.load(Ordering::Relaxed), 1);

        let _previous = viewport.replace_wake_up(None);
        viewport.woke_up();

        waker.wake_up();
        assert!(waker.is_alive());
        assert_eq!(wake_count.load(Ordering::Relaxed), 1);
    }
}

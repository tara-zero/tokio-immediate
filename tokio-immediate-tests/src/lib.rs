// SPDX-License-Identifier: Apache-2.0 OR MIT

// TODO: Most of the tests are LLM-generated. Needs review.

#[cfg(all(test, feature = "sync"))]
mod sync;
#[cfg(all(test, feature = "sync"))]
mod trigger;

#[cfg(test)]
mod tests {
    use ::core::future::pending;
    use ::std::sync::Arc;
    use ::std::sync::atomic::{AtomicUsize, Ordering};
    use ::std::thread::sleep as thread_sleep;
    use ::std::time::Duration;

    use ::tokio::runtime::Runtime;
    use ::tokio_immediate::single::{AsyncCall, AsyncCallState};
    use ::tokio_immediate::{AsyncViewport, AsyncWakeUp, AsyncWakerList};

    #[test]
    fn poll_returns_false_for_stopped_state() {
        let viewport = AsyncViewport::default();
        let mut call: AsyncCall<u32> = viewport.new_call();

        assert!(call.is_stopped());
        assert!(!call.poll());
    }

    #[test]
    fn start_and_poll_completes_task() {
        let runtime = Runtime::new().expect("runtime should initialize");
        let wake_count = Arc::new(AtomicUsize::new(0));
        let viewport = AsyncViewport::new_with_wake_up({
            let wake_count = wake_count.clone();
            Arc::new(move || {
                wake_count.fetch_add(1, Ordering::Relaxed);
            })
        });
        let mut call: AsyncCall<u32, _> = viewport.new_call_with_runtime(runtime.handle().clone());

        let previous = call.start(async { 7_u32 });
        assert!(previous.is_stopped());
        assert!(call.is_running());

        for _ in 0..50 {
            if call.poll() {
                break;
            }
            thread_sleep(Duration::from_millis(1));
        }

        assert!(call.is_completed());
        match &*call {
            AsyncCallState::Completed(value) => assert_eq!(*value, 7_u32),
            _ => panic!("state should be completed after task finish"),
        }
        assert_eq!(wake_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn aborted_task_transitions_back_to_stopped() {
        let runtime = Runtime::new().expect("runtime should initialize");
        let wake_count = Arc::new(AtomicUsize::new(0));
        let viewport = AsyncViewport::new_with_wake_up({
            let wake_count = wake_count.clone();
            Arc::new(move || {
                wake_count.fetch_add(1, Ordering::Relaxed);
            })
        });
        let mut call: AsyncCall<u32, _> = viewport.new_call_with_runtime(runtime.handle().clone());
        call.set_wake_up_on_manual_state_change(false);
        let _ = call.start(async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            1_u32
        });

        match &*call {
            AsyncCallState::Running(task) => task.abort(),
            _ => panic!("state should be running after start"),
        }

        for _ in 0..50 {
            if call.poll() {
                break;
            }
            thread_sleep(Duration::from_millis(1));
        }
        thread_sleep(Duration::from_millis(10));

        assert!(call.is_stopped());
        assert_eq!(wake_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn start_wakes_up_on_manual_state_change_by_default() {
        let runtime = Runtime::new().expect("runtime should initialize");
        let wake_count = Arc::new(AtomicUsize::new(0));
        let viewport = AsyncViewport::new_with_wake_up({
            let wake_count = wake_count.clone();
            Arc::new(move || {
                wake_count.fetch_add(1, Ordering::Relaxed);
            })
        });
        let mut call: AsyncCall<u32, _> = viewport.new_call_with_runtime(runtime.handle().clone());

        assert!(call.wake_up_on_manual_state_change());
        let _ = call.start(async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            1_u32
        });

        assert_eq!(wake_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn manual_state_change_wake_up_can_be_disabled() {
        let runtime = Runtime::new().expect("runtime should initialize");
        let wake_count = Arc::new(AtomicUsize::new(0));
        let viewport = AsyncViewport::new_with_wake_up({
            let wake_count = wake_count.clone();
            Arc::new(move || {
                wake_count.fetch_add(1, Ordering::Relaxed);
            })
        });
        let mut call: AsyncCall<u32, _> = viewport.new_call_with_runtime(runtime.handle().clone());

        call.set_wake_up_on_manual_state_change(false);
        assert!(!call.wake_up_on_manual_state_change());

        let _ = call.start(async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            1_u32
        });
        assert_eq!(wake_count.load(Ordering::Relaxed), 0);

        let previous = call.take_state();
        let task = match previous {
            AsyncCallState::Running(task) => task,
            _ => panic!("state should be running after start"),
        };
        assert_eq!(wake_count.load(Ordering::Relaxed), 0);
        task.abort();
    }

    #[test]
    fn take_state_wakes_up_when_manual_state_change_wake_up_enabled() {
        let runtime = Runtime::new().expect("runtime should initialize");
        let wake_count = Arc::new(AtomicUsize::new(0));
        let viewport = AsyncViewport::new_with_wake_up({
            let wake_count = wake_count.clone();
            Arc::new(move || {
                wake_count.fetch_add(1, Ordering::Relaxed);
            })
        });
        let mut call: AsyncCall<u32, _> = viewport.new_call_with_runtime(runtime.handle().clone());

        call.set_wake_up_on_manual_state_change(false);
        let _ = call.start(async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            1_u32
        });
        assert_eq!(wake_count.load(Ordering::Relaxed), 0);

        call.set_wake_up_on_manual_state_change(true);
        let previous = call.take_state();
        let task = match previous {
            AsyncCallState::Running(task) => task,
            _ => panic!("state should be running after start"),
        };
        assert!(call.is_stopped());
        assert_eq!(wake_count.load(Ordering::Relaxed), 1);
        task.abort();
    }

    #[test]
    fn second_start_returns_previous_running_state() {
        let runtime = Runtime::new().expect("runtime should initialize");
        let viewport = AsyncViewport::default();
        let mut call: AsyncCall<u32, _> = viewport.new_call_with_runtime(runtime.handle().clone());

        let _ = call.start(async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            1_u32
        });

        let previous = call.start(async { 9_u32 });
        match previous {
            AsyncCallState::Running(task) => task.abort(),
            _ => panic!("previous state should be running when starting second task"),
        }

        for _ in 0..50 {
            if call.poll() {
                break;
            }
            thread_sleep(Duration::from_millis(1));
        }

        match &*call {
            AsyncCallState::Completed(value) => assert_eq!(*value, 9_u32),
            _ => panic!("second task should complete"),
        }
    }

    #[test]
    fn future_is_dropped_before_finish_notifier_wake_up_on_normal_completion() {
        let runtime = Runtime::new().expect("runtime should initialize");
        let steps = Arc::new(AtomicUsize::new(0));
        let viewport = AsyncViewport::new_with_wake_up({
            let steps = steps.clone();
            Arc::new(move || {
                steps
                    .compare_exchange(1, 2, Ordering::Relaxed, Ordering::Relaxed)
                    .expect("wake_up happened out of order for normal completion");
            })
        });
        let mut call: AsyncCall<u32, _> = viewport.new_call_with_runtime(runtime.handle().clone());
        call.set_wake_up_on_manual_state_change(false);
        let task_steps = steps.clone();
        let drop_probe = ZeroToOneOnDrop { value: task_steps };

        let _ = call.start(async move {
            let _drop_probe = drop_probe;
            7_u32
        });

        for _ in 0..50 {
            if call.poll() {
                break;
            }
            thread_sleep(Duration::from_millis(1));
        }

        assert!(call.is_completed());
        assert_eq!(steps.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn future_is_dropped_before_finish_notifier_wake_up_when_task_is_aborted() {
        let runtime = Runtime::new().expect("runtime should initialize");
        let steps = Arc::new(AtomicUsize::new(0));
        let viewport = AsyncViewport::new_with_wake_up({
            let steps = steps.clone();
            Arc::new(move || {
                steps
                    .compare_exchange(1, 2, Ordering::Relaxed, Ordering::Relaxed)
                    .expect("wake_up happened out of order for abort");
            })
        });
        let mut call: AsyncCall<u32, _> = viewport.new_call_with_runtime(runtime.handle().clone());
        call.set_wake_up_on_manual_state_change(false);
        let task_steps = steps.clone();
        let drop_probe = ZeroToOneOnDrop { value: task_steps };
        let _ = call.start(async move {
            let _drop_probe = drop_probe;

            pending::<u32>().await
        });

        match &*call {
            AsyncCallState::Running(task) => task.abort(),
            _ => panic!("state should be running after start"),
        }

        for _ in 0..50 {
            if call.poll() {
                break;
            }
            thread_sleep(Duration::from_millis(1));
        }

        assert!(call.is_stopped());
        assert_eq!(steps.load(Ordering::Relaxed), 2);
    }

    struct ZeroToOneOnDrop {
        value: Arc<AtomicUsize>,
    }

    impl Drop for ZeroToOneOnDrop {
        fn drop(&mut self) {
            self.value
                .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
                .expect("drop happened out of order");
        }
    }

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

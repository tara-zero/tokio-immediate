// SPDX-License-Identifier: Apache-2.0 OR MIT

use ::std::panic::{AssertUnwindSafe, catch_unwind};
use ::std::sync::Arc;
use ::std::sync::atomic::{AtomicUsize, Ordering};
use ::std::thread::sleep as thread_sleep;
use ::std::time::Duration;

use ::tokio::runtime::Runtime;
use ::tokio_immediate::AsyncViewport;
use ::tokio_immediate::serial::AsyncSerialRunner;

#[test]
fn serial_runner_executes_tasks_in_submission_order() {
    let runtime = Runtime::new().expect("runtime should initialize");
    let viewport = AsyncViewport::default();
    let mut runner: AsyncSerialRunner<u32, _> =
        viewport.new_serial_runner_with_runtime(runtime.handle().clone());

    runner.run(async {
        tokio::time::sleep(Duration::from_millis(25)).await;
        1_u32
    });
    runner.run(async {
        tokio::time::sleep(Duration::from_millis(1)).await;
        2_u32
    });

    thread_sleep(Duration::from_millis(50));
    runner.poll();

    let values: Vec<u32> = runner.take_values_current().collect();
    assert_eq!(values, vec![1_u32, 2_u32]);
}

#[test]
fn serial_runner_accumulates_values_without_ui_polling() {
    let runtime = Runtime::new().expect("runtime should initialize");
    let viewport = AsyncViewport::default();
    let mut runner: AsyncSerialRunner<u32, _> =
        viewport.new_serial_runner_with_runtime(runtime.handle().clone());

    runner.run(async {
        tokio::time::sleep(Duration::from_millis(4)).await;
        10_u32
    });
    runner.run(async {
        tokio::time::sleep(Duration::from_millis(4)).await;
        11_u32
    });
    runner.run(async {
        tokio::time::sleep(Duration::from_millis(4)).await;
        12_u32
    });

    thread_sleep(Duration::from_millis(30));

    assert_eq!(runner.values_len(), 3);
    runner.poll();
    assert!(runner.is_idle());
}

#[test]
fn serial_runner_run_unit_does_not_buffer_values() {
    let runtime = Runtime::new().expect("runtime should initialize");
    let viewport = AsyncViewport::default();
    let mut runner: AsyncSerialRunner<u32, _> =
        viewport.new_serial_runner_with_runtime(runtime.handle().clone());

    let ran = Arc::new(AtomicUsize::new(0));
    runner.run_unit({
        let ran = ran.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            ran.fetch_add(1, Ordering::Relaxed);
        }
    });

    thread_sleep(Duration::from_millis(20));
    runner.poll();

    assert_eq!(ran.load(Ordering::Relaxed), 1);
    assert_eq!(runner.values_len(), 0);
    assert!(runner.take_value().is_none());
}

#[test]
fn serial_runner_take_values_variants_work() {
    let runtime = Runtime::new().expect("runtime should initialize");
    let viewport = AsyncViewport::default();
    let mut runner: AsyncSerialRunner<u32, _> =
        viewport.new_serial_runner_with_runtime(runtime.handle().clone());

    runner.run(async { 1_u32 });
    runner.run(async { 2_u32 });
    runner.run(async { 3_u32 });

    thread_sleep(Duration::from_millis(20));
    runner.poll();

    assert_eq!(runner.take_value(), Some(1_u32));

    let next_two: Vec<u32> = runner.take_values(5).collect();
    assert_eq!(next_two, vec![2_u32, 3_u32]);

    assert!(runner.take_values_current().next().is_none());
}

#[test]
fn serial_runner_propagates_task_panics_on_poll() {
    let runtime = Runtime::new().expect("runtime should initialize");
    let viewport = AsyncViewport::default();
    let mut runner: AsyncSerialRunner<u32, _> =
        viewport.new_serial_runner_with_runtime(runtime.handle().clone());

    runner.run(async {
        panic!("task panic should propagate via poll");
    });

    for _ in 0..100 {
        let poll_result = catch_unwind(AssertUnwindSafe(|| runner.poll()));
        if poll_result.is_err() {
            return;
        }
        thread_sleep(Duration::from_millis(1));
    }

    panic!("expected serial runner poll to re-raise task panic");
}

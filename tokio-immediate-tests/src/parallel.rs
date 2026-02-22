// SPDX-License-Identifier: Apache-2.0 OR MIT

use ::std::panic::{AssertUnwindSafe, catch_unwind};
use ::std::sync::Arc;
use ::std::sync::atomic::{AtomicUsize, Ordering};
use ::std::time::Duration;

use ::tokio::runtime::Runtime;
use ::tokio_immediate::AsyncViewport;
use ::tokio_immediate::parallel::AsyncParallelRunner;

use crate::common::{WAIT_TIMEOUT, wait_until};

#[test]
fn parallel_runner_executes_tasks_concurrently() {
    let runtime = Runtime::new().expect("runtime should initialize");
    let viewport = AsyncViewport::default();
    let mut runner: AsyncParallelRunner<u32, _> =
        viewport.new_parallel_runner_with_runtime(runtime.handle().clone());

    runner.run(async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        1_u32
    });
    runner.run(async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        2_u32
    });

    wait_until(
        || {
            runner.poll();
            runner.is_idle() && runner.values_len() == 2
        },
        WAIT_TIMEOUT,
        "timed out waiting for parallel tasks to complete",
    );

    assert!(runner.is_idle());
    assert_eq!(runner.values_len(), 2);
}

#[test]
fn parallel_runner_buffers_values_when_polled() {
    let runtime = Runtime::new().expect("runtime should initialize");
    let viewport = AsyncViewport::default();
    let mut runner: AsyncParallelRunner<u32, _> =
        viewport.new_parallel_runner_with_runtime(runtime.handle().clone());

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

    assert_eq!(runner.values_len(), 0);
    wait_until(
        || {
            runner.poll();
            runner.is_idle() && runner.values_len() == 3
        },
        WAIT_TIMEOUT,
        "timed out waiting for parallel values to be buffered",
    );
    assert!(runner.is_idle());
    assert_eq!(runner.values_len(), 3);
}

#[test]
fn parallel_runner_run_unit_does_not_buffer_values() {
    let runtime = Runtime::new().expect("runtime should initialize");
    let viewport = AsyncViewport::default();
    let mut runner: AsyncParallelRunner<u32, _> =
        viewport.new_parallel_runner_with_runtime(runtime.handle().clone());

    let ran = Arc::new(AtomicUsize::new(0));
    runner.run_unit({
        let ran = ran.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            ran.fetch_add(1, Ordering::Relaxed);
        }
    });

    wait_until(
        || ran.load(Ordering::Relaxed) == 1,
        WAIT_TIMEOUT,
        "timed out waiting for parallel run_unit task to execute",
    );
    wait_until(
        || {
            runner.poll();
            runner.is_idle()
        },
        WAIT_TIMEOUT,
        "timed out waiting for parallel run_unit task to settle",
    );

    assert_eq!(ran.load(Ordering::Relaxed), 1);
    assert_eq!(runner.values_len(), 0);
    assert!(runner.take_value().is_none());
}

#[test]
fn parallel_runner_take_values_variants_work() {
    let runtime = Runtime::new().expect("runtime should initialize");
    let viewport = AsyncViewport::default();
    let mut runner: AsyncParallelRunner<u32, _> =
        viewport.new_parallel_runner_with_runtime(runtime.handle().clone());

    runner.run(async { 1_u32 });
    runner.run(async { 2_u32 });
    runner.run(async { 3_u32 });

    wait_until(
        || {
            runner.poll();
            runner.is_idle() && runner.values_len() == 3
        },
        WAIT_TIMEOUT,
        "timed out waiting for parallel values to become available",
    );

    let first = runner.take_value().expect("one value should be available");
    assert!((1_u32..=3_u32).contains(&first));

    let mut all_values = vec![first];
    all_values.extend(runner.take_values(5));
    all_values.sort_unstable();
    assert_eq!(all_values, vec![1_u32, 2_u32, 3_u32]);

    assert!(runner.take_values_current().next().is_none());
}

#[test]
fn parallel_runner_propagates_task_panics_on_poll() {
    let runtime = Runtime::new().expect("runtime should initialize");
    let viewport = AsyncViewport::default();
    let mut runner: AsyncParallelRunner<u32, _> =
        viewport.new_parallel_runner_with_runtime(runtime.handle().clone());

    runner.run(async {
        panic!("task panic should propagate via poll");
    });

    wait_until(
        || catch_unwind(AssertUnwindSafe(|| runner.poll())).is_err(),
        WAIT_TIMEOUT,
        "expected parallel runner poll to re-raise task panic",
    );
}

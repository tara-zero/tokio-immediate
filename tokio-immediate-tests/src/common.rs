// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::thread::sleep as thread_sleep;
use std::time::{Duration, Instant};

pub const WAIT_TIMEOUT: Duration = Duration::from_secs(2);
const WAIT_STEP: Duration = Duration::from_millis(1);

pub fn wait_until(mut condition: impl FnMut() -> bool, timeout: Duration, timeout_message: &str) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        thread_sleep(WAIT_STEP);
    }
    panic!("{timeout_message}");
}

// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::{AsyncWaker, AsyncWakerList};

pub(super) struct WakerRegistration {
    wakers: AsyncWakerList,
    waker_idx: usize,
}

impl Drop for WakerRegistration {
    fn drop(&mut self) {
        if self.waker_idx != usize::MAX {
            unsafe {
                // SAFETY: This is safe because `self.waker_idx` is a valid index returned
                // by `AsyncWakerList::add_waker()` and we are removing only once.
                self.wakers.remove_waker(self.waker_idx);
            }
        }
    }
}

impl Clone for WakerRegistration {
    fn clone(&self) -> Self {
        Self::new(self.wakers.clone())
    }
}

impl WakerRegistration {
    pub(super) fn clone_with_waker(&self, waker: AsyncWaker) -> Self {
        Self::new_with_waker(self.wakers.clone(), waker)
    }

    pub(super) fn new_with_waker(wakers: AsyncWakerList, waker: AsyncWaker) -> Self {
        let waker_idx = wakers.add_waker(waker);
        Self { wakers, waker_idx }
    }

    pub(super) fn new(wakers: AsyncWakerList) -> Self {
        Self {
            wakers,
            waker_idx: usize::MAX,
        }
    }
}

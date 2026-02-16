// SPDX-License-Identifier: Apache-2.0 OR MIT

use ::std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{AsyncGlueWakeUp, AsyncGlueWaker};

#[derive(Clone, Default)]
pub(super) struct WakerBinding {
    waker: Arc<RwLock<Option<AsyncGlueWaker>>>,
}

impl WakerBinding {
    pub(super) fn set_waker(&self, waker: AsyncGlueWaker) {
        *self.waker_mut() = Some(waker);
    }

    pub(super) fn clear_waker(&self) {
        *self.waker_mut() = None;
    }

    pub(super) fn wake_up(&self) {
        if let Some(waker) = self.waker().as_ref() {
            waker.wake_up();
        }
    }

    fn waker(&'_ self) -> RwLockReadGuard<'_, Option<AsyncGlueWaker>> {
        self.waker
            .read()
            .expect("Failed to read-lock sync waker binding: poisoned by panic in another thread")
    }

    fn waker_mut(&'_ self) -> RwLockWriteGuard<'_, Option<AsyncGlueWaker>> {
        self.waker
            .write()
            .expect("Failed to write-lock sync waker binding: poisoned by panic in another thread")
    }
}

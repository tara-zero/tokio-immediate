// SPDX-License-Identifier: Apache-2.0 OR MIT

//! [`egui`] integration for [`tokio_immediate`].
//!
//! [`EguiAsync`] manages [`AsyncViewport`]s automatically using an
//! [`egui::Plugin`], so you only need to create it once, register the plugin,
//! and then call its factory methods to obtain [`AsyncCall`] instances,
//! [`AsyncWaker`]s, or [`trigger::AsyncTrigger`]s.
//!
//! Most factory methods come in three viewport-targeting variants:
//!
//! | Suffix | Target viewport |
//! |---|---|
//! | `*_for_root()` | [`ViewportId::ROOT`] |
//! | `*()` (no suffix) | The current viewport (`ctx.viewport_id()`) |
//! | `*_for(viewport_id)` | An explicit [`ViewportId`] |
//!
//! ## Feature flags
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(
    feature = "document-features",
    cfg_attr(doc, doc = ::document_features::document_features!())
)]
//
// Clippy lints.
#![warn(clippy::pedantic)]
#![warn(clippy::cargo)]
#![warn(clippy::undocumented_unsafe_blocks)]

use ::std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};

use ::egui::{Context, FullOutput, OrderedViewportIdMap, Plugin, RawInput, ViewportId};

pub use ::tokio_immediate::tokio;

#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub use ::tokio_immediate::sync;
#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub use ::tokio_immediate::trigger;

pub use ::tokio_immediate::parallel;
#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub use ::tokio_immediate::serial;
pub use ::tokio_immediate::single;
pub use ::tokio_immediate::{
    AsyncCurrentRuntime, AsyncRuntime, AsyncViewport, AsyncWakeUp, AsyncWakeUpCallback,
    AsyncWakeUpGuard, AsyncWaker, AsyncWakerList,
};

use ::tokio_immediate::parallel::AsyncParallelRunner;
#[cfg(feature = "sync")]
use ::tokio_immediate::serial::AsyncSerialRunner;
use ::tokio_immediate::single::AsyncCall;

/// Manages [`AsyncViewport`]s for all egui viewports via a [`Plugin`].
///
/// Create one at application startup, register its [`plugin()`](Self::plugin)
/// on the [`egui::Context`], and then use the factory methods to create
/// [`AsyncCall`] instances, [`AsyncWaker`]s, or
/// [`trigger::AsyncTrigger`]s bound to the appropriate viewport.
///
/// # Viewport lifetime
///
/// The plugin automatically drops [`AsyncViewport`]s for viewports that
/// are no longer present in the egui output (i.e. closed windows). Any
/// [`AsyncCall`] or [`AsyncWaker`] that was bound to a dropped viewport
/// will silently stop requesting UI repaints — tasks still run to
/// completion, but the UI will not be notified. Because of this, creating
/// [`AsyncCall`] instances for non-root viewports ahead of time is
/// pointless; create them only once the viewport is actually open.
///
/// # Panics
///
/// Factory methods that create viewport-bound objects (`new_call*`,
/// `new_serial_runner*`, `new_parallel_runner*`, `new_waker*`, and
/// `new_trigger*`) require the plugin returned by [`plugin()`](Self::plugin)
/// to be registered and initialized via [`Context::add_plugin()`]. Calling
/// them before plugin setup panics.
///
/// `new_serial_runner*` methods may additionally panic if runtime access
/// preconditions are not met by the selected runtime (for example,
/// [`AsyncCurrentRuntime`] requires a Tokio runtime context on the current
/// thread).
#[derive(Default, Clone)]
pub struct EguiAsync {
    inner: Arc<RwLock<EguiAsyncPluginInner>>,
}

// TODO: Make weak context ptr available on `egui` side and remove this.
/// The [`Plugin`] half of [`EguiAsync`].
///
/// This is a separate type that holds a [`Weak`] reference back to the
/// shared state in order to break a reference cycle:
/// `EguiAsync` → `AsyncViewport` → wake-up callback → `egui::Context`
/// → plugins → back to `EguiAsync`. The [`EguiAsync`] value must be kept
/// alive for the plugin to function.
///
/// Also exposes the same factory methods as [`EguiAsync`] for convenience.
///
/// # Panics
///
/// All forwarding factory methods panic if this plugin outlives its owning
/// [`EguiAsync`] value.
///
/// They also inherit the panic conditions documented on the corresponding
/// [`EguiAsync`] methods.
#[derive(Clone)]
pub struct EguiAsyncPlugin {
    inner: Weak<RwLock<EguiAsyncPluginInner>>,
}

#[derive(Default)]
struct EguiAsyncPluginInner {
    context: Option<Context>,
    viewports: OrderedViewportIdMap<AsyncViewport>,

    #[cfg(feature = "sync")]
    triggers: OrderedViewportIdMap<trigger::AsyncTriggerHandle>,
}

impl EguiAsync {
    /// Returns the [`Plugin`] that must be registered on the
    /// [`egui::Context`] (via [`Context::add_plugin()`]).
    #[must_use]
    pub fn plugin(&self) -> EguiAsyncPlugin {
        EguiAsyncPlugin {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// Creates an [`AsyncCall`] bound to the root viewport, using
    /// `A::default()` as the runtime.
    #[must_use]
    pub fn new_call_for_root<T, A>(&self) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        AsyncCall::new(self.new_waker_for_root())
    }

    /// Creates an [`AsyncCall`] bound to the root viewport with an explicit
    /// runtime.
    #[must_use]
    pub fn new_call_with_runtime_for_root<T, A>(&self, runtime: A) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        AsyncCall::new_with_runtime(self.new_waker_for_root(), runtime)
    }

    /// Creates an [`AsyncCall`] bound to the current viewport, using
    /// `A::default()` as the runtime.
    #[must_use]
    pub fn new_call<T, A>(&self) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        AsyncCall::new(self.new_waker())
    }

    /// Creates an [`AsyncCall`] bound to the current viewport with an
    /// explicit runtime.
    #[must_use]
    pub fn new_call_with_runtime<T, A>(&self, runtime: A) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        AsyncCall::new_with_runtime(self.new_waker(), runtime)
    }

    /// Creates an [`AsyncCall`] bound to `viewport_id`, using
    /// `A::default()` as the runtime.
    #[must_use]
    pub fn new_call_for<T, A>(&self, viewport_id: ViewportId) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        AsyncCall::new(self.new_waker_for(viewport_id))
    }

    /// Creates an [`AsyncCall`] bound to `viewport_id` with an explicit
    /// runtime.
    #[must_use]
    pub fn new_call_with_runtime_for<T, A>(
        &self,
        runtime: A,
        viewport_id: ViewportId,
    ) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        AsyncCall::new_with_runtime(self.new_waker_for(viewport_id), runtime)
    }

    /// Creates an [`AsyncSerialRunner`] bound to the root viewport, using
    /// `A::default()` as the runtime.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner_for_root<T, A>(&self) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        AsyncSerialRunner::new(self.new_waker_for_root())
    }

    /// Creates an [`AsyncSerialRunner`] bound to the root viewport with an
    /// explicit runtime.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner_with_runtime_for_root<T, A>(
        &self,
        runtime: A,
    ) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        AsyncSerialRunner::new_with_runtime(self.new_waker_for_root(), runtime)
    }

    /// Creates an [`AsyncSerialRunner`] bound to the current viewport, using
    /// `A::default()` as the runtime.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner<T, A>(&self) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        AsyncSerialRunner::new(self.new_waker())
    }

    /// Creates an [`AsyncSerialRunner`] bound to the current viewport with an
    /// explicit runtime.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner_with_runtime<T, A>(&self, runtime: A) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        AsyncSerialRunner::new_with_runtime(self.new_waker(), runtime)
    }

    /// Creates an [`AsyncSerialRunner`] bound to `viewport_id`, using
    /// `A::default()` as the runtime.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner_for<T, A>(&self, viewport_id: ViewportId) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        AsyncSerialRunner::new(self.new_waker_for(viewport_id))
    }

    /// Creates an [`AsyncSerialRunner`] bound to `viewport_id` with an
    /// explicit runtime.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner_with_runtime_for<T, A>(
        &self,
        runtime: A,
        viewport_id: ViewportId,
    ) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        AsyncSerialRunner::new_with_runtime(self.new_waker_for(viewport_id), runtime)
    }

    /// Creates an [`AsyncParallelRunner`] bound to the root viewport, using
    /// `A::default()` as the runtime.
    #[must_use]
    pub fn new_parallel_runner_for_root<T, A>(&self) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        AsyncParallelRunner::new(self.new_waker_for_root())
    }

    /// Creates an [`AsyncParallelRunner`] bound to the root viewport with an
    /// explicit runtime.
    #[must_use]
    pub fn new_parallel_runner_with_runtime_for_root<T, A>(
        &self,
        runtime: A,
    ) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        AsyncParallelRunner::new_with_runtime(self.new_waker_for_root(), runtime)
    }

    /// Creates an [`AsyncParallelRunner`] bound to the current viewport, using
    /// `A::default()` as the runtime.
    #[must_use]
    pub fn new_parallel_runner<T, A>(&self) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        AsyncParallelRunner::new(self.new_waker())
    }

    /// Creates an [`AsyncParallelRunner`] bound to the current viewport with
    /// an explicit runtime.
    #[must_use]
    pub fn new_parallel_runner_with_runtime<T, A>(&self, runtime: A) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        AsyncParallelRunner::new_with_runtime(self.new_waker(), runtime)
    }

    /// Creates an [`AsyncParallelRunner`] bound to `viewport_id`, using
    /// `A::default()` as the runtime.
    #[must_use]
    pub fn new_parallel_runner_for<T, A>(
        &self,
        viewport_id: ViewportId,
    ) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        AsyncParallelRunner::new(self.new_waker_for(viewport_id))
    }

    /// Creates an [`AsyncParallelRunner`] bound to `viewport_id` with an
    /// explicit runtime.
    #[must_use]
    pub fn new_parallel_runner_with_runtime_for<T, A>(
        &self,
        runtime: A,
        viewport_id: ViewportId,
    ) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        AsyncParallelRunner::new_with_runtime(self.new_waker_for(viewport_id), runtime)
    }

    /// Creates an [`AsyncWaker`] for the root viewport.
    #[must_use]
    pub fn new_waker_for_root(&self) -> AsyncWaker {
        self.new_waker_for(ViewportId::ROOT)
    }

    /// Creates an [`AsyncWaker`] for the current viewport.
    #[must_use]
    pub fn new_waker(&self) -> AsyncWaker {
        let inner = self.inner_mut();
        let viewport_id = Self::context(&inner).viewport_id();
        Self::new_waker_for_inner(inner, viewport_id)
    }

    /// Creates an [`AsyncWaker`] for `viewport_id`.
    #[must_use]
    pub fn new_waker_for(&self, viewport_id: ViewportId) -> AsyncWaker {
        Self::new_waker_for_inner(self.inner_mut(), viewport_id)
    }

    /// Shared implementation behind the `new_waker*` family.
    fn new_waker_for_inner(
        mut inner: RwLockWriteGuard<'_, EguiAsyncPluginInner>,
        viewport_id: ViewportId,
    ) -> AsyncWaker {
        if let Some(viewport) = inner.viewports.get(&viewport_id) {
            viewport.new_waker()
        } else {
            let ctx = Self::context(&inner).clone();
            let viewport = AsyncViewport::new_with_wake_up(Arc::new(move || {
                ctx.request_repaint_of(viewport_id);
            }));
            let waker = viewport.new_waker();
            inner.viewports.insert(viewport_id, viewport);
            waker
        }
    }

    /// Creates an [`AsyncTrigger`](trigger::AsyncTrigger) for the
    /// root viewport.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_trigger_for_root(&self) -> trigger::AsyncTrigger {
        self.new_trigger_for(ViewportId::ROOT)
    }

    /// Creates an [`AsyncTrigger`](trigger::AsyncTrigger) for the
    /// current viewport.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_trigger(&self) -> trigger::AsyncTrigger {
        let inner = self.inner_mut();
        let viewport_id = Self::context(&inner).viewport_id();
        Self::new_trigger_for_inner(inner, viewport_id)
    }

    /// Creates an [`AsyncTrigger`](trigger::AsyncTrigger) for
    /// `viewport_id`.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_trigger_for(&self, viewport_id: ViewportId) -> trigger::AsyncTrigger {
        Self::new_trigger_for_inner(self.inner_mut(), viewport_id)
    }

    #[cfg(feature = "sync")]
    fn new_trigger_for_inner(
        mut inner: RwLockWriteGuard<'_, EguiAsyncPluginInner>,
        viewport_id: ViewportId,
    ) -> trigger::AsyncTrigger {
        if let Some(trigger) = inner.triggers.get(&viewport_id) {
            trigger.subscribe()
        } else {
            let handle = trigger::AsyncTriggerHandle::default();
            let trigger = handle.subscribe();
            inner.triggers.insert(viewport_id, handle);
            trigger
        }
    }

    fn context<'a>(inner: &'a RwLockWriteGuard<'_, EguiAsyncPluginInner>) -> &'a Context {
        inner
            .context
            .as_ref()
            .expect("No egui context: EguiAsyncPlugin is not initialized yet")
    }

    fn inner_mut(&self) -> RwLockWriteGuard<'_, EguiAsyncPluginInner> {
        self.inner
            .write()
            .expect("Failed to access EguiAsync: poisoned by panic in another thread")
    }
}

impl Plugin for EguiAsyncPlugin {
    fn debug_name(&self) -> &'static str {
        "EguiAsyncPlugin"
    }

    fn setup(&mut self, ctx: &Context) {
        let egui_async = self.upgrade();
        let mut inner = Self::inner_mut(&egui_async.inner);

        inner.context = Some(ctx.clone());
    }

    #[cfg(feature = "sync")]
    fn on_end_pass(&mut self, ctx: &Context) {
        let egui_async = self.upgrade();
        let inner = Self::inner(&egui_async.inner);

        if !ctx.will_discard()
            && let Some(trigger) = inner.triggers.get(&ctx.viewport_id())
        {
            trigger.trigger();
        }
    }

    fn input_hook(&mut self, input: &mut RawInput) {
        let egui_async = self.upgrade();
        let inner = Self::inner(&egui_async.inner);

        if let Some(viewport) = inner.viewports.get(&input.viewport_id) {
            viewport.woke_up();
        }
    }

    fn output_hook(&mut self, output: &mut FullOutput) {
        let egui_async = self.upgrade();
        let mut inner = Self::inner_mut(&egui_async.inner);

        // Remove closed viewports.
        inner
            .viewports
            .retain(|vid, _| output.viewport_output.contains_key(vid));

        #[cfg(feature = "sync")]
        inner
            .triggers
            .retain(|vid, _| output.viewport_output.contains_key(vid));
    }
}

impl EguiAsyncPlugin {
    /// See [`EguiAsync::new_call_for_root()`].
    #[must_use]
    pub fn new_call_for_root<T, A>(&self) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        self.upgrade().new_call_for_root()
    }

    /// See [`EguiAsync::new_call_with_runtime_for_root()`].
    #[must_use]
    pub fn new_call_with_runtime_for_root<T, A>(&self, runtime: A) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        self.upgrade().new_call_with_runtime_for_root(runtime)
    }

    /// See [`EguiAsync::new_call()`].
    #[must_use]
    pub fn new_call<T, A>(&self) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        self.upgrade().new_call()
    }

    /// See [`EguiAsync::new_call_with_runtime()`].
    #[must_use]
    pub fn new_call_with_runtime<T, A>(&self, runtime: A) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        self.upgrade().new_call_with_runtime(runtime)
    }

    /// See [`EguiAsync::new_call_for()`].
    #[must_use]
    pub fn new_call_for<T, A>(&self, viewport_id: ViewportId) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        self.upgrade().new_call_for(viewport_id)
    }

    /// See [`EguiAsync::new_call_with_runtime_for()`].
    #[must_use]
    pub fn new_call_with_runtime_for<T, A>(
        &self,
        runtime: A,
        viewport_id: ViewportId,
    ) -> AsyncCall<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        self.upgrade()
            .new_call_with_runtime_for(runtime, viewport_id)
    }

    /// See [`EguiAsync::new_serial_runner_for_root()`].
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner_for_root<T, A>(&self) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        self.upgrade().new_serial_runner_for_root()
    }

    /// See [`EguiAsync::new_serial_runner_with_runtime_for_root()`].
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner_with_runtime_for_root<T, A>(
        &self,
        runtime: A,
    ) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        self.upgrade()
            .new_serial_runner_with_runtime_for_root(runtime)
    }

    /// See [`EguiAsync::new_serial_runner()`].
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner<T, A>(&self) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        self.upgrade().new_serial_runner()
    }

    /// See [`EguiAsync::new_serial_runner_with_runtime()`].
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner_with_runtime<T, A>(&self, runtime: A) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        self.upgrade().new_serial_runner_with_runtime(runtime)
    }

    /// See [`EguiAsync::new_serial_runner_for()`].
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner_for<T, A>(&self, viewport_id: ViewportId) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        self.upgrade().new_serial_runner_for(viewport_id)
    }

    /// See [`EguiAsync::new_serial_runner_with_runtime_for()`].
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_serial_runner_with_runtime_for<T, A>(
        &self,
        runtime: A,
        viewport_id: ViewportId,
    ) -> AsyncSerialRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        self.upgrade()
            .new_serial_runner_with_runtime_for(runtime, viewport_id)
    }

    /// See [`EguiAsync::new_parallel_runner_for_root()`].
    #[must_use]
    pub fn new_parallel_runner_for_root<T, A>(&self) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        self.upgrade().new_parallel_runner_for_root()
    }

    /// See [`EguiAsync::new_parallel_runner_with_runtime_for_root()`].
    #[must_use]
    pub fn new_parallel_runner_with_runtime_for_root<T, A>(
        &self,
        runtime: A,
    ) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        self.upgrade()
            .new_parallel_runner_with_runtime_for_root(runtime)
    }

    /// See [`EguiAsync::new_parallel_runner()`].
    #[must_use]
    pub fn new_parallel_runner<T, A>(&self) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        self.upgrade().new_parallel_runner()
    }

    /// See [`EguiAsync::new_parallel_runner_with_runtime()`].
    #[must_use]
    pub fn new_parallel_runner_with_runtime<T, A>(&self, runtime: A) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        self.upgrade().new_parallel_runner_with_runtime(runtime)
    }

    /// See [`EguiAsync::new_parallel_runner_for()`].
    #[must_use]
    pub fn new_parallel_runner_for<T, A>(
        &self,
        viewport_id: ViewportId,
    ) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncRuntime,
    {
        self.upgrade().new_parallel_runner_for(viewport_id)
    }

    /// See [`EguiAsync::new_parallel_runner_with_runtime_for()`].
    #[must_use]
    pub fn new_parallel_runner_with_runtime_for<T, A>(
        &self,
        runtime: A,
        viewport_id: ViewportId,
    ) -> AsyncParallelRunner<T, A>
    where
        T: 'static + Send,
        A: AsyncRuntime,
    {
        self.upgrade()
            .new_parallel_runner_with_runtime_for(runtime, viewport_id)
    }

    /// See [`EguiAsync::new_waker_for_root()`].
    #[must_use]
    pub fn new_waker_for_root(&self) -> AsyncWaker {
        self.upgrade().new_waker_for_root()
    }

    /// See [`EguiAsync::new_waker()`].
    #[must_use]
    pub fn new_waker(&self) -> AsyncWaker {
        self.upgrade().new_waker()
    }

    /// See [`EguiAsync::new_waker_for()`].
    #[must_use]
    pub fn new_waker_for(&self, viewport_id: ViewportId) -> AsyncWaker {
        self.upgrade().new_waker_for(viewport_id)
    }

    /// See [`EguiAsync::new_trigger_for_root()`].
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_trigger_for_root(&self) -> trigger::AsyncTrigger {
        self.upgrade().new_trigger_for_root()
    }

    /// See [`EguiAsync::new_trigger()`].
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_trigger(&self) -> trigger::AsyncTrigger {
        self.upgrade().new_trigger()
    }

    /// See [`EguiAsync::new_trigger_for()`].
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_trigger_for(&self, viewport_id: ViewportId) -> trigger::AsyncTrigger {
        self.upgrade().new_trigger_for(viewport_id)
    }

    fn inner(
        inner: &Arc<RwLock<EguiAsyncPluginInner>>,
    ) -> RwLockReadGuard<'_, EguiAsyncPluginInner> {
        inner
            .read()
            .expect("Failed to read-lock EguiAsyncPlugin: poisoned by panic in another thread")
    }

    fn inner_mut(
        inner: &Arc<RwLock<EguiAsyncPluginInner>>,
    ) -> RwLockWriteGuard<'_, EguiAsyncPluginInner> {
        inner
            .write()
            .expect("Failed to write-lock EguiAsyncPlugin: poisoned by panic in another thread")
    }

    fn upgrade(&self) -> EguiAsync {
        EguiAsync {
            inner: self
                .inner
                .upgrade()
                .expect("EguiAsyncPlugin outlived its EguiAsync owner"),
        }
    }
}

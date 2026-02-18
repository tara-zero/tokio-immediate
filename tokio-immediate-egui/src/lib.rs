// SPDX-License-Identifier: Apache-2.0 OR MIT

//! [`egui`] integration for [`tokio_immediate`].
//!
//! [`EguiAsync`] manages [`AsyncGlueViewport`]s automatically using an
//! [`egui::Plugin`], so you only need to create it once, register the plugin,
//! and then call its factory methods to obtain [`AsyncGlue`] instances,
//! [`AsyncGlueWaker`]s, or [`trigger::AsyncGlueTrigger`]s.
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
#![cfg_attr(
    feature = "document-features",
    cfg_attr(doc, doc = ::document_features::document_features!())
)]
//
// Clippy lints.
#![warn(clippy::pedantic)]
#![warn(clippy::cargo)]

use ::std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};

use ::egui::{Context, FullOutput, OrderedViewportIdMap, Plugin, RawInput, ViewportId};

pub use ::tokio_immediate::tokio;

#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub use ::tokio_immediate::sync;
#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub use ::tokio_immediate::trigger;

pub use ::tokio_immediate::{
    AsyncGlue, AsyncGlueCurrentRuntime, AsyncGlueRuntime, AsyncGlueState, AsyncGlueViewport,
    AsyncGlueWakeUp, AsyncGlueWakeUpCallback, AsyncGlueWakeUpGuard, AsyncGlueWaker,
    AsyncGlueWakerList,
};

/// Manages [`AsyncGlueViewport`]s for all egui viewports via a [`Plugin`].
///
/// Create one at application startup, register its [`plugin()`](Self::plugin)
/// on the [`egui::Context`], and then use the factory methods to create
/// [`AsyncGlue`] instances, [`AsyncGlueWaker`]s, or
/// [`trigger::AsyncGlueTrigger`]s bound to the appropriate viewport.
///
/// # Viewport lifetime
///
/// The plugin automatically drops [`AsyncGlueViewport`]s for viewports that
/// are no longer present in the egui output (i.e. closed windows). Any
/// [`AsyncGlue`] or [`AsyncGlueWaker`] that was bound to a dropped viewport
/// will silently stop requesting UI repaints — tasks still run to
/// completion, but the UI will not be notified. Because of this, creating
/// [`AsyncGlue`] instances for non-root viewports ahead of time is
/// pointless; create them only once the viewport is actually open.
#[derive(Default, Clone)]
pub struct EguiAsync {
    inner: Arc<RwLock<EguiAsyncPluginInner>>,
}

// TODO: Make weak context ptr available on `egui` side and remove this.
/// The [`Plugin`] half of [`EguiAsync`].
///
/// This is a separate type that holds a [`Weak`] reference back to the
/// shared state in order to break a reference cycle:
/// `EguiAsync` → `AsyncGlueViewport` → wake-up callback → `egui::Context`
/// → plugins → back to `EguiAsync`. The [`EguiAsync`] value must be kept
/// alive for the plugin to function.
///
/// Also exposes the same factory methods as [`EguiAsync`] for convenience.
#[derive(Clone)]
pub struct EguiAsyncPlugin {
    inner: Weak<RwLock<EguiAsyncPluginInner>>,
}

#[derive(Default)]
struct EguiAsyncPluginInner {
    context: Option<Context>,
    viewports: OrderedViewportIdMap<AsyncGlueViewport>,

    #[cfg(feature = "sync")]
    triggers: OrderedViewportIdMap<trigger::AsyncGlueTriggerHandle>,
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

    /// Creates an [`AsyncGlue`] bound to the root viewport, using
    /// `A::default()` as the runtime.
    #[must_use]
    pub fn new_glue_for_root<T, A>(&self) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncGlueRuntime,
    {
        AsyncGlue::new(self.new_waker_for_root())
    }

    /// Creates an [`AsyncGlue`] bound to the root viewport with an explicit
    /// runtime.
    #[must_use]
    pub fn new_glue_with_runtime_for_root<T, A>(&self, runtime: A) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: AsyncGlueRuntime,
    {
        AsyncGlue::new_with_runtime(self.new_waker_for_root(), runtime)
    }

    /// Creates an [`AsyncGlue`] bound to the current viewport, using
    /// `A::default()` as the runtime.
    #[must_use]
    pub fn new_glue<T, A>(&self) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncGlueRuntime,
    {
        AsyncGlue::new(self.new_waker())
    }

    /// Creates an [`AsyncGlue`] bound to the current viewport with an
    /// explicit runtime.
    #[must_use]
    pub fn new_glue_with_runtime<T, A>(&self, runtime: A) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: AsyncGlueRuntime,
    {
        AsyncGlue::new_with_runtime(self.new_waker(), runtime)
    }

    /// Creates an [`AsyncGlue`] bound to `viewport_id`, using
    /// `A::default()` as the runtime.
    #[must_use]
    pub fn new_glue_for<T, A>(&self, viewport_id: ViewportId) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncGlueRuntime,
    {
        AsyncGlue::new(self.new_waker_for(viewport_id))
    }

    /// Creates an [`AsyncGlue`] bound to `viewport_id` with an explicit
    /// runtime.
    #[must_use]
    pub fn new_glue_with_runtime_for<T, A>(
        &self,
        runtime: A,
        viewport_id: ViewportId,
    ) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: AsyncGlueRuntime,
    {
        AsyncGlue::new_with_runtime(self.new_waker_for(viewport_id), runtime)
    }

    /// Creates an [`AsyncGlueWaker`] for the root viewport.
    #[must_use]
    pub fn new_waker_for_root(&self) -> AsyncGlueWaker {
        self.new_waker_for(ViewportId::ROOT)
    }

    /// Creates an [`AsyncGlueWaker`] for the current viewport.
    #[must_use]
    pub fn new_waker(&self) -> AsyncGlueWaker {
        let inner = self.inner_mut();
        let viewport_id = Self::context(&inner).viewport_id();
        Self::new_waker_for_inner(inner, viewport_id)
    }

    /// Creates an [`AsyncGlueWaker`] for `viewport_id`.
    #[must_use]
    pub fn new_waker_for(&self, viewport_id: ViewportId) -> AsyncGlueWaker {
        Self::new_waker_for_inner(self.inner_mut(), viewport_id)
    }

    /// Shared implementation behind the `new_waker*` family.
    fn new_waker_for_inner(
        mut inner: RwLockWriteGuard<'_, EguiAsyncPluginInner>,
        viewport_id: ViewportId,
    ) -> AsyncGlueWaker {
        if let Some(viewport) = inner.viewports.get(&viewport_id) {
            viewport.new_waker()
        } else {
            let ctx = Self::context(&inner).clone();
            let viewport = AsyncGlueViewport::new_with_wake_up(Arc::new(move || {
                ctx.request_repaint_of(viewport_id);
            }));
            let waker = viewport.new_waker();
            inner.viewports.insert(viewport_id, viewport);
            waker
        }
    }

    /// Creates an [`AsyncGlueTrigger`](trigger::AsyncGlueTrigger) for the
    /// root viewport.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_trigger_for_root(&self) -> trigger::AsyncGlueTrigger {
        self.new_trigger_for(ViewportId::ROOT)
    }

    /// Creates an [`AsyncGlueTrigger`](trigger::AsyncGlueTrigger) for the
    /// current viewport.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_trigger(&self) -> trigger::AsyncGlueTrigger {
        let inner = self.inner_mut();
        let viewport_id = Self::context(&inner).viewport_id();
        Self::new_trigger_for_inner(inner, viewport_id)
    }

    /// Creates an [`AsyncGlueTrigger`](trigger::AsyncGlueTrigger) for
    /// `viewport_id`.
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_trigger_for(&self, viewport_id: ViewportId) -> trigger::AsyncGlueTrigger {
        Self::new_trigger_for_inner(self.inner_mut(), viewport_id)
    }

    #[cfg(feature = "sync")]
    fn new_trigger_for_inner(
        mut inner: RwLockWriteGuard<'_, EguiAsyncPluginInner>,
        viewport_id: ViewportId,
    ) -> trigger::AsyncGlueTrigger {
        if let Some(trigger) = inner.triggers.get(&viewport_id) {
            trigger.subscribe()
        } else {
            let handle = trigger::AsyncGlueTriggerHandle::default();
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
    /// See [`EguiAsync::new_glue_for_root()`].
    #[must_use]
    pub fn new_glue_for_root<T, A>(&self) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncGlueRuntime,
    {
        self.upgrade().new_glue_for_root()
    }

    /// See [`EguiAsync::new_glue_with_runtime_for_root()`].
    #[must_use]
    pub fn new_glue_with_runtime_for_root<T, A>(&self, runtime: A) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: AsyncGlueRuntime,
    {
        self.upgrade().new_glue_with_runtime_for_root(runtime)
    }

    /// See [`EguiAsync::new_glue()`].
    #[must_use]
    pub fn new_glue<T, A>(&self) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncGlueRuntime,
    {
        self.upgrade().new_glue()
    }

    /// See [`EguiAsync::new_glue_with_runtime()`].
    #[must_use]
    pub fn new_glue_with_runtime<T, A>(&self, runtime: A) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: AsyncGlueRuntime,
    {
        self.upgrade().new_glue_with_runtime(runtime)
    }

    /// See [`EguiAsync::new_glue_for()`].
    #[must_use]
    pub fn new_glue_for<T, A>(&self, viewport_id: ViewportId) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: Default + AsyncGlueRuntime,
    {
        self.upgrade().new_glue_for(viewport_id)
    }

    /// See [`EguiAsync::new_glue_with_runtime_for()`].
    #[must_use]
    pub fn new_glue_with_runtime_for<T, A>(
        &self,
        runtime: A,
        viewport_id: ViewportId,
    ) -> AsyncGlue<T, A>
    where
        T: 'static + Send,
        A: AsyncGlueRuntime,
    {
        self.upgrade()
            .new_glue_with_runtime_for(runtime, viewport_id)
    }

    /// See [`EguiAsync::new_waker_for_root()`].
    #[must_use]
    pub fn new_waker_for_root(&self) -> AsyncGlueWaker {
        self.upgrade().new_waker_for_root()
    }

    /// See [`EguiAsync::new_waker()`].
    #[must_use]
    pub fn new_waker(&self) -> AsyncGlueWaker {
        self.upgrade().new_waker()
    }

    /// See [`EguiAsync::new_waker_for()`].
    #[must_use]
    pub fn new_waker_for(&self, viewport_id: ViewportId) -> AsyncGlueWaker {
        self.upgrade().new_waker_for(viewport_id)
    }

    /// See [`EguiAsync::new_trigger_for_root()`].
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_trigger_for_root(&self) -> trigger::AsyncGlueTrigger {
        self.upgrade().new_trigger_for_root()
    }

    /// See [`EguiAsync::new_trigger()`].
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_trigger(&self) -> trigger::AsyncGlueTrigger {
        self.upgrade().new_trigger()
    }

    /// See [`EguiAsync::new_trigger_for()`].
    #[must_use]
    #[cfg(feature = "sync")]
    #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
    pub fn new_trigger_for(&self, viewport_id: ViewportId) -> trigger::AsyncGlueTrigger {
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

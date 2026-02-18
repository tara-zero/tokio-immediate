// SPDX-License-Identifier: Apache-2.0 OR MIT

#![warn(clippy::pedantic)]

// TODO: Investigate why second window stops receiving repaint notifications when
//  * main windows is constantly repainting (because of an animation)
//  * main window is active
//  * second window is inactive but visible.

use ::std::sync::{Arc, Mutex};
use ::std::time::{Duration, Instant};

use ::egui::{
    Align, CentralPanel, Color32, Label, Layout, ProgressBar, Rect, RichText, TextEdit, Ui,
    UiBuilder, Vec2, ViewportBuilder, ViewportClass, ViewportId,
};
use ::futures_util::StreamExt as _;
use ::tokio_immediate_egui::sync::watch;
use ::tokio_immediate_egui::tokio::runtime::{Handle, Runtime};
use ::tokio_immediate_egui::trigger::AsyncGlueTrigger;
use ::tokio_immediate_egui::{AsyncGlue, AsyncGlueState, AsyncGlueWaker, EguiAsync};
use ::tokio_util::sync::CancellationToken;

const APP_NAME: &str = "egui + tokio-immediate";
const SECOND_WINDOW_ID: &str = "second-window";

fn main() -> ::eframe::Result {
    let runtime = Runtime::new().expect("Failed to create Tokio runtime");
    let _runtime_guard = runtime.enter();

    ::eframe::run_native(
        APP_NAME,
        ::eframe::NativeOptions {
            viewport: ViewportBuilder::default()
                .with_inner_size([400.0, 550.0])
                .with_min_inner_size([400.0, 550.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(ExampleApp::new(cc)))),
    )
}

struct ExampleApp {
    egui_async: EguiAsync,

    frame_n: usize,

    second_window: Arc<Mutex<Option<SecondWindow>>>,

    str_a: String,
    str_b: String,
    async_concat: AsyncGlue<String>,

    url: String,
    download: DownloadInner,
    async_download: AsyncGlue<DownloadResult>,
}

struct SecondWindow {
    frame_n: usize,

    sender: watch::Sender<usize>,
    receiver: watch::Receiver<usize>,
    async_counter: AsyncGlue<(), Handle>,
}

struct DownloadInner {
    cancel: tokio_util::sync::CancellationToken,
    sender: watch::Sender<Option<DownloadProgress>>,
    receiver: watch::Receiver<Option<DownloadProgress>>,
}

type DownloadResult = Result<DownloadProgress, String>;

struct DownloadProgress {
    downloaded: u64,
    size: u64,
    speed: u64,
}

impl ::eframe::App for ExampleApp {
    fn update(&mut self, ctx: &::egui::Context, frame: &mut ::eframe::Frame) {
        CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| self.update_ui(ui, frame))
        });
    }
}

impl ExampleApp {
    fn new(cc: &::eframe::CreationContext<'_>) -> Self {
        let egui_async = EguiAsync::default();
        cc.egui_ctx.add_plugin(egui_async.plugin());

        let async_concat = egui_async.new_glue_for_root();

        let download = DownloadInner::new(egui_async.new_waker_for_root());
        let async_download = egui_async.new_glue_for_root();

        Self {
            egui_async,

            frame_n: 0,

            second_window: Arc::new(Mutex::new(None)),

            str_a: String::from("abc"),
            str_b: String::from("123"),
            async_concat,

            url: String::from("http://speedtest.tele2.net/200MB.zip"),
            download,
            async_download,
        }
    }

    fn update_ui(&mut self, ui: &mut Ui, frame: &mut ::eframe::Frame) {
        ui.heading(APP_NAME);

        ui.separator();
        ui.columns_const::<3, _>(|columns| {
            columns[0].label(format!("Frame #{}", self.frame_n));
            self.frame_n += 1;

            columns[1].vertical_centered(|ui| {
                if let Some(cpu) = frame.info().cpu_usage {
                    ui.label(format!("{:.03} ms CPU", cpu * 1000.0));
                }
            });

            columns[2].with_layout(Layout::right_to_left(Align::Min), |ui| {
                ui.label(format!("{:.02} FPS", 1.0 / ui.ctx().input(|i| i.stable_dt)))
            });
        });

        // We use scopes with fixed IDs to make sure that all child widget IDs are stable
        // and thus things like focus and clicks are working as expected.
        ui.scope_builder(UiBuilder::new().id("ui_new_window"), |ui| {
            self.ui_new_window(ui);
        });
        ui.scope_builder(UiBuilder::new().id("ui_concat"), |ui| self.ui_concat(ui));
        ui.scope_builder(UiBuilder::new().id("ui_downloader"), |ui| {
            self.ui_download(ui);
        });
    }

    //
    // Asynchronous task with a separate window/viewport.
    //

    fn ui_new_window(&mut self, ui: &mut Ui) {
        ui.separator();

        let mut second_window = self.second_window.lock().expect("Poisoned by panic");

        let open_new_window = ui
            .add_enabled_ui(second_window.is_none(), |ui| {
                ui.button("Open new window").clicked()
            })
            .inner;
        if open_new_window {
            *second_window = Some(SecondWindow::new(&self.egui_async, Handle::current()));
        }

        if second_window.is_some() {
            let second_window = self.second_window.clone();
            ui.ctx().show_viewport_deferred(
                ViewportId::from_hash_of(SECOND_WINDOW_ID),
                ViewportBuilder::default()
                    .with_title(APP_NAME)
                    .with_inner_size([300.0, 200.0])
                    .with_min_inner_size([300.0, 200.0]),
                move |ctx, class| {
                    let mut second_window = second_window.lock().expect("Poisoned by panic");

                    let close = if let Some(second_window) = second_window.as_mut() {
                        second_window.update(ctx, class)
                    } else {
                        // Already closed.
                        false
                    };

                    if close {
                        *second_window = None;
                    }
                },
            );
        }
    }

    //
    // Simple asynchronous task: string concatenation.
    //

    fn ui_concat(&mut self, ui: &mut Ui) {
        let wide = Vec2::new(ui.available_width(), 0.0);

        ui.separator();
        ui.label("Simple asynchronous task:");
        ui.label("sleep for a few seconds");
        ui.label("then return the concatenated string");
        Self::ui_add_space(ui);

        self.async_concat.poll();

        let start_new_task = ui
            .add_enabled_ui(!self.async_concat.is_running(), |ui| {
                ui.add(TextEdit::singleline(&mut self.str_a).min_size(wide));
                ui.label("+");
                ui.add(TextEdit::singleline(&mut self.str_b).min_size(wide));

                ui.button("Concatenate").clicked()
            })
            .inner;

        if start_new_task {
            let _ = self
                .async_concat
                .start(Self::concat(self.str_a.clone(), self.str_b.clone()));
        }

        match &*self.async_concat {
            AsyncGlueState::Stopped => {
                Self::ui_concat_result(ui, true, "");
            }

            AsyncGlueState::Running(task) => {
                let rect = Self::ui_concat_result(ui, true, "");
                if Self::ui_waiting_with_abort(ui, rect, "Waiting...") {
                    task.abort();
                }
            }

            AsyncGlueState::Completed(value) => {
                Self::ui_concat_result(ui, false, value);
            }
        }
    }

    fn ui_concat_result(ui: &mut Ui, invisible: bool, mut result: &str) -> Rect {
        let wide = Vec2::new(ui.available_width(), 0.0);

        ui.scope(|ui| {
            if invisible {
                ui.set_invisible();
            }

            Self::ui_add_space(ui);
            ui.label("Result:");
            ui.add(TextEdit::singleline(&mut result).min_size(wide));
            Self::ui_add_space(ui);
        })
        .response
        .rect
    }

    async fn concat(mut str_a: String, str_b: String) -> String {
        tokio::time::sleep(Duration::from_secs(3)).await;
        str_a += &str_b;
        str_a
    }

    //
    // Complex asynchronous task: HTTP download.
    //

    fn ui_download(&mut self, ui: &mut Ui) {
        let wide = Vec2::new(ui.available_width(), 0.0);

        ui.separator();
        ui.label("Complex asynchronous task:");
        ui.label("download a file from a URL into /dev/null");
        ui.label("continuously report progress");
        ui.label("gracefully cancel using a cancellation handle");
        Self::ui_add_space(ui);

        self.async_download.poll();

        let start_new_task = ui
            .add_enabled_ui(!self.async_download.is_running(), |ui| {
                ui.add(TextEdit::singleline(&mut self.url).min_size(wide));
                ui.button("Speed test").clicked()
            })
            .inner;

        if start_new_task {
            // Clear leftovers of previous task.
            self.download.cancel = CancellationToken::new();
            self.download
                .sender
                .im_send(None)
                .expect("Impossible: progress receiver and sender are in the same struct");

            let trigger = self.egui_async.new_trigger_for_root();
            let _ = self.async_download.start(Self::download(
                self.url.clone(),
                self.download.cancel.clone(),
                trigger,
                self.download.sender.clone(),
            ));
        }

        match &*self.async_download {
            AsyncGlueState::Stopped => {}

            AsyncGlueState::Running(task) => {
                if let Some(progress) = self.download.receiver.borrow().as_ref() {
                    // Already have some download progress.
                    Self::ui_add_space(ui);

                    ui.add(ProgressBar::new(progress.percents()).show_percentage());

                    let (size, speed) = progress.progress();
                    ui.columns_const::<2, _>(|columns| {
                        columns[0]
                            .with_layout(Layout::right_to_left(Align::Min), |ui| ui.label(size));
                        columns[1].label(speed);
                    });

                    if ui.button("Cancel").clicked() {
                        self.download.cancel.cancel();
                    }
                } else {
                    // Still connecting.
                    if Self::ui_waiting_with_abort(
                        ui,
                        ui.available_rect_before_wrap(),
                        "Connecting...",
                    ) {
                        task.abort();
                    }
                }
            }

            AsyncGlueState::Completed(value) => {
                Self::ui_add_space(ui);

                match value {
                    Ok(completed) => {
                        let (size, speed) = completed.completed();
                        ui.label("Download completed!");
                        ui.columns_const::<2, _>(|columns| {
                            columns[0].with_layout(Layout::right_to_left(Align::Min), |ui| {
                                ui.label(size)
                            });
                            columns[1].label(speed);
                        });
                    }

                    Err(error) => {
                        ui.label(RichText::new("Download failed!").color(Color32::RED));
                        ui.add(Label::new(RichText::new(error).color(Color32::RED)).wrap());
                    }
                }
            }
        }
    }

    async fn download(
        url: String,
        cancel: CancellationToken,
        mut trigger: AsyncGlueTrigger,
        sender: watch::Sender<Option<DownloadProgress>>,
    ) -> Result<DownloadProgress, String> {
        let start = Instant::now();

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(4))
            .read_timeout(Duration::from_secs(4))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

        let file = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("Failed to start download: {e}"))?;
        if file.status().as_u16() != 200 {
            return Err(format!("HTTP {}", file.status()));
        }

        let size = file
            .content_length()
            .ok_or_else(|| String::from("Missing content length"))?;

        let mut stream = file.bytes_stream();
        let mut downloaded = 0;
        loop {
            tokio::select! {
                chunk = stream.next() => {
                    let Some(chunk) = chunk else {
                        break
                    };

                    let chunk = chunk.map_err(|e| format!("Failed to receive chunk: {e}"))?;
                    downloaded += chunk.len() as u64;
                },

                () = cancel.cancelled() => return Err(String::from("CANCELLED GRACEFULLY")),

                () = trigger.triggered() => DownloadProgress::new(downloaded, size, start.elapsed()).send(&sender),
            }
        }

        if downloaded == size {
            Ok(DownloadProgress::new(downloaded, size, start.elapsed()))
        } else {
            Err(format!("Size mismatch: {downloaded} != {size} (expected)"))
        }
    }

    //
    // Common UI.
    //

    fn ui_waiting_with_abort(ui: &mut Ui, rect: Rect, label: &str) -> bool {
        let mut ui = ui.new_child(UiBuilder::new().max_rect(rect));

        Self::ui_add_space(&mut ui);
        ui.columns_const::<3, _>(|columns| {
            columns[1].label(label);
            columns[2].spinner();
        });
        ui.button("Abort").clicked()
    }

    fn ui_add_space(ui: &mut Ui) {
        ui.add_space(ui.style().spacing.item_spacing.y * 4.0);
    }
}

impl SecondWindow {
    fn new(egui_async: &EguiAsync, runtime: Handle) -> Self {
        let viewport_id = ViewportId::from_hash_of(SECOND_WINDOW_ID);
        let (sender, receiver) =
            watch::channel_with_waker(0, egui_async.new_waker_for(viewport_id));
        Self {
            frame_n: 0,

            sender,
            receiver,
            async_counter: egui_async.new_glue_with_runtime_for(runtime, viewport_id),
        }
    }

    fn update(&mut self, ctx: &::egui::Context, _class: ViewportClass) -> bool {
        if ctx.input(|i| i.viewport().close_requested()) {
            return true;
        }

        CentralPanel::default().show(ctx, |ui| ui.vertical_centered(|ui| self.update_ui(ui)));

        false
    }

    fn update_ui(&mut self, ui: &mut Ui) {
        ui.columns_const::<2, _>(|columns| {
            columns[0].label(format!("Frame #{}", self.frame_n));
            self.frame_n += 1;

            columns[1].with_layout(Layout::right_to_left(Align::Min), |ui| {
                ui.label(format!("{:.02} FPS", 1.0 / ui.ctx().input(|i| i.stable_dt)))
            });
        });

        ui.separator();

        self.async_counter.poll();

        if self.async_counter.is_stopped() {
            let sender = self.sender.clone();
            let _ = self.async_counter.start(async move {
                let mut seconds = 0;
                loop {
                    sender.im_send_replace(seconds);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    seconds += 1;
                }
            });
        }

        if self.async_counter.is_running() {
            ui.label("Seconds passed:");
            ui.label(RichText::new(self.receiver.borrow().to_string()).size(100.0));
        }
    }
}

impl DownloadInner {
    fn new(waker: AsyncGlueWaker) -> Self {
        let (sender, receiver) = watch::channel_with_waker(None, waker);
        Self {
            cancel: CancellationToken::new(),
            sender,
            receiver,
        }
    }
}

impl DownloadProgress {
    #[expect(clippy::cast_possible_truncation)]
    #[expect(clippy::cast_sign_loss)]
    #[expect(clippy::cast_precision_loss)]
    fn new(downloaded: u64, size: u64, elapsed: Duration) -> Self {
        Self {
            downloaded,
            size,

            speed: if elapsed.is_zero() {
                0.0
            } else {
                downloaded as f64 / elapsed.as_secs_f64()
            } as u64,
        }
    }

    fn send(self, sender: &watch::Sender<Option<DownloadProgress>>) {
        sender
            .im_send(Some(self))
            .expect("Progress receiver closed before download finished");
    }

    #[expect(clippy::cast_possible_truncation)]
    #[expect(clippy::cast_precision_loss)]
    fn percents(&self) -> f32 {
        (if self.size == 0 {
            1.0
        } else {
            self.downloaded as f64 / self.size as f64
        }) as f32
    }

    fn progress(&self) -> (String, String) {
        let (dled, dled_unit) = Self::human_readable(self.downloaded);
        let (size, size_unit) = Self::human_readable(self.size);
        let (speed, speed_unit) = Self::human_readable(self.speed);
        (
            format!("{dled:.02} {dled_unit} / {size:.02} {size_unit}"),
            format!("{speed:.02} {speed_unit}/s"),
        )
    }

    fn completed(&self) -> (String, String) {
        let (size, size_unit) = Self::human_readable(self.size);
        let (speed, speed_unit) = Self::human_readable(self.speed);
        (
            format!("{size:.02} {size_unit}"),
            format!("{speed:.02} {speed_unit}/s"),
        )
    }

    #[expect(clippy::cast_precision_loss)]
    fn human_readable(value: u64) -> (f64, &'static str) {
        if value >= 1024 * 1024 * 1024 {
            ((value / 1024 / 1024) as f64 / 1024.0, "GiB")
        } else if value >= 1024 * 1024 {
            (value as f64 / 1024.0 / 1024.0, "MiB")
        } else if value >= 1024 {
            (value as f64 / 1024.0, "KiB")
        } else {
            (value as f64, "B")
        }
    }
}

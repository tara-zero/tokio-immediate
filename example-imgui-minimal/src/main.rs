// SPDX-License-Identifier: Apache-2.0 OR MIT

#![warn(clippy::pedantic)]

use ::std::net::SocketAddr;
use ::std::sync::Arc;
use ::std::time::Duration;

use ::dear_app::{AppBuilder, RedrawMode, RunnerConfig};
use ::dear_imgui_rs::{Condition, InputText, Ui, WindowFlags};
use ::tokio::net::TcpSocket;
use ::tokio::runtime::Runtime;
use ::tokio::time::timeout;
use ::tokio_immediate::{AsyncGlue, AsyncGlueState, AsyncGlueViewport, AsyncGlueWakeUp};
use ::winit::event::{Event, WindowEvent};

fn main() {
    let runtime = Runtime::new().expect("Failed to create Tokio runtime");
    let _runtime_guard = runtime.enter();

    let viewport = AsyncGlueViewport::default();
    let mut app = ExampleApp::new(viewport.clone());

    AppBuilder::new()
        .with_config(RunnerConfig {
            window_title: String::from("TCPing"),
            window_size: (300.0, 150.0),
            redraw: RedrawMode::Wait,
            ..RunnerConfig::default()
        })
        .on_gpu_init({
            move |window, _wgpu_device, _wgpu_queue, _wgpu_surface_conf| {
                let window = window.clone();
                let _ = viewport.replace_wake_up(Some(Arc::new(move || {
                    window.request_redraw();
                })));
            }
        })
        .on_event({
            |event, window, _ctx| {
                let should_redraw = !matches!(
                    event,
                    Event::WindowEvent {
                        event: WindowEvent::RedrawRequested,
                        ..
                    }
                );
                if should_redraw {
                    window.request_redraw();
                }
            }
        })
        .on_frame(move |ui, _addons| {
            app.render_ui(ui);
        })
        .run()
        .expect("Failed to run dear-app");
}

struct ExampleApp {
    frame_n: usize,
    viewport: AsyncGlueViewport,
    addr_port: String,
    tcping: AsyncGlue<Result<(), String>>,
}

impl ExampleApp {
    fn new(viewport: AsyncGlueViewport) -> Self {
        Self {
            frame_n: 0,
            tcping: viewport.new_glue(),
            viewport,
            addr_port: String::from("127.0.0.1:22"),
        }
    }

    fn render_ui(&mut self, ui: &Ui) {
        // Allow more redraw requests.
        self.viewport.woke_up();

        // Check for state updates for asynchronous task.
        self.tcping.poll();

        ui.window("##main")
            .position([0.0, 0.0], Condition::Always)
            .size(ui.io().display_size(), Condition::Always)
            .flags(WindowFlags::NO_DECORATION)
            .build(|| {
                ui.text(format!("Frame: {}", self.frame_n));
                self.frame_n += 1;

                // Always add address field with button, but make them disabled when task is already running.
                let start_new_task = {
                    let _disabled = ui.begin_disabled_with_cond(self.tcping.is_running());
                    InputText::new(ui, "##address", &mut self.addr_port).build();
                    ui.button("TCPing!")
                };

                if start_new_task {
                    // Start new asynchronous task.
                    let addr_port = self.addr_port.clone();
                    let _ = self.tcping.start(async move {
                        let addr_port: SocketAddr = addr_port
                            .parse()
                            .map_err(|e| format!("Failed to parse address: {e}"))?;

                        let socket = if addr_port.is_ipv4() {
                            TcpSocket::new_v4()
                        } else {
                            TcpSocket::new_v6()
                        }
                        .map_err(|e| format!("Failed to create TCP socket: {e}"))?;

                        timeout(Duration::from_secs(4), socket.connect(addr_port))
                            .await
                            .map_err(|_| String::from("TCP connect timed out"))?
                            .map_err(|e| format!("Failed to connect TCP socket: {e}"))?;

                        Ok(())
                    });
                }

                match &*self.tcping {
                    // Do not add any widgets when task was aborted or was never started.
                    AsyncGlueState::Stopped => {}

                    // Add spinner and "Abort" button when task is running.
                    AsyncGlueState::Running(task) => {
                        ui.text("Waiting...");
                        if ui.button("Abort") {
                            task.abort();
                        }
                    }

                    // Task completed, successfully.
                    AsyncGlueState::Completed(Ok(())) => {
                        ui.text("Port is open!");
                    }

                    // Task completed, with error.
                    AsyncGlueState::Completed(Err(error)) => {
                        ui.text("TCPing failed:");
                        ui.text(error);
                    }
                }
            });

        if ui.is_any_item_active() {
            // Immediately trigger the next frame if some element is active and may require animation.
            self.viewport.wake_up();
        }
    }
}

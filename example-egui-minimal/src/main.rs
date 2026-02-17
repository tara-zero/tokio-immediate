// SPDX-License-Identifier: Apache-2.0 OR MIT

#![warn(clippy::pedantic)]

use ::std::net::SocketAddr;
use ::std::time::Duration;

use ::egui::{CentralPanel, ViewportBuilder};
use ::tokio_immediate_egui::tokio::net::TcpSocket;
use ::tokio_immediate_egui::tokio::runtime::Runtime;
use ::tokio_immediate_egui::tokio::time::timeout;
use ::tokio_immediate_egui::{AsyncGlue, AsyncGlueState, EguiAsync};

fn main() -> ::eframe::Result {
    let runtime = Runtime::new().expect("Failed to create Tokio runtime");
    let _runtime_guard = runtime.enter();

    ::eframe::run_native(
        "TCPing",
        ::eframe::NativeOptions {
            viewport: ViewportBuilder::default()
                .with_inner_size([300.0, 100.0])
                .with_min_inner_size([300.0, 100.0]),
            ..Default::default()
        },
        Box::new(|cc| {
            let egui_async = EguiAsync::default();
            cc.egui_ctx.add_plugin(egui_async.plugin());

            let tcping = egui_async.new_glue();

            Ok(Box::new(ExampleApp {
                _egui_async: egui_async,
                addr_port: String::from("127.0.0.1:22"),
                tcping,
            }))
        }),
    )
}

struct ExampleApp {
    _egui_async: EguiAsync,
    addr_port: String,
    tcping: AsyncGlue<Result<(), String>>,
}

impl ::eframe::App for ExampleApp {
    fn update(&mut self, ctx: &::egui::Context, _frame: &mut ::eframe::Frame) {
        CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                // Check for state updates for asynchronous task.
                self.tcping.poll();

                // Always add address field with button, but make them disabled when task is already running.
                let start_new_task = ui
                    .add_enabled_ui(!self.tcping.is_running(), |ui| {
                        ui.text_edit_singleline(&mut self.addr_port);
                        ui.button("TCPing!").clicked()
                    })
                    .inner;

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

                    // Add spinner and an "Abort" button when task is running.
                    AsyncGlueState::Running(task) => {
                        ui.spinner();
                        if ui.button("Abort").clicked() {
                            task.abort();
                        }
                    }

                    // Task completed, successfully.
                    AsyncGlueState::Completed(Ok(())) => {
                        ui.label("Port is open!");
                    }

                    // Task completed, with an error.
                    AsyncGlueState::Completed(Err(error)) => {
                        ui.label("TCPing failed:");
                        ui.label(error);
                    }
                }
            })
        });
    }
}

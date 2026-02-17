// SPDX-License-Identifier: Apache-2.0 OR MIT

#![warn(clippy::pedantic)]

use ::std::net::SocketAddr;
use ::std::sync::Arc;
use ::std::time::Duration;

use ::crossterm::event::{Event, EventStream, KeyCode, KeyModifiers};
use ::futures_util::StreamExt as _;
use ::ratatui::Frame;
use ::ratatui::layout::{Constraint, Layout};
use ::ratatui::style::{Color, Style, Styled, Stylize};
use ::ratatui::text::{Line, ToSpan};
use ::ratatui::widgets::{Block, Paragraph};
use ::tokio::net::TcpSocket;
use ::tokio::runtime::Runtime;
use ::tokio::select;
use ::tokio::sync::watch;
use ::tokio::time::timeout;
use ::tokio_immediate::{AsyncGlue, AsyncGlueState, AsyncGlueViewport, AsyncGlueWakeUp};
use ::tui_input::Input;
use ::tui_input::backend::crossterm::EventHandler;

fn main() {
    let runtime = Runtime::new().expect("Failed to create Tokio runtime");
    let _runtime_guard = runtime.enter();

    let _restore = RestoreGuard;
    let mut terminal = ::ratatui::init();
    let mut events = EventStream::new();

    let (sender, mut receiver) = watch::channel(());
    receiver.mark_changed();
    let viewport = AsyncGlueViewport::new_with_wake_up(Arc::new(move || sender.send_replace(())));
    let mut app = ExampleApp::new(viewport);

    loop {
        let event = runtime.block_on(async {
            select! {
                event = events.next() => Some(event.expect("Event stream ended unexpectedly")),

                result = receiver.changed() => {
                    result.expect("Repaint request receiver failed");
                    None
                },
            }
        });
        let Some(event) = event else {
            // Repaint request.
            terminal
                .draw(|frame| app.render_ui(frame))
                .expect("draw() failed");
            continue;
        };
        // Terminal event.
        let event = event.expect("Got error event");

        if app.handle_event(&event) {
            break;
        }
    }
}

struct RestoreGuard;

impl Drop for RestoreGuard {
    fn drop(&mut self) {
        ::ratatui::restore();
    }
}

struct ExampleApp {
    frame_n: usize,
    viewport: AsyncGlueViewport,
    addr_port: Input,
    tcping: AsyncGlue<Result<(), String>>,
}

impl ExampleApp {
    fn new(viewport: AsyncGlueViewport) -> Self {
        Self {
            frame_n: 0,
            tcping: viewport.new_glue(),
            viewport,
            addr_port: Input::new(String::from("127.0.0.1:22")),
        }
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        if let Event::Key(key) = event
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            match key.code {
                // Close this app.
                KeyCode::Char('q' | 'w') => return true,

                KeyCode::Char('c') => {
                    if let AsyncGlueState::Running(join_handle) = &*self.tcping {
                        // Abort task if it is running.
                        join_handle.abort();
                    }
                }

                _ => {}
            }
        } else if let Event::Key(key) = event
            && (key.code == KeyCode::Enter)
            && !self.tcping.is_running()
        {
            // Start new asynchronous task.
            let addr_port = String::from(self.addr_port.value());
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
        } else if self.addr_port.handle_event(event).is_some() {
            // Pass all other events to the input field.
            self.viewport.wake_up();
        }

        false
    }

    fn render_ui(&mut self, frame: &mut Frame) {
        // Allow more redraw requests.
        self.viewport.woke_up();

        // Check for state updates for asynchronous task.
        self.tcping.poll();

        let [frame_n_area, help_area, input_area, result_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .areas(frame.area());

        // Current frame number.
        frame.render_widget(Line::from(format!("Frame: {}", self.frame_n)), frame_n_area);
        self.frame_n += 1;

        // Help message.
        let help_message = Line::from_iter([
            "Press ".to_span(),
            "Ctrl-Q".bold(),
            " or ".to_span(),
            "Ctrl-W".bold(),
            " to exit, ".to_span(),
            "Enter".bold(),
            " to start TCPing, or ".to_span(),
            "Ctrl-C".bold(),
            " to abort running TCPing.".to_span(),
        ]);
        frame.render_widget(help_message, help_area);

        // Address:port input.
        let width = input_area.width.max(3) - 3;
        let scroll = self.addr_port.visual_scroll(width as usize);
        let style = if self.tcping.is_running() {
            Style::default()
        } else {
            Color::Yellow.into()
        };
        let input = Paragraph::new(self.addr_port.value())
            .style(style)
            .scroll((
                0,
                u16::try_from(scroll).expect("`scroll` value is too large"),
            ))
            .block(Block::bordered().title("ADDRESS:PORT"));
        frame.render_widget(input, input_area);

        if !self.tcping.is_running() {
            // Ratatui hides the cursor unless it's explicitly set. Position the  cursor past the
            // end of the input text and one line down from the border to the input line
            let x = self.addr_port.visual_cursor().max(scroll) - scroll + 1;
            frame.set_cursor_position((
                input_area.x + u16::try_from(x).expect("`x` value is too large"),
                input_area.y + 1,
            ));
        }

        // Task result.
        match &*self.tcping {
            // Do not add any widgets when task was aborted or was never started.
            AsyncGlueState::Stopped => {}

            // Task is running.
            AsyncGlueState::Running(_) => {
                frame.render_widget(Line::from("Waiting..."), result_area);
            }

            // Task completed, successfully.
            AsyncGlueState::Completed(Ok(())) => {
                let result = Line::from("Port is open!".set_style(Color::Green));
                frame.render_widget(result, result_area);
            }

            // Task completed, with an error.
            AsyncGlueState::Completed(Err(error)) => {
                let error =
                    Line::from_iter(["TCPing failed: ".set_style(Color::Red), error.to_span()]);
                frame.render_widget(error, result_area);
            }
        }
    }
}

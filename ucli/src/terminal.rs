use std::io::Write;

use crossbeam::channel::{Receiver, Sender};
use crossterm::{
    style::{Color, SetForegroundColor},
    ExecutableCommand,
};

use common::ServerMessage;

#[derive(Clone)]
pub struct TerminalWriter {
    inner: Sender<Output>,
}

impl TerminalWriter {
    pub fn write_server_msg(&self, item: ServerMessage) {
        self.inner.send(Output::ServerMessage(item)).unwrap();
    }
}

enum Output {
    ServerMessage(ServerMessage),
}

impl Output {
    fn print_to_console<T: Write, U: Write>(&self, stdout: &mut T, stderr: &mut U) {
        match self {
            Self::ServerMessage(ServerMessage::IsBusy) => {
                todo!();
            }
            Self::ServerMessage(ServerMessage::UnityConsoleOutput {
                log_type,
                log,
                stack_trace,
            }) => {
                todo!();
            }
            Self::ServerMessage(ServerMessage::CommandFinished { is_success, msg }) => {
                todo!();
            }
            _ => {
                todo!();
            }
        }
        stdout.execute(SetForegroundColor(Color::Red)).unwrap();
    }
}

pub fn print_loop<T: Write + Send + 'static, U: Write + Send + 'static>(
    mut stdout: T,
    mut stderr: U,
) -> TerminalWriter {
    let (tx, rx) = crossbeam::channel::unbounded::<Output>();

    std::thread::spawn(move || {
        while let Ok(output) = rx.recv() {
            output.print_to_console(&mut stdout, &mut stderr);
        }
    });

    TerminalWriter { inner: tx }
}

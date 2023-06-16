use std::io::{Stdout, Stderr};

use common::ServerMessage;
use crossbeam::channel::Receiver;
use crossterm::{ExecutableCommand, style::{SetForegroundColor, Color}};

pub enum Output {
    ServerMessage(ServerMessage),
}

impl Output {
    fn print_to_console(&self, stdout: &mut Stdout, stderr: &mut Stderr) {
        match self {
            Self::ServerMessage(ServerMessage::IsBusy) => {
                todo!();
            }
            Self::ServerMessage(ServerMessage::UnityConsoleOutput { log_type, log, stack_trace }) => {
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

pub fn print_loop(rx: Receiver<Output>) {
    std::thread::spawn(move || {
        let mut stdout = std::io::stdout();
        let mut stderr = std::io::stderr();

        while let Ok(output) = rx.recv() {
            output.print_to_console(&mut stdout, &mut stderr);
        }
    });
}

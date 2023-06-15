use crossbeam::channel::Receiver;

pub struct Output {

}

pub fn print_loop(rx: Receiver<Output>) {
    std::thread::spawn(move || {
        while let Ok(output) = rx.recv() {

        }
    });
}

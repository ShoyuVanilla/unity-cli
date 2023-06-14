use ucli::{cli_args::get_cli_args, run};

pub fn main() {
    let args = get_cli_args();
    run(args);
}

use cli_args::CliArgs;

pub mod cli_args;
mod service_discovery;
mod terminal;

pub async fn run(args: CliArgs) {
    match args {
        CliArgs::ListSessions { discovery_args } => {

        }
        CliArgs::Compile { discovery_args } => {

        }
        CliArgs::Run { command, args, discovery_args } => {

        }
        _ => {
            todo!()
        }
    }
}

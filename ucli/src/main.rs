use ucli::{cli_args::get_cli_args, run};

#[tokio::main]
pub async fn main() {
    let args = get_cli_args();
    run(args).await
}

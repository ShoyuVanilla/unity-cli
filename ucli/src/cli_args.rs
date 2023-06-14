use std::{path::PathBuf, time::Duration};

use clap::{arg, ArgMatches, Command, ValueHint};

#[derive(Debug, PartialEq)]
pub enum CliArgs {
    ListSessions {
        discovery_args: DiscoveryArgs,
    },
    Compile {
        discovery_args: DiscoveryArgs,
    },
    Run {
        command: String,
        args: Vec<String>,
        discovery_args: DiscoveryArgs,
    },
    ListCommands {
        discovery_args: DiscoveryArgs,
    },
}

#[derive(Debug, PartialEq)]
pub struct DiscoveryArgs {
    pub path: Option<PathBuf>,
    pub project: Option<String>,
    pub session: Option<String>,
    pub discovery_timeout: Option<Duration>,
}

pub fn get_cli_args() -> CliArgs {
    parse_args(&cli().get_matches())
}

fn cli() -> Command {
    Command::new("ucli")
        .about("A command line interface for Unity game engine")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("list-sessions")
                .about("List available Unity sessions")
                .args(session_discovery_args()),
        )
        .subcommand(
            Command::new("compile")
                .about("Compiles project scripts")
                .args(session_discovery_args()),
        )
        .subcommand(
            Command::new("run")
                .about("Run custom command")
                .args(session_discovery_args())
                .arg(arg!(command: <cmd>))
                .arg(arg!(args: [args] ...).trailing_var_arg(true))
                .arg_required_else_help(true),
        )
        .subcommand(
            Command::new("list-commands")
                .about("List available custom commands")
                .args(session_discovery_args()),
        )
}

fn session_discovery_args() -> Vec<clap::Arg> {
    vec![
        arg!(--path[PATH])
            .value_hint(ValueHint::DirPath)
            .value_parser(clap::value_parser!(PathBuf)),
        arg!(--project[NAME]),
        arg!(--session[NAME]),
        arg!(--"discovery-timeout"[ms]).value_parser(clap::value_parser!(u64)),
    ]
}

fn parse_args(matches: &ArgMatches) -> CliArgs {
    match matches.subcommand() {
        Some(("list-sessions", sub_matches)) => CliArgs::ListSessions {
            discovery_args: parse_discovery_args(sub_matches),
        },
        Some(("compile", sub_matches)) => CliArgs::Compile {
            discovery_args: parse_discovery_args(sub_matches),
        },
        Some(("run", sub_matches)) => CliArgs::Run {
            command: sub_matches
                .get_one::<String>("command")
                .map(String::to_owned)
                .unwrap(),
            args: sub_matches
                .get_many::<String>("args")
                .unwrap()
                .map(String::to_owned)
                .collect(),
            discovery_args: parse_discovery_args(sub_matches),
        },
        Some(("list-commands", sub_matches)) => CliArgs::ListCommands {
            discovery_args: parse_discovery_args(sub_matches),
        },
        _ => unreachable!(),
    }
}

fn parse_discovery_args(matches: &ArgMatches) -> DiscoveryArgs {
    DiscoveryArgs {
        path: matches.get_one::<PathBuf>("path").map(PathBuf::to_owned),
        project: matches.get_one::<String>("project").map(String::to_owned),
        session: matches.get_one::<String>("session").map(String::to_owned),
        discovery_timeout: matches
            .get_one::<u64>("discovery-timeout")
            .map(|v| Duration::from_millis(v.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use crate::cli_args::{cli, parse_args, CliArgs, DiscoveryArgs};

    #[test]
    fn parse_list_sessions_subcommand() {
        let matches =
            cli().get_matches_from(vec!["ucli", "list-sessions", "--path", "foo/bar/baz"]);
        let parsed = parse_args(&matches);

        assert_eq!(
            CliArgs::ListSessions {
                discovery_args: DiscoveryArgs {
                    path: Some(PathBuf::from("foo/bar/baz")),
                    project: None,
                    session: None,
                    discovery_timeout: None,
                }
            },
            parsed
        );
    }

    #[test]
    fn parse_compile_command() {
        let matches = cli().get_matches_from(vec!["ucli", "compile"]);
        let parsed = parse_args(&matches);

        assert_eq!(
            CliArgs::Compile {
                discovery_args: DiscoveryArgs {
                    path: None,
                    project: None,
                    session: None,
                    discovery_timeout: None,
                }
            },
            parsed
        );
    }

    #[test]
    fn parse_run_command() {
        let matches = cli().get_matches_from(vec![
            "ucli",
            "run",
            "--discovery-timeout",
            "500",
            "--session",
            "foo-bar",
            "foo",
            "--",
            "--bar",
            "baz",
            "--",
            "foo/bar",
        ]);
        let parsed = parse_args(&matches);

        assert_eq!(
            CliArgs::Run {
                command: "foo".to_owned(),
                args: vec!["--bar", "baz", "--", "foo/bar"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                discovery_args: DiscoveryArgs {
                    path: None,
                    project: None,
                    session: Some(String::from("foo-bar")),
                    discovery_timeout: Some(Duration::from_millis(500)),
                }
            },
            parsed
        );
    }

    #[test]
    fn parse_list_command_command() {
        let matches = cli().get_matches_from(vec![
            "ucli",
            "list-commands",
            "--project",
            "My Unity Project",
        ]);
        let parsed = parse_args(&matches);

        assert_eq!(
            CliArgs::ListCommands {
                discovery_args: DiscoveryArgs {
                    path: None,
                    project: Some(String::from("My Unity Project")),
                    session: None,
                    discovery_timeout: None,
                }
            },
            parsed
        );
    }
}

//! Server commands, from two sources:
//!
//! * the server's own stdin (operator, full permissions)
//! * a player's chat message starting with `/` (player permissions)
//!
//! Both go through the same parser and the same dispatcher, so a command
//! can't accidentally exist for one and not the other, and permissions
//! are checked in exactly one place.
//!
//! Parsing is deliberately separated from execution: `parse` is a pure
//! function over a string, so the whole surface -- unknown commands,
//! missing arguments, bad numbers, permission levels -- is testable
//! without a running server, a socket, or a world.

use std::fmt;

use primitive_shared::protocol::PlayerId;

/// Who is asking. Operators type into the server console; players type
/// into chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Permission {
    Player,
    Operator,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Help,
    /// Who's online.
    List,
    /// Broadcast a message to everyone.
    Say(String),
    /// Report or set the time of day (0.0..1.0, or a named phase).
    Time(Option<f32>),
    /// Where the caller is.
    Where,
    /// Teleport to absolute coordinates.
    Teleport { x: f32, y: f32, z: f32 },
    /// Teleport to the world spawn.
    Spawn,
    /// Server counters: uptime, players, chunks, ticks.
    Stats,
    /// Flush the world to disk now.
    Save,
    /// Disconnect a player by name.
    Kick { username: String, reason: String },
    /// Save and shut down.
    Stop,
}

impl Command {
    /// Minimum permission needed to run it.
    pub fn required_permission(&self) -> Permission {
        match self {
            // Read-only or self-affecting: anyone.
            Command::Help
            | Command::List
            | Command::Where
            | Command::Spawn
            | Command::Stats
            | Command::Time(None) => Permission::Player,
            // Anything that affects other people or the world.
            Command::Say(_)
            | Command::Time(Some(_))
            | Command::Teleport { .. }
            | Command::Save
            | Command::Kick { .. }
            | Command::Stop => Permission::Operator,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    Empty,
    Unknown(String),
    /// Wrong or missing arguments; carries the usage line.
    Usage(&'static str),
    BadNumber(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => write!(f, "empty command"),
            ParseError::Unknown(name) => {
                write!(f, "unknown command '{name}' -- try /help")
            }
            ParseError::Usage(usage) => write!(f, "usage: {usage}"),
            ParseError::BadNumber(s) => write!(f, "'{s}' is not a number"),
        }
    }
}

pub const HELP_TEXT: &[&str] = &[
    "/help                 - this list",
    "/list                 - who is online",
    "/where                - your position",
    "/spawn                - teleport to spawn",
    "/stats                - server counters",
    "/time                 - show the time of day",
    "/time <0..1|day|night|noon|midnight>  - set it (operator)",
    "/tp <x> <y> <z>       - teleport (operator)",
    "/say <message>        - broadcast (operator)",
    "/save                 - flush the world to disk (operator)",
    "/kick <player> [why]  - disconnect someone (operator)",
    "/stop                 - save and shut down (operator)",
];

/// Parses one command line. A leading `/` is optional, so the same text
/// works from chat (`/list`) and from the console (`list`).
pub fn parse(line: &str) -> Result<Command, ParseError> {
    let line = line.trim();
    let line = line.strip_prefix('/').unwrap_or(line);
    if line.is_empty() {
        return Err(ParseError::Empty);
    }

    let mut parts = line.split_whitespace();
    let name = parts.next().ok_or(ParseError::Empty)?.to_ascii_lowercase();
    let rest: Vec<&str> = parts.collect();

    match name.as_str() {
        "help" | "?" => Ok(Command::Help),
        "list" | "who" | "players" => Ok(Command::List),
        "where" | "pos" => Ok(Command::Where),
        "spawn" => Ok(Command::Spawn),
        "stats" | "tps" => Ok(Command::Stats),
        "save" => Ok(Command::Save),
        "stop" | "quit" | "shutdown" => Ok(Command::Stop),

        "say" | "broadcast" => {
            if rest.is_empty() {
                return Err(ParseError::Usage("/say <message>"));
            }
            Ok(Command::Say(rest.join(" ")))
        }

        "time" => match rest.first() {
            None => Ok(Command::Time(None)),
            Some(arg) => Ok(Command::Time(Some(parse_time(arg)?))),
        },

        "tp" | "teleport" => {
            if rest.len() != 3 {
                return Err(ParseError::Usage("/tp <x> <y> <z>"));
            }
            let coord = |s: &str| {
                s.parse::<f32>()
                    .map_err(|_| ParseError::BadNumber(s.to_string()))
            };
            Ok(Command::Teleport {
                x: coord(rest[0])?,
                y: coord(rest[1])?,
                z: coord(rest[2])?,
            })
        }

        "kick" => {
            if rest.is_empty() {
                return Err(ParseError::Usage("/kick <player> [reason]"));
            }
            Ok(Command::Kick {
                username: rest[0].to_string(),
                reason: if rest.len() > 1 {
                    rest[1..].join(" ")
                } else {
                    "kicked by an operator".to_string()
                },
            })
        }

        other => Err(ParseError::Unknown(other.to_string())),
    }
}

/// Accepts either a raw 0..1 fraction or a named phase. Named phases
/// exist because "0.75" is not how anyone thinks about sunset.
fn parse_time(arg: &str) -> Result<f32, ParseError> {
    match arg.to_ascii_lowercase().as_str() {
        "midnight" => Ok(0.0),
        "sunrise" | "dawn" => Ok(0.25),
        "day" | "noon" => Ok(0.5),
        "sunset" | "dusk" => Ok(0.75),
        "night" => Ok(0.85),
        other => {
            let value: f32 = other
                .parse()
                .map_err(|_| ParseError::BadNumber(other.to_string()))?;
            if !value.is_finite() {
                return Err(ParseError::BadNumber(other.to_string()));
            }
            Ok(value.rem_euclid(1.0))
        }
    }
}

/// What the caller should do with the result. Kept as data rather than
/// having the command mutate the world directly, so dispatch stays
/// testable and the side effects all happen in one place in `main.rs`.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// Text back to whoever asked.
    Reply(Vec<String>),
    /// Text to everyone.
    Broadcast(String),
    SetTime(f32),
    TeleportSelf { x: f32, y: f32, z: f32 },
    TeleportSelfToSpawn,
    Kick { username: String, reason: String },
    Save,
    Stop,
    Denied(String),
}

/// Turns a parsed command into an action, enforcing permissions.
/// `caller` is `None` for the console.
pub fn authorize(command: Command, permission: Permission, caller: Option<PlayerId>) -> Response {
    if permission < command.required_permission() {
        return Response::Denied(format!(
            "'{}' is operator-only",
            command_name(&command)
        ));
    }

    match command {
        Command::Help => Response::Reply(HELP_TEXT.iter().map(|s| s.to_string()).collect()),
        Command::List => Response::Reply(vec!["__LIST__".to_string()]),
        Command::Stats => Response::Reply(vec!["__STATS__".to_string()]),
        Command::Where => {
            if caller.is_none() {
                // The console isn't standing anywhere.
                Response::Reply(vec!["the console has no position".to_string()])
            } else {
                Response::Reply(vec!["__WHERE__".to_string()])
            }
        }
        Command::Spawn => {
            if caller.is_none() {
                Response::Reply(vec!["the console can't teleport".to_string()])
            } else {
                Response::TeleportSelfToSpawn
            }
        }
        Command::Teleport { x, y, z } => {
            if caller.is_none() {
                Response::Reply(vec!["the console can't teleport".to_string()])
            } else {
                Response::TeleportSelf { x, y, z }
            }
        }
        Command::Say(text) => Response::Broadcast(text),
        Command::Time(None) => Response::Reply(vec!["__TIME__".to_string()]),
        Command::Time(Some(t)) => Response::SetTime(t),
        Command::Save => Response::Save,
        Command::Kick { username, reason } => Response::Kick { username, reason },
        Command::Stop => Response::Stop,
    }
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Help => "help",
        Command::List => "list",
        Command::Say(_) => "say",
        Command::Time(_) => "time",
        Command::Where => "where",
        Command::Teleport { .. } => "tp",
        Command::Spawn => "spawn",
        Command::Stats => "stats",
        Command::Save => "save",
        Command::Kick { .. } => "kick",
        Command::Stop => "stop",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leading_slash_is_optional() {
        assert_eq!(parse("/list"), Ok(Command::List));
        assert_eq!(parse("list"), Ok(Command::List));
        assert_eq!(parse("  /LIST  "), Ok(Command::List));
    }

    #[test]
    fn aliases_work() {
        assert_eq!(parse("who"), Ok(Command::List));
        assert_eq!(parse("?"), Ok(Command::Help));
        assert_eq!(parse("quit"), Ok(Command::Stop));
    }

    #[test]
    fn unknown_commands_say_so_instead_of_being_ignored() {
        match parse("/fly") {
            Err(ParseError::Unknown(name)) => assert_eq!(name, "fly"),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn teleport_needs_three_finite_numbers() {
        assert_eq!(
            parse("/tp 1 2 3"),
            Ok(Command::Teleport {
                x: 1.0,
                y: 2.0,
                z: 3.0
            })
        );
        assert!(matches!(parse("/tp 1 2"), Err(ParseError::Usage(_))));
        assert!(matches!(parse("/tp a b c"), Err(ParseError::BadNumber(_))));
    }

    #[test]
    fn time_accepts_names_and_fractions() {
        assert_eq!(parse("/time"), Ok(Command::Time(None)));
        assert_eq!(parse("/time noon"), Ok(Command::Time(Some(0.5))));
        assert_eq!(parse("/time midnight"), Ok(Command::Time(Some(0.0))));
        assert_eq!(parse("/time 0.25"), Ok(Command::Time(Some(0.25))));
    }

    #[test]
    fn out_of_range_times_wrap_instead_of_breaking_the_sky() {
        // 1.25 is a quarter past the start of the next day.
        assert_eq!(parse("/time 1.25"), Ok(Command::Time(Some(0.25))));
        assert_eq!(parse("/time -0.25"), Ok(Command::Time(Some(0.75))));
        assert!(matches!(parse("/time nan"), Err(ParseError::BadNumber(_))));
        assert!(matches!(parse("/time inf"), Err(ParseError::BadNumber(_))));
    }

    #[test]
    fn say_keeps_the_whole_message_together() {
        assert_eq!(
            parse("/say hello there everyone"),
            Ok(Command::Say("hello there everyone".to_string()))
        );
        assert!(matches!(parse("/say"), Err(ParseError::Usage(_))));
    }

    #[test]
    fn kick_defaults_its_reason() {
        match parse("/kick alice") {
            Ok(Command::Kick { username, reason }) => {
                assert_eq!(username, "alice");
                assert!(!reason.is_empty(), "a kicked player deserves a reason");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            parse("/kick bob being rude"),
            Ok(Command::Kick {
                username: "bob".to_string(),
                reason: "being rude".to_string()
            })
        );
    }

    #[test]
    fn players_cannot_run_operator_commands() {
        for line in ["/stop", "/kick alice", "/say hi", "/time noon", "/tp 0 0 0"] {
            let command = parse(line).unwrap();
            let response = authorize(command, Permission::Player, Some(1));
            assert!(
                matches!(response, Response::Denied(_)),
                "{line} must be refused for a plain player, got {response:?}"
            );
        }
    }

    #[test]
    fn players_can_run_the_harmless_ones() {
        for line in ["/help", "/list", "/where", "/spawn", "/stats", "/time"] {
            let command = parse(line).unwrap();
            let response = authorize(command, Permission::Player, Some(1));
            assert!(
                !matches!(response, Response::Denied(_)),
                "{line} should be allowed for a player"
            );
        }
    }

    #[test]
    fn the_console_is_an_operator_but_has_no_body() {
        let stop = authorize(parse("/stop").unwrap(), Permission::Operator, None);
        assert_eq!(stop, Response::Stop);

        // It can't teleport itself anywhere -- there's nothing to move.
        let tp = authorize(parse("/tp 1 2 3").unwrap(), Permission::Operator, None);
        assert!(matches!(tp, Response::Reply(_)), "got {tp:?}");
    }

    #[test]
    fn an_operator_teleport_from_a_player_moves_that_player() {
        let tp = authorize(parse("/tp 5 6 7").unwrap(), Permission::Operator, Some(3));
        assert_eq!(
            tp,
            Response::TeleportSelf {
                x: 5.0,
                y: 6.0,
                z: 7.0
            }
        );
    }

    #[test]
    fn help_lists_every_command() {
        // Guard against adding a command and forgetting to document it.
        let documented = HELP_TEXT.join(" ");
        for name in [
            "/help", "/list", "/where", "/spawn", "/stats", "/time", "/tp", "/say", "/save",
            "/kick", "/stop",
        ] {
            assert!(documented.contains(name), "{name} is missing from /help");
        }
    }
}

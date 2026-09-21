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

/// Which of the two a caller holds, from the two facts that decide it:
/// whether this is a world of their own, and what the profiles say about
/// their UUID (`None` for a connection that has not got one).
///
/// **One rule, because it is now asked in two places.** It has always been
/// worked out where a chat line arrives; the give menu asks the same
/// question outright before it draws itself
/// (`protocol::ClientMessage::AmIAnOperator`). Written twice, the menu and
/// the command could disagree about who may run `/give` -- a page that
/// opens and then refuses, or worse, one that hides itself from somebody
/// the server would have obeyed.
pub fn permission_for(local_operator: bool, profile_says_operator: Option<bool>) -> Permission {
    // Свой мир: единственный игрок — оператор по построению. См.
    // `RunOptions::local_operator`.
    if local_operator || profile_says_operator == Some(true) {
        return Permission::Operator;
    }
    // No profile, no authority. Unreachable in practice -- the UUID is set
    // before the handle is published -- but the fallback that costs
    // nothing is the one that grants nothing.
    Permission::Player
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Help,
    /// Who's online.
    List,
    /// Everyone the server has ever seen: names, UUIDs, and where they
    /// left off.
    Profiles,
    /// Broadcast a message to everyone.
    Say(String),
    /// Report or set the time of day (0.0..1.0, or a named phase).
    Time(Option<f32>),
    /// Report or set the weather.
    ///
    /// The same shape as `/time` and for the same reason: an operator
    /// wants to know what the sky is doing far more often than they want
    /// to change it, so the reporting form is the one without an
    /// argument and the one anybody may run.
    ///
    /// `Some(None)` is `/weather auto`: hand the sky back to the
    /// countdown after it has been held somewhere by hand.
    Weather(Option<Option<primitive_shared::weather::Weather>>),
    /// Where the caller is.
    Where,
    /// Teleport to absolute coordinates.
    Teleport { x: f32, y: f32, z: f32 },
    /// Teleport to the world spawn.
    Spawn,
    /// Teleport to the nearest chunk of a named biome.
    ///
    /// **An operator's command and a test tool before it is anything
    /// else.** "Does the bog look right" and "do wolves spawn in a taiga"
    /// are questions that used to cost twenty minutes of walking or a
    /// world generated until one appeared under the spawn point, and a
    /// scenario cannot walk at all. It carries the name as typed rather
    /// than a parsed `Biome` for one reason: an unknown name has to come
    /// back as a list of the ones that exist, and the list belongs with
    /// the reply rather than with the parser.
    BiomeTeleport { biome: String },
    /// Throw a bolt of lightning: at the top of a named column, or --
    /// with no argument -- wherever the storm would have put one near
    /// the caller (`logic::lightning::Storm::draw`).
    ///
    /// **Because a bolt cannot be waited for.** It falls about once a
    /// minute and only in a storm, which is fine for a player and
    /// useless for everybody who has to *check* what it does: a
    /// scenario cannot wait five minutes, and a phone cannot be driven
    /// at all (see CLAUDE.md on why anything that has to be verified has
    /// to be reachable from a command or an environment variable). The
    /// column form is the deterministic one -- plant a tree, strike it,
    /// assert it burns.
    ///
    /// It does not need a storm overhead, deliberately: an operator
    /// asking for a bolt has said what they want more plainly than the
    /// sky could.
    Lightning { at: Option<(i32, i32)> },
    /// Server counters: uptime, players, chunks, ticks.
    Stats,
    /// What is extending this server: the scripted plugins and the
    /// native mods, with what each of them says about itself.
    ///
    /// One command for both, because from an operator's side they are
    /// one question -- "what is running here" -- and two commands would
    /// mean somebody diagnosing a strange server has to know which kind
    /// of thing to suspect before they can look.
    Extensions,
    /// Flush the world to disk now.
    Save,
    /// Put blocks straight into the caller's pack.
    ///
    /// Operator-only, and the one command that can make something out of
    /// nothing -- which is exactly why it is worth having: testing what
    /// happens to a chest full of stone should not require mining a
    /// chest full of stone.
    Give { block: String, count: u32 },
    /// Disconnect a player by name.
    Kick { username: String, reason: String },
    /// Hand a player the console's permissions, by name.
    ///
    /// The one command that changes who may run commands, which makes
    /// it the one command whose permission check matters most: a player
    /// who could run it would be a player who could grant themselves
    /// everything else in this list. Hence `Operator`, like `give` and
    /// for the same reason -- except that where `give` makes blocks out
    /// of nothing, this makes operators out of nothing.
    Op { username: String },
    /// Take it away again.
    Deop { username: String },
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
            | Command::Profiles
            | Command::Where
            | Command::Spawn
            | Command::Stats
            // Read-only, and worth being open: "what is running on this
            // server" is a question a player is entitled to an answer
            // to, and a server that hid it would be a server where
            // nobody can tell a mod's behaviour from a bug.
            | Command::Extensions
            | Command::Time(None)
            | Command::Weather(None) => Permission::Player,
            // Anything that affects other people or the world.
            Command::Say(_)
            | Command::Time(Some(_))
            | Command::Weather(Some(_))
            | Command::Teleport { .. }
            // It is a teleport, and it searches: a player who could run
            // it could put themselves anywhere and make the server
            // generate a few thousand columns while they thought about
            // it.
            | Command::BiomeTeleport { .. }
            // It sets fire to things.
            | Command::Lightning { .. }
            | Command::Save
            | Command::Give { .. }
            | Command::Kick { .. }
            | Command::Op { .. }
            | Command::Deop { .. }
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
    "/players              - everyone ever seen, with UUIDs",
    "/where                - your position",
    "/spawn                - teleport to spawn",
    "/stats                - server counters",
    "/mods                 - the plugins and mods that are loaded",
    "/time                 - show the time of day",
    "/time <0..1|day|night|noon|midnight>  - set it (operator)",
    "/weather              - what the sky is doing",
    "/weather <clear|rain|storm|auto>  - set it (operator)",
    "/tp <x> <y> <z>       - teleport (operator)",
    "/biometp <biome>      - teleport to the nearest chunk of it (operator)",
    "/lightning [<x> <z>]  - throw a bolt, here or at a column (operator)",
    "/say <message>        - broadcast (operator)",
    "/save                 - flush world, chests and players to disk (operator)",
    "/give <block> [n]     - put blocks in your pack (operator)",
    "/kick <player> [why]  - disconnect someone (operator)",
    "/op <player>          - make someone an operator (operator)",
    "/deop <player>        - take it back (operator)",
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
        "list" | "who" => Ok(Command::List),
        "players" | "profiles" | "whois" => Ok(Command::Profiles),
        "where" | "pos" => Ok(Command::Where),
        "spawn" => Ok(Command::Spawn),
        "stats" | "tps" => Ok(Command::Stats),
        "mods" | "plugins" | "extensions" => Ok(Command::Extensions),
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

        "weather" => match rest.first() {
            None => Ok(Command::Weather(None)),
            Some(arg) if arg.eq_ignore_ascii_case("auto") => Ok(Command::Weather(Some(None))),
            Some(arg) => match primitive_shared::weather::Weather::parse(arg) {
                Some(weather) => Ok(Command::Weather(Some(Some(weather)))),
                // A typo is a refusal rather than silently a clear sky,
                // which is the same rule `Weather::parse` follows and
                // for the same reason: an operator who mistyped "rian"
                // should be told, not quietly given sunshine.
                None => Err(ParseError::Usage("/weather <clear|rain|storm|auto>")),
            },
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

        // The tail is joined rather than refused, so `/biometp snowy
        // peaks` works as well as `/biometp snowy_peaks`: a biome whose
        // name is two words is one a player will type as two words, and
        // a usage error for a name that is on the list the command
        // itself prints would be a command arguing with its own help.
        "biometp" | "biome" => {
            if rest.is_empty() {
                return Err(ParseError::Usage("/biometp <biome>"));
            }
            Ok(Command::BiomeTeleport {
                biome: rest.join(" "),
            })
        }

        "lightning" | "bolt" => match rest.len() {
            0 => Ok(Command::Lightning { at: None }),
            2 => {
                let coord = |s: &str| {
                    s.parse::<i32>()
                        .map_err(|_| ParseError::BadNumber(s.to_string()))
                };
                Ok(Command::Lightning {
                    at: Some((coord(rest[0])?, coord(rest[1])?)),
                })
            }
            // Two, not three: a bolt picks its own height -- the top of
            // the column -- because that is what lightning does and a
            // `y` would only be a way to ask for one inside a hill.
            _ => Err(ParseError::Usage("/lightning [<x> <z>]")),
        },

        "give" => {
            if rest.is_empty() || rest.len() > 2 {
                return Err(ParseError::Usage("/give <block> [count]"));
            }
            let count = match rest.get(1) {
                None => 1,
                Some(n) => n
                    .parse::<u32>()
                    .map_err(|_| ParseError::BadNumber(n.to_string()))?,
            };
            Ok(Command::Give {
                block: rest[0].to_ascii_lowercase(),
                count,
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

        // Exactly one argument, unlike `kick`, whose tail is a reason:
        // there is no tail here, and `/op alice bob` is far more likely
        // to be someone expecting two promotions than someone naming a
        // player "alice bob" -- so it is refused rather than half done.
        "op" => {
            if rest.len() != 1 {
                return Err(ParseError::Usage("/op <player>"));
            }
            Ok(Command::Op {
                username: rest[0].to_string(),
            })
        }

        "deop" => {
            if rest.len() != 1 {
                return Err(ParseError::Usage("/deop <player>"));
            }
            Ok(Command::Deop {
                username: rest[0].to_string(),
            })
        }

        other => Err(ParseError::Unknown(other.to_string())),
    }
}

/// How far `/biometp` looks, in chunks of sixteen columns.
///
/// **Ninety-six chunks is a kilometre and a half each way**, which is far
/// enough to find anything the generator makes at the scale it makes it
/// -- a desert and a taiga are never more than fifteen hundred blocks
/// apart (`no_walk_leads_from_a_taiga_to_a_desert_inside_a_kilometre`),
/// and the rarest biome there is, a bog, is a third of a percent of the
/// world and so has about forty of them in this square.
///
/// And it is bounded because **an unbounded search is a hung server**:
/// `biome_at` is noise rather than a chunk, so it costs no generation,
/// but a spiral with no end asked for a biome the seed does not contain
/// -- and some seeds contain no snowy peaks at all -- would walk until
/// the coordinates overflowed. The reply says how far it looked, so a
/// refusal reads as "not near here" rather than as "no such place".
pub const BIOME_SEARCH_CHUNKS: i32 = 96;

/// Which chunk of `wanted` is nearest, and how many chunks away it is.
///
/// Rings outward from the caller's own chunk, testing the middle column
/// of each: the biome field is smooth (see `worldgen::Biome`, which is
/// derived from three smooth fields), so a chunk whose middle is a bog
/// is a bog, and sampling four corners would cost four times as much to
/// find the same chunk half a ring sooner.
///
/// Ring by ring rather than a square scan sorted afterwards, because the
/// answer is wanted *nearest first* and the far half of a square is
/// thousands of columns that are only looked at to be thrown away.
///
/// A closure rather than a `&World`, so the whole of the search --
/// including that it stops, and including what it does when the biome is
/// under the caller's feet -- is testable against a field somebody drew
/// by hand.
pub fn nearest_biome(
    from_chunk: (i32, i32),
    wanted: primitive_shared::worldgen::Biome,
    biome_at_chunk: impl Fn(i32, i32) -> primitive_shared::worldgen::Biome,
) -> Option<((i32, i32), i32)> {
    for ring in 0..=BIOME_SEARCH_CHUNKS {
        // The edge of the square at this distance. `ring == 0` is the
        // caller's own chunk, which is the answer to `/biometp forest`
        // typed in a forest -- and it has to be, or the command would
        // march somebody out of the place they were asking about.
        for dz in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs() != ring && dz.abs() != ring {
                    continue;
                }
                let at = (from_chunk.0 + dx, from_chunk.1 + dz);
                if biome_at_chunk(at.0, at.1) == wanted {
                    return Some((at, ring));
                }
            }
        }
    }
    None
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
    /// Set the weather, or -- for `None` -- hand it back to the
    /// countdown. See `Command::Weather`.
    SetWeather(Option<primitive_shared::weather::Weather>),
    TeleportSelf { x: f32, y: f32, z: f32 },
    TeleportSelfToSpawn,
    /// Find the nearest chunk of this biome and stand the caller in it.
    /// The name is still unparsed here: see `Command::BiomeTeleport`.
    TeleportSelfToBiome { biome: String },
    /// Throw a bolt at a column, or near the caller. See
    /// `Command::Lightning`.
    Lightning { at: Option<(i32, i32)> },
    Kick { username: String, reason: String },
    /// Grant (`operator`) or revoke operator rights for a named player.
    ///
    /// One variant with a flag rather than two, because everything the
    /// caller then has to do -- find the profile, refuse an unknown
    /// name, notice it was already so, tell both parties -- is the same
    /// work in both directions, and splitting it would only duplicate
    /// that.
    SetOperator { username: String, operator: bool },
    /// Put `count` of a block, named as `types::block_name` names it,
    /// into the caller's pack.
    Give { block: String, count: u32 },
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
        Command::Profiles => Response::Reply(vec!["__PROFILES__".to_string()]),
        Command::Stats => Response::Reply(vec!["__STATS__".to_string()]),
        Command::Extensions => Response::Reply(vec!["__EXTENSIONS__".to_string()]),
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
        Command::BiomeTeleport { biome } => {
            if caller.is_none() {
                Response::Reply(vec!["the console can't teleport".to_string()])
            } else {
                Response::TeleportSelfToBiome { biome }
            }
        }
        // The console may throw one at a named column; it may not throw
        // one "here", because it is not standing anywhere.
        Command::Lightning { at } => match (at, caller) {
            (None, None) => Response::Reply(vec!["the console is not standing anywhere: /lightning <x> <z>".to_string()]),
            (at, _) => Response::Lightning { at },
        },
        Command::Give { block, count } => {
            if caller.is_none() {
                // The console has no pack to put anything in.
                Response::Reply(vec!["the console cannot carry anything".to_string()])
            } else {
                Response::Give { block, count }
            }
        }
        Command::Say(text) => Response::Broadcast(text),
        Command::Time(None) => Response::Reply(vec!["__TIME__".to_string()]),
        Command::Time(Some(t)) => Response::SetTime(t),
        Command::Weather(None) => Response::Reply(vec!["__WEATHER__".to_string()]),
        Command::Weather(Some(weather)) => Response::SetWeather(weather),
        Command::Save => Response::Save,
        Command::Kick { username, reason } => Response::Kick { username, reason },
        Command::Op { username } => Response::SetOperator {
            username,
            operator: true,
        },
        Command::Deop { username } => Response::SetOperator {
            username,
            operator: false,
        },
        Command::Stop => Response::Stop,
    }
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Help => "help",
        Command::List => "list",
        Command::Profiles => "players",
        Command::Say(_) => "say",
        Command::Time(_) => "time",
        Command::Weather(_) => "weather",
        Command::Where => "where",
        Command::Teleport { .. } => "tp",
        Command::Spawn => "spawn",
        Command::BiomeTeleport { .. } => "biometp",
        Command::Lightning { .. } => "lightning",
        Command::Stats => "stats",
        Command::Extensions => "mods",
        Command::Save => "save",
        Command::Give { .. } => "give",
        Command::Kick { .. } => "kick",
        Command::Op { .. } => "op",
        Command::Deop { .. } => "deop",
        Command::Stop => "stop",
    }
}

#[cfg(test)]
mod tests {
    /// **The rule the give menu is drawn from.** `ui::journal` shows the
    /// page to an operator and to nobody else, and it learns which it is
    /// facing by asking the server, which answers with this. A rule that
    /// said yes where `authorize` says no would be a menu that opens and
    /// then refuses -- which is what it did before, and what the player
    /// asked to be rid of.
    #[test]
    fn your_own_world_makes_you_an_operator_and_a_server_that_never_heard_of_you_does_not() {
        use super::{permission_for, Permission};
        assert_eq!(permission_for(true, None), Permission::Operator, "the only player of a local world is not its operator");
        assert_eq!(permission_for(true, Some(false)), Permission::Operator, "a local world outranks the profile file");
        assert_eq!(permission_for(false, Some(true)), Permission::Operator);
        assert_eq!(permission_for(false, Some(false)), Permission::Player);
        assert_eq!(permission_for(false, None), Permission::Player, "a connection with no profile was granted authority");
    }

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
    fn give_takes_a_block_and_an_optional_count() {
        assert_eq!(
            parse("/give chest"),
            Ok(Command::Give {
                block: "chest".to_string(),
                count: 1,
            })
        );
        assert_eq!(
            parse("/give COBBLESTONE 40"),
            Ok(Command::Give {
                block: "cobblestone".to_string(),
                count: 40,
            })
        );
        assert!(matches!(parse("/give"), Err(ParseError::Usage(_))));
        assert!(matches!(parse("/give chest lots"), Err(ParseError::BadNumber(_))));
    }

    #[test]
    fn give_is_operator_only_and_needs_somewhere_to_put_it() {
        // The one command that makes something out of nothing, so it is
        // the one a player must not be able to run -- and the console,
        // which is nobody, has no pack for it to go into.
        let command = Command::Give {
            block: "chest".to_string(),
            count: 1,
        };
        assert_eq!(command.required_permission(), Permission::Operator);
        assert!(matches!(
            authorize(command.clone(), Permission::Player, Some(1)),
            Response::Denied(_)
        ));
        assert!(matches!(
            authorize(command, Permission::Operator, None),
            Response::Reply(_)
        ));
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
    fn op_and_deop_take_exactly_one_name() {
        assert_eq!(
            parse("/op alice"),
            Ok(Command::Op {
                username: "alice".to_string()
            })
        );
        assert_eq!(
            parse("/deop alice"),
            Ok(Command::Deop {
                username: "alice".to_string()
            })
        );
        // The name is kept as typed: case is not identity here (see
        // `profiles::Uuid::of_name`), but the reply should say the name
        // back the way a person would recognise it.
        assert_eq!(
            parse("/op Alice"),
            Ok(Command::Op {
                username: "Alice".to_string()
            })
        );
        for line in ["/op", "/deop", "/op alice bob", "/deop alice bob"] {
            assert!(
                matches!(parse(line), Err(ParseError::Usage(_))),
                "{line} should have been refused with a usage line"
            );
        }
    }

    #[test]
    fn a_player_cannot_make_themselves_an_operator() {
        // The whole point of the permission check: if this were allowed,
        // every other operator-only command would be too.
        for line in ["/op alice", "/deop alice"] {
            let command = parse(line).unwrap();
            assert_eq!(command.required_permission(), Permission::Operator);
            assert!(
                matches!(
                    authorize(command, Permission::Player, Some(1)),
                    Response::Denied(_)
                ),
                "{line} must be refused for a plain player"
            );
        }
    }

    #[test]
    fn an_operator_op_becomes_a_grant_and_a_deop_a_revocation() {
        assert_eq!(
            authorize(parse("/op alice").unwrap(), Permission::Operator, None),
            Response::SetOperator {
                username: "alice".to_string(),
                operator: true,
            }
        );
        assert_eq!(
            authorize(parse("/deop alice").unwrap(), Permission::Operator, Some(2)),
            Response::SetOperator {
                username: "alice".to_string(),
                operator: false,
            }
        );
    }

    #[test]
    fn players_cannot_run_operator_commands() {
        for line in [
            "/stop",
            "/kick alice",
            "/say hi",
            "/time noon",
            "/tp 0 0 0",
            "/op alice",
            "/deop alice",
        ] {
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
            "/help", "/list", "/where", "/spawn", "/stats", "/time", "/tp", "/biometp", "/lightning",
            "/say", "/save", "/kick", "/op", "/deop", "/stop",
        ] {
            assert!(documented.contains(name), "{name} is missing from /help");
        }
    }

    #[test]
    fn a_biome_is_named_the_way_the_game_names_it_and_a_two_word_name_survives_a_command_line() {
        use primitive_shared::worldgen::Biome;
        assert_eq!(
            parse("/biometp steppe"),
            Ok(Command::BiomeTeleport { biome: "steppe".to_string() })
        );
        // Both spellings of a two-word name reach the same biome.
        for line in ["/biometp snowy peaks", "/biometp snowy_peaks", "/biometp SNOWY_PEAKS"] {
            let Ok(Command::BiomeTeleport { biome }) = parse(line) else {
                panic!("{line} did not parse");
            };
            assert_eq!(Biome::parse(&biome), Some(Biome::SnowyPeaks), "{line}");
        }
        // Every biome the game has can be asked for by the name it
        // prints, which is what makes the list in the refusal usable.
        for biome in Biome::ALL {
            assert_eq!(Biome::parse(biome.name()), Some(*biome));
        }
        assert_eq!(Biome::parse("mordor"), None);
        // ...and a bare `/biometp` is a usage line, not a walk to
        // wherever chunk zero happens to be.
        assert_eq!(parse("/biometp"), Err(ParseError::Usage("/biometp <biome>")));
    }

    #[test]
    fn biometp_is_an_operators_command_and_the_console_cannot_stand_anywhere() {
        let asked = parse("/biometp bog").unwrap();
        assert!(matches!(
            authorize(asked.clone(), Permission::Player, Some(1)),
            Response::Denied(_)
        ));
        assert_eq!(
            authorize(asked.clone(), Permission::Operator, Some(1)),
            Response::TeleportSelfToBiome { biome: "bog".to_string() }
        );
        assert!(matches!(
            authorize(asked, Permission::Operator, None),
            Response::Reply(_)
        ));
    }

    #[test]
    fn a_bolt_can_be_asked_for_by_column_or_for_wherever_the_storm_would_put_one() {
        assert_eq!(parse("/lightning"), Ok(Command::Lightning { at: None }));
        assert_eq!(parse("/lightning 12 -30"), Ok(Command::Lightning { at: Some((12, -30)) }));
        // A height is refused rather than half obeyed: a bolt picks its
        // own, and `/lightning 1 2 3` is somebody expecting `/tp`.
        assert_eq!(parse("/lightning 1 2 3"), Err(ParseError::Usage("/lightning [<x> <z>]")));
        assert_eq!(parse("/lightning x z"), Err(ParseError::BadNumber("x".to_string())));
        // The console is not standing anywhere, so it must say where.
        assert!(matches!(
            authorize(Command::Lightning { at: None }, Permission::Operator, None),
            Response::Reply(_)
        ));
        assert_eq!(
            authorize(Command::Lightning { at: Some((0, 0)) }, Permission::Operator, None),
            Response::Lightning { at: Some((0, 0)) }
        );
        assert!(matches!(
            authorize(Command::Lightning { at: None }, Permission::Player, Some(1)),
            Response::Denied(_)
        ));
    }

    #[test]
    fn the_search_finds_the_nearest_chunk_of_it_and_the_one_underfoot_first() {
        use primitive_shared::worldgen::Biome;
        // A world that is meadow everywhere except one column of bog and
        // one, nearer, of steppe.
        let field = |x: i32, z: i32| match (x, z) {
            (10, 0) => Biome::Bog,
            (-2, 1) => Biome::Steppe,
            _ => Biome::Plains,
        };
        assert_eq!(nearest_biome((0, 0), Biome::Bog, field), Some(((10, 0), 10)));
        assert_eq!(nearest_biome((0, 0), Biome::Steppe, field), Some(((-2, 1), 2)));
        // Standing in it already is a distance of nothing, not a march
        // to the next one.
        assert_eq!(nearest_biome((10, 0), Biome::Bog, field), Some(((10, 0), 0)));
    }

    #[test]
    fn the_search_gives_up_rather_than_walking_out_of_the_world() {
        use primitive_shared::worldgen::Biome;
        use std::cell::Cell as CountCell;
        // A seed with no snowy peaks in it at all -- which is a seed
        // that exists. An unbounded spiral here is a hung server.
        let looked = CountCell::new(0);
        let counted = |_x: i32, _z: i32| {
            looked.set(looked.get() + 1);
            Biome::Ocean
        };
        assert_eq!(nearest_biome((0, 0), Biome::SnowyPeaks, counted), None);
        let side = (BIOME_SEARCH_CHUNKS * 2 + 1) as i64;
        assert_eq!(looked.get() as i64, side * side, "the rings did not cover the square exactly once");
    }
}

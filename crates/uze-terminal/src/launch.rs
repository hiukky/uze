//! The vocabulary of variables a launch stamps into a pane.
//!
//! Two parties write into a pane's environment on purpose: the server, when
//! it spawns the pane (`UZE_PANE`, and whatever the client asked the launch
//! to carry), and uze's PATH shim, when it `exec`s into a harness
//! (`UZE_SHIM_NAME`, `UZE_SHIM_PID`). Every descendant of those processes
//! inherits the lot, and the server itself is routinely one of them — it
//! is started by whatever `uze` first needed it, which in this project is
//! usually a `uze` run from inside a shimmed agent. So the same list serves
//! two ends: it names what a launch may carry, and what a pane must never
//! inherit from the process that spawned it.
//!
//! The server interprets none of these values. The agent identity is a
//! string the client chose and the shim reads back; what it means is the
//! client's and the shim's business, which is why the name lives here, in
//! transport, and not with either of them.

/// The identifier of the agent a pane was launched for, stamped by the
/// client on the tab's first process and read back by uze's shim and by
/// the agent's own commands.
pub const AGENT_IDENTITY_VARIABLE: &str = "UZE_AGENT";

/// The pane a process runs in, stamped by the server on every pane it
/// spawns so a `uze` started inside one opens a space here instead of a
/// client within a client.
pub const PANE_VARIABLE: &str = "UZE_PANE";

/// The alias uze's shim launched a process under.
pub const SHIM_NAME_VARIABLE: &str = "UZE_SHIM_NAME";

/// The pid the shim's stamp was made for — the shim `exec`s, so it is the
/// process that carries the stamp for the rest of its life.
pub const SHIM_PID_VARIABLE: &str = "UZE_SHIM_PID";

/// What a launch stamps and a pane never inherits: removed from the
/// environment every spawned pane is seeded with, so a pane carries only
/// what its own launch put there.
pub const STAMPED_VARIABLES: [&str; 3] = [
    SHIM_NAME_VARIABLE,
    SHIM_PID_VARIABLE,
    AGENT_IDENTITY_VARIABLE,
];

/// The environment a launch carries: applied to the tab's first process,
/// persisted with the tab, and reported back on it.
pub type Environment = Vec<(String, String)>;

/// A launch environment is a few identifiers, and the persisted workspace
/// file is what a bound protects: one line per tab, never a dump of
/// somebody's shell.
pub const MAX_ENVIRONMENT_ENTRIES: usize = 16;
pub const MAX_ENVIRONMENT_BYTES: usize = 4096;

/// Why a launch environment was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnvironmentRefusal {
    /// An environment belongs to a command; a shell carries none.
    WithoutCommand,
    TooManyEntries {
        entries: usize,
    },
    TooLarge {
        bytes: usize,
    },
    InvalidName {
        name: String,
    },
}

impl std::fmt::Display for EnvironmentRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WithoutCommand => {
                write!(f, "a launch environment needs a command to launch")
            }
            Self::TooManyEntries { entries } => write!(
                f,
                "a launch environment carries at most {MAX_ENVIRONMENT_ENTRIES} variables, not {entries}"
            ),
            Self::TooLarge { bytes } => write!(
                f,
                "a launch environment carries at most {MAX_ENVIRONMENT_BYTES} bytes, not {bytes}"
            ),
            Self::InvalidName { name } => {
                write!(f, "{name:?} is not a variable name")
            }
        }
    }
}

/// What a pane's first process is launched as.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) enum Launch {
    /// The user's default shell, carrying nothing beyond the pane's own
    /// environment.
    Shell,
    /// A program, and what its process starts with beyond the pane's own
    /// environment.
    Program { argv: Vec<String>, env: Environment },
}

impl Launch {
    pub(crate) fn argv(&self) -> &[String] {
        match self {
            Self::Shell => &[],
            Self::Program { argv, .. } => argv,
        }
    }

    pub(crate) fn env(&self) -> &[(String, String)] {
        match self {
            Self::Shell => &[],
            Self::Program { env, .. } => env,
        }
    }
}

/// The launch a `CreateTab` asks for, or why `environment` may not
/// accompany `command`. No command, or an empty one, is the shell.
pub(crate) fn validate(
    command: Option<Vec<String>>,
    environment: Environment,
) -> Result<Launch, EnvironmentRefusal> {
    let Some(argv) = command.filter(|argv| !argv.is_empty()) else {
        return if environment.is_empty() {
            Ok(Launch::Shell)
        } else {
            Err(EnvironmentRefusal::WithoutCommand)
        };
    };
    if environment.len() > MAX_ENVIRONMENT_ENTRIES {
        return Err(EnvironmentRefusal::TooManyEntries {
            entries: environment.len(),
        });
    }
    let bytes = environment
        .iter()
        .map(|(name, value)| name.len() + value.len())
        .sum::<usize>();
    if bytes > MAX_ENVIRONMENT_BYTES {
        return Err(EnvironmentRefusal::TooLarge { bytes });
    }
    if let Some((name, _)) = environment
        .iter()
        .find(|(name, _)| name.is_empty() || name.contains('=') || name.contains('\0'))
    {
        return Err(EnvironmentRefusal::InvalidName { name: name.clone() });
    }
    Ok(Launch::Program {
        argv,
        env: environment,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> Vec<String> {
        vec!["claude".to_owned()]
    }

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    #[test]
    fn no_command_is_the_shell() {
        assert_eq!(validate(None, Vec::new()), Ok(Launch::Shell));
        assert_eq!(validate(Some(Vec::new()), Vec::new()), Ok(Launch::Shell));
        assert_eq!(
            validate(Some(command()), Vec::new()),
            Ok(Launch::Program {
                argv: command(),
                env: Vec::new()
            })
        );
    }

    #[test]
    fn a_shell_never_carries_a_launch_environment() {
        let stamp = vec![pair(AGENT_IDENTITY_VARIABLE, "abc")];
        assert_eq!(
            validate(None, stamp.clone()),
            Err(EnvironmentRefusal::WithoutCommand)
        );
        assert_eq!(
            validate(Some(Vec::new()), stamp.clone()),
            Err(EnvironmentRefusal::WithoutCommand)
        );
        assert_eq!(
            validate(Some(command()), stamp.clone()),
            Ok(Launch::Program {
                argv: command(),
                env: stamp
            })
        );
    }

    #[test]
    fn the_bounds_are_on_entries_and_on_bytes() {
        let many: Vec<_> = (0..=MAX_ENVIRONMENT_ENTRIES)
            .map(|index| pair(&format!("V{index}"), "x"))
            .collect();
        assert!(matches!(
            validate(Some(command()), many),
            Err(EnvironmentRefusal::TooManyEntries { .. })
        ));
        let large = vec![pair("V", &"x".repeat(MAX_ENVIRONMENT_BYTES))];
        assert!(matches!(
            validate(Some(command()), large),
            Err(EnvironmentRefusal::TooLarge { .. })
        ));
    }

    #[test]
    fn a_name_is_non_empty_and_carries_no_separator() {
        for name in ["", "A=B", "A\0"] {
            assert!(matches!(
                validate(Some(command()), vec![pair(name, "x")]),
                Err(EnvironmentRefusal::InvalidName { .. })
            ));
        }
    }

    #[test]
    fn the_agent_identity_is_among_what_a_pane_never_inherits() {
        assert!(STAMPED_VARIABLES.contains(&AGENT_IDENTITY_VARIABLE));
        assert!(STAMPED_VARIABLES.contains(&SHIM_PID_VARIABLE));
        assert!(!STAMPED_VARIABLES.contains(&PANE_VARIABLE));
    }
}

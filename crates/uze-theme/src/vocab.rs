//! One declaration site per vocabulary.
//!
//! A token or a symbol has to exist four times over — as a Rust variant, as
//! the dotted name a theme file uses, as an entry in the exhaustive list the
//! resolver walks, and on both sides of serde. Writing those out by hand is
//! how a vocabulary acquires an entry that serializes under a name nothing
//! reads. This macro makes the name and the variant one line, and the list
//! impossible to forget.

macro_rules! vocabulary {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[doc = $doc:literal])*
                $variant:ident = $wire:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
        $vis enum $name {
            $(
                $(#[doc = $doc])*
                $variant,
            )+
        }

        impl $name {
            /// Every member, in declaration order. The resolver walks this,
            /// so a variant that is never named here cannot exist.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// This member's position in [`ALL`](Self::ALL) — the index a
            /// resolved theme stores its value at, so a lookup is an array
            /// read rather than a map probe on the draw path.
            pub const fn index(self) -> usize {
                self as usize
            }

            /// The name this member is written as in a theme file.
            pub const fn name(self) -> &'static str {
                match self {
                    $($name::$variant => $wire),+
                }
            }

            /// The member a theme file named, or `None` if the file names
            /// something this build does not know about.
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($wire => Some($name::$variant),)+
                    _ => None,
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.name())
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.name())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let name = String::deserialize(deserializer)?;
                $name::from_name(&name).ok_or_else(|| {
                    serde::de::Error::custom(format!(
                        concat!("unknown ", stringify!($name), " name: {}"),
                        name
                    ))
                })
            }
        }
    };
}

pub(crate) use vocabulary;

#[cfg(test)]
mod tests {
    use crate::{Symbol, Token};

    #[test]
    fn every_member_round_trips_through_its_written_name() {
        for token in Token::ALL {
            assert_eq!(Token::from_name(token.name()), Some(*token));
        }
        for symbol in Symbol::ALL {
            assert_eq!(Symbol::from_name(symbol.name()), Some(*symbol));
        }
    }

    #[test]
    fn no_two_members_share_a_written_name() {
        let mut names: Vec<&str> = Token::ALL.iter().map(|token| token.name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "two tokens serialize under one name");

        let mut names: Vec<&str> = Symbol::ALL.iter().map(|symbol| symbol.name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "two symbols serialize under one name");
    }

    #[test]
    fn an_unknown_name_is_reported_rather_than_guessed() {
        assert_eq!(Token::from_name("text.chartreuse"), None);
        assert_eq!(Symbol::from_name("mark.nonexistent"), None);
    }
}

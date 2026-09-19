//! How a record written by another build is read by this one.
//!
//! Every UZE on a machine meets state a previous UZE left there, and what
//! it does about it used to be each module's own decision: one refused, one
//! set the bytes aside, one discarded them without a word, one warned and
//! read on anyway. Four answers to one question, and nine documents with no
//! version at all — so a release that moved three shapes at once cost an
//! operator their spaces, even though the step that mattered was *dropping
//! one field*.
//!
//! The rule this module holds is:
//!
//! - **Carrying a record across is the first answer, and it is silent.**
//!   When this build knows the shape a record was written in, it climbs the
//!   [`Ladder`] to the shape written now. Nothing is reported: carrying a
//!   record across is the product working, not an event.
//! - **Setting it aside is the floor, not the policy.** Only a record whose
//!   shape cannot be climbed at all is moved out of the way — never
//!   deleted — so the caller can reconstruct what the world still knows and
//!   report only the residue.
//! - **Recovery has a direction.** A shape this build is *ahead* of is this
//!   build's to climb or set aside. A shape *ahead* of this build is left
//!   exactly as it is: two builds on one machine is the daily state of this
//!   repository, and a rule without a direction has them taking turns
//!   destroying each other's records, each saying it had recovered.
//! - **The shape is read before the record.** Every field of a record is
//!   required, so a document written under an older shape fails `serde` —
//!   `missing field ...` — long before a guard that compares versions could
//!   look at it. Reading [`Declared`] first is what keeps the guard alive
//!   for the one case it exists for.
//! - **A record with no shape at all is shape 1.** That is what a document
//!   written before its kind declared one *is*, and reading it any other
//!   way sets aside every machine's state on the release that adds the
//!   field.
//! - **The judgement happens under the lock that guards the record.** A
//!   reader without it can catch a publication halfway and would set aside
//!   a record that was never broken, so a read-only surface reports what it
//!   saw and moves on; the next mutation heals.
//!
//! A ladder step is a function from one shape to the next, and it is
//! removable once no machine can still be below it. That is what makes this
//! the opposite of the scattered compatibility this project refuses: the
//! current struct stays clean *because* the old shapes live here rather
//! than as optional fields nobody ever deletes.
//!
//! Only records carry a shape. What UZE generated for another program to
//! read, and what UZE remembered from observing, are produced or observed
//! again when they cannot be read — there is nothing in them to carry.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, de::DeserializeOwned};

/// What reading a record can fail with.
///
/// Its own type rather than the domain's, because this crate is a leaf: the
/// terminal runtime holds the workspace and depends on nothing of UZE's,
/// and a durability rule written in two places is the failure this crate
/// exists to end. Consumers map these into their own error vocabulary.
#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not a record this build can read: {source}")]
    Unreadable {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{path} was written in shape {found}; this build writes {expected}")]
    UnsupportedShape {
        path: PathBuf,
        found: u32,
        expected: u32,
    },
}

pub type Result<T> = std::result::Result<T, DocumentError>;

/// The shape a record with no version field declares by having none.
pub const FIRST_SHAPE: u32 = 1;

/// The one field every shape of every record shares, read before the record
/// itself.
#[derive(Deserialize)]
struct Declared {
    #[serde(default = "first_shape")]
    schema_version: u32,
}

fn first_shape() -> u32 {
    FIRST_SHAPE
}

/// One rung: what a version change meant, written where the next record can
/// reach it rather than in one module's own prose.
pub struct Step {
    /// The shape this step reads.
    pub from: u32,
    /// The shape it leaves behind, always `from + 1`.
    pub to: u32,
    /// The change itself, over the record's own document rather than over a
    /// struct — the struct for `from` no longer exists, which is the point.
    pub climb: fn(serde_json::Value) -> Result<serde_json::Value>,
}

/// Every step this record has, oldest first and contiguous.
pub type Ladder = &'static [Step];

/// A record: something only UZE's own record knows, which therefore has to
/// survive an upgrade rather than be observed again.
pub trait Shaped: DeserializeOwned {
    /// The shape this build writes.
    const SHAPE: u32;

    /// What this record is, for a message an operator reads.
    const KIND: &'static str;

    /// The steps from older shapes, oldest first. A record that has never
    /// changed shape has none.
    fn ladder() -> Ladder {
        &[]
    }
}

/// What reading a record had to do before it could answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Carried<T> {
    /// Nothing was recorded there.
    Absent,
    /// The record was already at the shape this build writes.
    Current(T),
    /// The record was climbed from an older shape. A caller holding the
    /// record's lock owes it a rewrite, so the next reader finds it
    /// current; a read-only caller simply uses it.
    Climbed { record: T, from: u32 },
}

impl<T> Carried<T> {
    /// The record, however it was reached.
    pub fn record(self) -> Option<T> {
        match self {
            Self::Absent => None,
            Self::Current(record) | Self::Climbed { record, .. } => Some(record),
        }
    }

    /// The record, or its default when nothing was recorded.
    pub fn or_default(self) -> T
    where
        T: Default,
    {
        self.record().unwrap_or_default()
    }

    /// Whether the caller owes the file a rewrite at the current shape.
    pub fn climbed(&self) -> bool {
        matches!(self, Self::Climbed { .. })
    }
}

/// Reads the record at `path`, climbing it to the current shape.
///
/// Answers [`Carried::Absent`] when nothing is there — a first run is not a
/// failure. Reports [`UzeError::UnsupportedStateSchema`] for a shape this
/// build cannot reach: ahead of it, or behind it with a rung missing. The
/// caller decides what that costs, because only the caller knows whether
/// the world can still answer for what the record held.
pub fn read<T: Shaped>(path: &Path) -> Result<Carried<T>> {
    let Some(bytes) = read_bytes(path)? else {
        return Ok(Carried::Absent);
    };
    let declared: Declared =
        serde_json::from_slice(&bytes).map_err(|source| DocumentError::Unreadable {
            path: path.to_path_buf(),
            source,
        })?;
    let found = declared.schema_version;
    if found == T::SHAPE {
        return Ok(Carried::Current(parse(path, &bytes)?));
    }
    if found > T::SHAPE {
        // Never this build's to take. The operation that needed it is
        // refused, and the bytes stay exactly as the newer build left them.
        return Err(DocumentError::UnsupportedShape {
            path: path.to_path_buf(),
            found,
            expected: T::SHAPE,
        });
    }
    let document = serde_json::from_slice(&bytes).map_err(|source| DocumentError::Unreadable {
        path: path.to_path_buf(),
        source,
    })?;
    let climbed = climb::<T>(path, document, found)?;
    Ok(Carried::Climbed {
        record: serde_json::from_value(climbed).map_err(|source| DocumentError::Unreadable {
            path: path.to_path_buf(),
            source,
        })?,
        from: found,
    })
}

/// Walks the ladder from `found` to the current shape, one rung at a time,
/// so a record several releases behind arrives where it would have arrived
/// had the operator upgraded one release at a time.
fn climb<T: Shaped>(
    path: &Path,
    mut document: serde_json::Value,
    found: u32,
) -> Result<serde_json::Value> {
    let mut shape = found;
    while shape < T::SHAPE {
        let Some(step) = T::ladder().iter().find(|step| step.from == shape) else {
            // A shape older than the ladder reaches. Behind this build, so
            // the direction rule allows setting it aside; what that costs is
            // the caller's to decide and to say.
            return Err(DocumentError::UnsupportedShape {
                path: path.to_path_buf(),
                found,
                expected: T::SHAPE,
            });
        };
        document = (step.climb)(document)?;
        shape = step.to;
    }
    if let Some(object) = document.as_object_mut() {
        object.insert(
            "schema_version".to_owned(),
            serde_json::Value::from(T::SHAPE),
        );
    }
    Ok(document)
}

fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(DocumentError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn parse<T: DeserializeOwned>(path: &Path, bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|source| DocumentError::Unreadable {
        path: path.to_path_buf(),
        source,
    })
}

/// A record UZE could not read, kept rather than overwritten.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetAside {
    /// Where the bytes are now. Nothing reads them again; they are kept
    /// because a record UZE cannot understand is still not one it may throw
    /// away. The name deliberately stops being a `.json`: what is set aside
    /// must not read as a second record to anything listing the directory.
    pub path: PathBuf,
    pub reason: String,
}

/// Whether a record this build cannot read is one it may set aside.
///
/// Bytes that are not the record at all, and a shape this build is *ahead*
/// of: what is lost there is bookkeeping this build would rewrite anyway.
///
/// Never a shape ahead of this one. Setting that aside takes a newer UZE's
/// record away from it, and two builds on one machine — the ordinary state
/// of this repository, `target/debug/uze` beside `~/.cargo/bin/uze` — would
/// then take turns destroying each other's records, one recovery at a time.
/// The older build reports and leaves it where it is.
pub fn may_be_set_aside(reason: &DocumentError) -> bool {
    match reason {
        DocumentError::Unreadable { .. } => true,
        DocumentError::UnsupportedShape {
            found, expected, ..
        } => found < expected,
        DocumentError::Read { .. } | DocumentError::Write { .. } => false,
    }
}

/// Moves the record aside so it can be recorded again, and says what was
/// moved.
///
/// Only ever reached under the lock that guards the record: with it held no
/// other pass is publishing the file, so bytes that do not read are
/// genuinely unreadable rather than a write caught halfway.
pub fn set_aside(path: &Path, kind: &str, reason: &DocumentError) -> Result<SetAside> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(kind);
    let moved = path.with_file_name(format!("{name}.unreadable-{}", now_unix()));
    fs::rename(path, &moved).map_err(|source| DocumentError::Write {
        path: moved.clone(),
        source,
    })?;
    tracing::warn!(
        document = %path.display(),
        set_aside = %moved.display(),
        kind,
        reason = %reason,
        "a record could not be read and was set aside"
    );
    Ok(SetAside {
        path: moved,
        reason: reason.to_string(),
    })
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
    struct Sample {
        schema_version: u32,
        label: String,
        /// Added by shape 3.
        #[serde(default)]
        target: String,
    }

    impl Shaped for Sample {
        const SHAPE: u32 = 3;
        const KIND: &'static str = "sample";

        fn ladder() -> Ladder {
            &[
                Step {
                    from: 1,
                    to: 2,
                    // Shape 1 carried a `kind` nothing reads any more.
                    climb: |mut document| {
                        if let Some(object) = document.as_object_mut() {
                            object.remove("kind");
                        }
                        Ok(document)
                    },
                },
                Step {
                    from: 2,
                    to: 3,
                    climb: |mut document| {
                        if let Some(object) = document.as_object_mut() {
                            object.insert("target".to_owned(), "main".into());
                        }
                        Ok(document)
                    },
                },
            ]
        }
    }

    fn write(directory: &Path, body: &str) -> PathBuf {
        let path = directory.join("sample.json");
        fs::write(&path, body).unwrap();
        path
    }

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("uze-document-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn nothing_recorded_is_not_a_failure() {
        let directory = scratch("absent");
        let carried: Carried<Sample> = read(&directory.join("sample.json")).unwrap();
        assert_eq!(carried, Carried::Absent);
    }

    #[test]
    fn the_current_shape_is_read_as_it_stands() {
        let directory = scratch("current");
        let path = write(
            &directory,
            r#"{"schema_version": 3, "label": "a", "target": "main"}"#,
        );
        let carried: Carried<Sample> = read(&path).unwrap();
        assert!(!carried.climbed());
        assert_eq!(carried.record().unwrap().label, "a");
    }

    #[test]
    fn an_older_shape_climbs_every_rung_in_order() {
        let directory = scratch("climb");
        let path = write(
            &directory,
            r#"{"schema_version": 1, "label": "a", "kind": "isolated"}"#,
        );
        let carried: Carried<Sample> = read(&path).unwrap();
        assert!(carried.climbed(), "a record behind this build is carried");
        let record = carried.record().unwrap();
        assert_eq!(record.label, "a", "what the shapes share survives");
        assert_eq!(record.target, "main", "what a later shape added is filled");
        assert_eq!(record.schema_version, Sample::SHAPE);
    }

    #[test]
    fn a_record_with_no_version_at_all_is_the_first_shape() {
        let directory = scratch("unversioned");
        let path = write(&directory, r#"{"label": "a", "kind": "isolated"}"#);
        let carried: Carried<Sample> = read(&path).unwrap();
        assert!(
            carried.climbed(),
            "a document written before the field existed is shape 1, not a failure"
        );
        assert_eq!(carried.record().unwrap().target, "main");
    }

    #[test]
    fn a_newer_shape_is_refused_and_left_alone() {
        let directory = scratch("newer");
        let path = write(&directory, r#"{"schema_version": 99, "label": "a"}"#);
        let error = read::<Sample>(&path).unwrap_err();
        assert!(
            !may_be_set_aside(&error),
            "a newer build's record is never this build's to move"
        );
        assert!(
            path.exists(),
            "and the bytes stay where the newer build put them"
        );
    }

    #[test]
    fn a_shape_older_than_the_ladder_reaches_may_be_set_aside() {
        #[derive(Debug, Deserialize)]
        struct Narrow {
            #[allow(dead_code)]
            schema_version: u32,
        }
        impl Shaped for Narrow {
            const SHAPE: u32 = 5;
            const KIND: &'static str = "narrow";
        }
        let directory = scratch("no-rung");
        let path = write(&directory, r#"{"schema_version": 2}"#);
        let error = read::<Narrow>(&path).unwrap_err();
        assert!(
            may_be_set_aside(&error),
            "behind this build, so the direction rule allows recovery"
        );
    }

    #[test]
    fn bytes_that_are_not_a_record_may_be_set_aside_and_are_kept() {
        let directory = scratch("garbage");
        let path = write(&directory, "not json at all");
        let error = read::<Sample>(&path).unwrap_err();
        assert!(may_be_set_aside(&error));
        let moved = set_aside(&path, Sample::KIND, &error).unwrap();
        assert!(!path.exists(), "the record is out of the way");
        assert!(moved.path.exists(), "and its bytes are kept, never deleted");
        assert!(
            !moved.path.to_string_lossy().ends_with(".json"),
            "under a name nothing reads as a record"
        );
    }
}

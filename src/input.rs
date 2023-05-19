use crate::err::SpecError;
use crate::sys::fd_t;
use libc::c_int;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::string::String;
use std::vec::Vec;

//------------------------------------------------------------------------------
// Serde helpers
//------------------------------------------------------------------------------

/// Deserializer that accepts either a single map or a sequence of items.  In
/// the former case, the map is wrapped into a single-element sequence.

fn one_or_many<'de, T, D>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    // The `PhantomData` is to keep the compiler from complaining about T being
    // an unused generic type parameter.  We need T in order to know the Value
    // type for the Visitor impl.
    struct OneOrMany<T>(std::marker::PhantomData<fn() -> T>);

    impl<'de, T> serde::de::Visitor<'de> for OneOrMany<T>
    where
        T: Deserialize<'de>,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a map or seq of maps")
        }

        fn visit_seq<S>(self, mut seq: S) -> std::result::Result<Vec<T>, S::Error>
        where
            S: serde::de::SeqAccess<'de>,
        {
            let mut res = Vec::new();
            while let Some(e) = seq.next_element()? {
                res.push(e);
            }
            Ok(res)
        }

        fn visit_map<M>(self, map: M) -> std::result::Result<Vec<T>, M::Error>
        where
            M: serde::de::MapAccess<'de>,
        {
            // `MapAccessDeserializer` is a wrapper that turns a `MapAccess`
            // into a `Deserializer`, allowing it to be used as the input to T's
            // `Deserialize` implementation. T then deserializes itself using
            // the entries from the map visitor.
            let res = Deserialize::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
            Ok(vec![res])
        }
    }

    deserializer.deserialize_any(OneOrMany(std::marker::PhantomData))
}

//------------------------------------------------------------------------------
// Spec error
//------------------------------------------------------------------------------

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::error::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            Error::Io(ref err) => err.fmt(f),
            Error::Json(ref err) => err.fmt(f),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        Error::Io(err)
    }
}

impl From<serde_json::error::Error> for Error {
    fn from(err: serde_json::error::Error) -> Error {
        Error::Json(err)
    }
}

type Result<T> = std::result::Result<T, Error>;

//------------------------------------------------------------------------------
// Env spec
//------------------------------------------------------------------------------

#[derive(Serialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub enum EnvInherit {
    None,
    All,
    Vars(Vec<String>), // FIXME: Use OsString instead?
}

impl Default for EnvInherit {
    fn default() -> Self {
        Self::All
    }
}

impl<'de> Deserialize<'de> for EnvInherit {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = EnvInherit;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("true, false, or seq of env var names")
            }

            fn visit_bool<E>(self, v: bool) -> std::result::Result<Self::Value, E> {
                Ok(if v {
                    Self::Value::All
                } else {
                    Self::Value::None
                })
            }

            fn visit_seq<S>(self, mut seq: S) -> std::result::Result<Self::Value, S::Error>
            where
                S: serde::de::SeqAccess<'de>,
            {
                let mut vars = Vec::new();
                while let Some(var) = seq.next_element()? {
                    vars.push(var);
                }
                Ok(Self::Value::Vars(vars))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Env {
    pub inherit: EnvInherit,
    pub vars: BTreeMap<String, String>,
}

//------------------------------------------------------------------------------
// Fd spec
//------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum OpenFlag {
    // FIXME: Generalize.
    /// Equivalent to `Read` for stdin, `Write` for stdout/stderr,
    /// `ReadWrite` for others.
    Default,

    /// Open existing file for reading.
    Read,
    /// Create or open exsting file for writing.
    Write,
    /// Create a new file for writing; file may not exist.
    Create,
    /// Overwrite an existing file for writing; file must exist.
    Replace,
    /// Create or open an existing file for appending.
    CreateAppend,
    /// Open an existing file for appending.
    Append,
    /// Create or open existing file for reading and writing.
    ReadWrite,
}

impl Default for OpenFlag {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "lowercase")]
pub enum CaptureMode {
    TempFile,
    Memory,
}

impl Default for CaptureMode {
    fn default() -> Self {
        Self::TempFile
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "lowercase")]
pub enum CaptureFormat {
    Text,
    Base64,
}

impl Default for CaptureFormat {
    fn default() -> Self {
        Self::Text
    }
}

fn get_default_mode() -> c_int {
    0o666
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "lowercase")]
pub enum Fd {
    /// Inherit this fd from the parent process, if any.
    Inherit,

    /// Close this fd, if it's open.
    Close,

    /// Open this fd to /dev/null.
    Null {
        #[serde(default)]
        flags: OpenFlag,
    },

    /// Open this fd to a file.
    File {
        path: PathBuf,
        #[serde(default)]
        flags: OpenFlag,
        #[serde(default = "get_default_mode")]
        mode: c_int,
        // format
    },

    /// Duplicate another existing fd to this one.
    Dup { fd: fd_t },

    /// Capture output from fd; include in results.
    Capture {
        #[serde(default)]
        mode: CaptureMode,

        #[serde(default)]
        format: CaptureFormat,
    },
}

impl Default for Fd {
    fn default() -> Self {
        Self::Inherit
    }
}

//------------------------------------------------------------------------------
// Process spec
//------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(deny_unknown_fields)]
#[serde(default)]
pub struct Proc {
    pub id: Option<String>,
    pub argv: Vec<String>,
    pub env: Env,
    pub fds: Vec<(String, Fd)>,
}

//------------------------------------------------------------------------------
// Input spec
//------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(deny_unknown_fields)]
pub struct Input {
    #[serde(deserialize_with = "one_or_many")]
    pub procs: Vec<Proc>,
}

pub fn load_file<P: AsRef<Path>>(path: P) -> Result<Input> {
    // Open the file in read-only mode with buffer.
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    // Read the JSON contents of the file as an instance of `Proc`.
    let spec = serde_json::from_reader(reader)?;

    // Return the spec.
    Ok(spec)
}

//------------------------------------------------------------------------------

/// Checks that no two specs have the same ID, and assigns in place an unused ID
/// to each spec that doesn't already have one.
pub fn assign_ids(input: &mut Input) -> std::result::Result<(), SpecError> {
    // Collecting existing IDs, and also duplicates.
    let mut ids = HashSet::<String>::new();
    let mut dups = HashSet::<String>::new();
    for proc in &mut input.procs {
        if let Some(id) = proc.id.clone() {
            if let Some(existing_id) = ids.replace(id) {
                dups.insert(existing_id);
            }
        }
    }

    // Assign an unused ID, generated from consecutive integers, to any spec
    // that doesn't already have an ID.
    let mut next: u64 = 0;
    for proc in &mut input.procs {
        if proc.id.is_none() {
            let id = loop {
                let id: String = next.to_string();
                next += 1;
                if !ids.contains(&id) {
                    break id;
                }
            };

            ids.insert(id.clone());
            proc.id = Some(id);
        }
    }

    if dups.len() == 0 {
        Ok(())
    } else {
        Err(SpecError::DupId(dups.into_iter().collect::<Vec<_>>()))
    }
}

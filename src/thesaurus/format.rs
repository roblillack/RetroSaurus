//! The on-disk format of the embedded WordNet index — the single source of
//! truth shared by the writer (`build.rs`, via `#[path]`) and the reader (the
//! library, via `mod format`).
//!
//! The blob is a small header followed by two regions:
//!
//! ```text
//! magic  "RETROSAU"        (8 bytes)
//! version u32              (FORMAT_VERSION)
//! source_sha [u8; 32]      sha-256 of the WN-LMF source, for build skip checks
//! fst_len  u64
//! tables_len u64
//! fst_bytes  [fst_len]     fst::Map<lowercased lemma -> group id>
//! tables     [tables_len]  postcard-encoded `Tables`
//! ```
//!
//! The `fst` answers prefix search; `Tables` holds the word / sense / synset
//! graph addressed by id. This module deals only in compact `u8` tags and `u32`
//! ids — the rich domain enums live in the parent module and translate through
//! the tag helpers here, so this file has no dependency on the rest of the crate
//! and compiles unchanged inside `build.rs`.

// This module is compiled into two contexts — the library (reader half) and
// `build.rs` (writer half) — so each sees the other half as unused.
#![allow(dead_code)]

use std::io::{self, Write};

use fst::automaton::{Automaton, Str};
use fst::{IntoStreamer, Map, MapBuilder, Streamer};
use serde::{Deserialize, Serialize};

/// Bumped whenever the byte layout or tag vocabulary changes; a mismatch forces
/// `build.rs` to regenerate and the reader to reject a stale blob.
pub const FORMAT_VERSION: u32 = 1;

const MAGIC: &[u8; 8] = b"RETROSAU";

/// A head word: a written form plus the ids of its senses, in display order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordRec {
    pub lemma: String,
    pub senses: Vec<u32>,
}

/// One sense: which synset it belongs to and its sense-level relations
/// (antonym, derivation, …) whose targets are *sense* ids.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenseRec {
    pub word: u32,
    pub synset: u32,
    pub rels: Vec<Rel>,
}

/// One synset: part of speech, gloss, examples, member words (the synonyms),
/// and synset-level relations (hypernym, …) whose targets are *synset* ids.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynsetRec {
    pub pos: u8,
    pub definition: String,
    pub examples: Vec<String>,
    pub members: Vec<u32>,
    pub rels: Vec<Rel>,
}

/// A typed edge: `kind` is a [`rel_tag`] value, `target` an id whose meaning
/// (sense vs synset) depends on which record holds the edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rel {
    pub kind: u8,
    pub target: u32,
}

/// The id-addressed graph. `groups[g]` lists every word id whose lower-cased
/// lemma folds to the same search key (case variants / homographs); the fst
/// maps that key to `g`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tables {
    pub words: Vec<WordRec>,
    pub senses: Vec<SenseRec>,
    pub synsets: Vec<SynsetRec>,
    pub groups: Vec<Vec<u32>>,
}

// --- Part-of-speech tags (must match `Pos::tag` in the parent module) --------

/// Map a WN-LMF part-of-speech code to its compact tag, folding the adjective
/// satellite (`s`) into the adjective tag. `None` for unknown codes.
pub fn pos_tag(code: &str) -> Option<u8> {
    match code {
        "n" => Some(0),
        "v" => Some(1),
        "a" | "s" => Some(2),
        "r" => Some(3),
        _ => None,
    }
}

// --- Relation tags -----------------------------------------------------------

pub const REL_ANTONYM: u8 = 0;
pub const REL_HYPERNYM: u8 = 1;
pub const REL_HYPONYM: u8 = 2;
pub const REL_HOLONYM: u8 = 3;
pub const REL_MERONYM: u8 = 4;
pub const REL_SIMILAR: u8 = 5;
pub const REL_ALSO: u8 = 6;
pub const REL_DERIVATION: u8 = 7;
pub const REL_PERTAINYM: u8 = 8;

/// Map a WN-LMF `relType` string to a tag we surface, collapsing the
/// part/member/substance and instance variants. `None` means "ignore this
/// relation" (most of WordNet's rarer edge types).
pub fn rel_tag(rel_type: &str) -> Option<u8> {
    match rel_type {
        "antonym" => Some(REL_ANTONYM),
        "hypernym" | "instance_hypernym" => Some(REL_HYPERNYM),
        "hyponym" | "instance_hyponym" => Some(REL_HYPONYM),
        "holonym" | "holo_part" | "holo_member" | "holo_substance" => Some(REL_HOLONYM),
        "meronym" | "mero_part" | "mero_member" | "mero_substance" => Some(REL_MERONYM),
        "similar" => Some(REL_SIMILAR),
        "also" => Some(REL_ALSO),
        "derivation" => Some(REL_DERIVATION),
        "pertainym" => Some(REL_PERTAINYM),
        _ => None,
    }
}

/// `true` if the tag is an antonym (rendered as its own list, not a related
/// group).
pub fn rel_is_antonym(tag: u8) -> bool {
    tag == REL_ANTONYM
}

/// Human-facing heading for a related-word group.
pub fn rel_label(tag: u8) -> &'static str {
    match tag {
        REL_ANTONYM => "antonyms",
        REL_HYPERNYM => "more general",
        REL_HYPONYM => "more specific",
        REL_HOLONYM => "part of",
        REL_MERONYM => "has parts",
        REL_SIMILAR => "similar to",
        REL_ALSO => "see also",
        REL_DERIVATION => "related forms",
        REL_PERTAINYM => "pertains to",
        _ => "related",
    }
}

/// Display order of related-word groups within a sense.
pub fn rel_order(tag: u8) -> u8 {
    match tag {
        REL_SIMILAR => 0,
        REL_HYPERNYM => 1,
        REL_HYPONYM => 2,
        REL_HOLONYM => 3,
        REL_MERONYM => 4,
        REL_DERIVATION => 5,
        REL_PERTAINYM => 6,
        REL_ALSO => 7,
        _ => 8,
    }
}

// --- Writer (used by build.rs) ----------------------------------------------

/// Serialize the index. `fst_entries` must be `(lower-cased key, group id)`
/// pairs sorted by key with no duplicates (the fst requirement); `tables` is the
/// id-addressed graph. `source_sha` is the sha-256 of the WN-LMF source.
pub fn write_index(
    fst_entries: &[(String, u64)],
    tables: &Tables,
    source_sha: [u8; 32],
) -> io::Result<Vec<u8>> {
    let mut builder = MapBuilder::memory();
    for (key, val) in fst_entries {
        builder
            .insert(key.as_bytes(), *val)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    }
    let fst_bytes = builder.into_inner().map_err(io::Error::other)?;

    let table_bytes =
        postcard::to_allocvec(tables).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let mut out = Vec::with_capacity(fst_bytes.len() + table_bytes.len() + 64);
    out.write_all(MAGIC)?;
    out.write_all(&FORMAT_VERSION.to_le_bytes())?;
    out.write_all(&source_sha)?;
    out.write_all(&(fst_bytes.len() as u64).to_le_bytes())?;
    out.write_all(&(table_bytes.len() as u64).to_le_bytes())?;
    out.write_all(&fst_bytes)?;
    out.write_all(&table_bytes)?;
    Ok(out)
}

/// Read just the header's `(version, source_sha)` without decoding the body —
/// used by `build.rs` to decide whether an existing blob is current.
pub fn peek_header(bytes: &[u8]) -> Option<(u32, [u8; 32])> {
    if bytes.len() < 8 + 4 + 32 || &bytes[..8] != MAGIC {
        return None;
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    let mut sha = [0u8; 32];
    sha.copy_from_slice(&bytes[12..44]);
    Some((version, sha))
}

// --- Reader (used by the library) -------------------------------------------

/// Why a blob failed to load.
#[derive(Debug)]
pub enum ReadError {
    BadMagic,
    Version { found: u32, expected: u32 },
    Truncated,
    Fst(fst::Error),
    Tables(postcard::Error),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::BadMagic => write!(f, "not a RetroSaurus index (bad magic)"),
            ReadError::Version { found, expected } => {
                write!(f, "index format version {found}, expected {expected}")
            }
            ReadError::Truncated => write!(f, "index is truncated"),
            ReadError::Fst(e) => write!(f, "fst error: {e}"),
            ReadError::Tables(e) => write!(f, "tables decode error: {e}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// The decoded index: an fst over lemmas plus the id-addressed [`Tables`]. Owns
/// its data (the fst region is copied), so it does not borrow the source bytes
/// and works equally for `include_bytes!` blobs and test buffers.
pub struct Index {
    fst: Map<Vec<u8>>,
    tables: Tables,
}

impl Index {
    /// Decode a blob produced by [`write_index`].
    pub fn read(bytes: &[u8]) -> Result<Index, ReadError> {
        if bytes.len() < 8 + 4 + 32 + 8 + 8 {
            return Err(ReadError::Truncated);
        }
        if &bytes[..8] != MAGIC {
            return Err(ReadError::BadMagic);
        }
        let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        if version != FORMAT_VERSION {
            return Err(ReadError::Version {
                found: version,
                expected: FORMAT_VERSION,
            });
        }
        let fst_len = u64::from_le_bytes(bytes[44..52].try_into().unwrap()) as usize;
        let tables_len = u64::from_le_bytes(bytes[52..60].try_into().unwrap()) as usize;
        let fst_start = 60;
        let tables_start = fst_start + fst_len;
        let end = tables_start + tables_len;
        if bytes.len() < end {
            return Err(ReadError::Truncated);
        }
        let fst = Map::new(bytes[fst_start..tables_start].to_vec()).map_err(ReadError::Fst)?;
        let tables = postcard::from_bytes(&bytes[tables_start..end]).map_err(ReadError::Tables)?;
        Ok(Index { fst, tables })
    }

    /// The id-addressed graph.
    pub fn tables(&self) -> &Tables {
        &self.tables
    }

    /// The group id for an exact lower-cased key, if present.
    pub fn group_of(&self, lower_key: &str) -> Option<u32> {
        self.fst.get(lower_key.as_bytes()).map(|v| v as u32)
    }

    /// Word ids whose lemma starts with `lower_prefix` (already lower-cased),
    /// in lexicographic order, capped at `limit`. Group members are appended in
    /// id order so case variants stay adjacent.
    pub fn prefix(&self, lower_prefix: &str, limit: usize) -> Vec<u32> {
        let mut out = Vec::new();
        if limit == 0 {
            return out;
        }
        let automaton = Str::new(lower_prefix).starts_with();
        let mut stream = self.fst.search(automaton).into_stream();
        'outer: while let Some((_key, group_id)) = stream.next() {
            if let Some(members) = self.tables.groups.get(group_id as usize) {
                for &word in members {
                    out.push(word);
                    if out.len() >= limit {
                        break 'outer;
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> (Vec<(String, u64)>, Tables) {
        // Two head words: "abandon" (group 0) and "Abandon" folding to the same
        // search key as "abandon"? Keep it simple: "abandon" and "able".
        let tables = Tables {
            words: vec![
                WordRec {
                    lemma: "abandon".into(),
                    senses: vec![0],
                },
                WordRec {
                    lemma: "able".into(),
                    senses: vec![1],
                },
            ],
            senses: vec![
                SenseRec {
                    word: 0,
                    synset: 0,
                    rels: vec![Rel {
                        kind: REL_ANTONYM,
                        target: 1,
                    }],
                },
                SenseRec {
                    word: 1,
                    synset: 1,
                    rels: vec![],
                },
            ],
            synsets: vec![
                SynsetRec {
                    pos: 1,
                    definition: "give up".into(),
                    examples: vec!["abandon ship".into()],
                    members: vec![0],
                    rels: vec![Rel {
                        kind: REL_HYPERNYM,
                        target: 1,
                    }],
                },
                SynsetRec {
                    pos: 2,
                    definition: "having the power".into(),
                    examples: vec![],
                    members: vec![1],
                    rels: vec![],
                },
            ],
            groups: vec![vec![0], vec![1]],
        };
        // fst keys must be sorted and unique.
        let entries = vec![("abandon".to_string(), 0u64), ("able".to_string(), 1u64)];
        (entries, tables)
    }

    #[test]
    fn round_trips() {
        let (entries, tables) = tiny();
        let sha = [7u8; 32];
        let bytes = write_index(&entries, &tables, sha).unwrap();

        assert_eq!(peek_header(&bytes), Some((FORMAT_VERSION, sha)));

        let index = Index::read(&bytes).unwrap();
        assert_eq!(index.tables(), &tables);
        assert_eq!(index.group_of("abandon"), Some(0));
        assert_eq!(index.group_of("able"), Some(1));
        assert_eq!(index.group_of("zebra"), None);
        assert_eq!(index.prefix("aba", 10), vec![0]);
        assert_eq!(index.prefix("ab", 10), vec![0, 1]); // "abandon" and "able"
        assert_eq!(index.prefix("a", 10), vec![0, 1]);
        assert_eq!(index.prefix("a", 1), vec![0]);
        assert_eq!(index.prefix("z", 10), Vec::<u32>::new());
    }

    #[test]
    fn rejects_bad_blobs() {
        assert!(matches!(
            Index::read(b"too short"),
            Err(ReadError::Truncated)
        ));
        let mut bytes = write_index(&[], &Tables::default(), [0u8; 32]).unwrap();
        bytes[0] = b'X';
        assert!(matches!(Index::read(&bytes), Err(ReadError::BadMagic)));
    }
}

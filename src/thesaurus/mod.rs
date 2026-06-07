//! The [`Thesaurus`] data interface and the denormalized view types the UI
//! renders.
//!
//! Everything the UI draws comes through [`Thesaurus`]: an
//! [`EmbeddedThesaurus`](index::EmbeddedThesaurus) backed by the WordNet index
//! baked into the binary, or a hand-built [`Fixture`](fixture::Fixture) for
//! tests and screenshots. Both hand back the same [`Entry`] shape, so the
//! definition-document builder and the widgets never touch WordNet's normalized
//! synset/sense graph directly.

pub mod fixture;
pub mod index;
// The on-disk index format, shared verbatim with `build.rs` via `#[path]`.
pub(crate) mod format;

pub use fixture::Fixture;
pub use index::EmbeddedThesaurus;

/// Stable identifier for a head word (a distinct written form, aggregating all
/// of its parts of speech and senses). Used as the selection key in the result
/// list and as the target of clickable cross-references.
pub type WordId = u32;

/// The four parts of speech RetroSaurus distinguishes. WordNet's adjective
/// satellite (`s`) folds into [`Pos::Adjective`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Pos {
    Noun,
    Verb,
    Adjective,
    Adverb,
}

impl Pos {
    /// Parse a WN-LMF part-of-speech code (`n`, `v`, `a`, `s`, `r`).
    pub fn from_code(code: &str) -> Option<Pos> {
        match code {
            "n" => Some(Pos::Noun),
            "v" => Some(Pos::Verb),
            "a" | "s" => Some(Pos::Adjective),
            "r" => Some(Pos::Adverb),
            _ => None,
        }
    }

    /// Compact byte tag used in the on-disk format.
    pub fn tag(self) -> u8 {
        match self {
            Pos::Noun => 0,
            Pos::Verb => 1,
            Pos::Adjective => 2,
            Pos::Adverb => 3,
        }
    }

    /// Inverse of [`Pos::tag`]; `None` for an unknown byte.
    pub fn from_tag(tag: u8) -> Option<Pos> {
        match tag {
            0 => Some(Pos::Noun),
            1 => Some(Pos::Verb),
            2 => Some(Pos::Adjective),
            3 => Some(Pos::Adverb),
            _ => None,
        }
    }

    /// Full label shown as a section header, e.g. `"noun"`.
    pub fn label(self) -> &'static str {
        match self {
            Pos::Noun => "noun",
            Pos::Verb => "verb",
            Pos::Adjective => "adjective",
            Pos::Adverb => "adverb",
        }
    }

    /// Canonical display order: noun, verb, adjective, adverb.
    pub fn order(self) -> u8 {
        self.tag()
    }
}

/// A clickable cross-reference to another head word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub word: WordId,
    pub lemma: String,
}

/// A labeled group of related words (e.g. *more general*, *parts*), surfaced
/// from WordNet's synset relations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelatedGroup {
    /// Human-facing label, e.g. `"more general"` for a hypernym.
    pub label: &'static str,
    pub links: Vec<Link>,
}

/// One sense of a head word, ready to render: its gloss, examples, synonyms,
/// antonyms, and related-word groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SenseView {
    pub pos: Pos,
    pub definition: String,
    pub examples: Vec<String>,
    pub synonyms: Vec<Link>,
    pub antonyms: Vec<Link>,
    pub related: Vec<RelatedGroup>,
}

/// A head word with all of its senses, grouped and ordered by part of speech —
/// the unit the definition pane renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub word: WordId,
    pub lemma: String,
    pub senses: Vec<SenseView>,
}

/// The read-only data interface the UI is built against.
///
/// Implementations resolve WordNet's normalized graph into the denormalized
/// [`Entry`] shape so the widgets stay backend-agnostic.
pub trait Thesaurus {
    /// Head words whose written form starts with `prefix` (case-insensitive),
    /// in lexicographic order, capped at `limit`.
    fn search_prefix(&self, prefix: &str, limit: usize) -> Vec<WordId>;

    /// The exact head word for `lemma` (case-insensitive), if one exists.
    fn lookup(&self, lemma: &str) -> Option<WordId>;

    /// The display written form for `word`.
    fn lemma(&self, word: WordId) -> Option<&str>;

    /// The full, denormalized entry for `word`.
    fn entry(&self, word: WordId) -> Option<Entry>;

    /// Total number of head words (used for "Random word").
    fn word_count(&self) -> usize;

    /// The head word at flat position `index` (`0..word_count()`), for picking a
    /// random word without exposing the id space.
    fn word_at(&self, index: usize) -> Option<WordId>;
}

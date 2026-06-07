//! The [`Thesaurus`] backed by the WordNet index baked into the binary.
//!
//! `build.rs` writes `$OUT_DIR/retrosaurus.dat`; here it is `include_bytes!`-ed
//! and decoded once into an [`Index`](super::format::Index). [`Thesaurus::entry`]
//! resolves WordNet's normalized sense/synset graph into the denormalized
//! [`Entry`] the UI renders.

use std::collections::BTreeMap;

use super::format::{self, Index, Tables};
use super::{Entry, Link, Pos, RelatedGroup, SenseView, Thesaurus, WordId};

/// The compiled index, embedded at build time.
static INDEX_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/retrosaurus.dat"));

/// Cap on links shown per related-word group, so dense synsets (e.g. the
/// hyponyms of *entity*) don't flood the pane.
const MAX_RELATED_PER_GROUP: usize = 16;

/// A [`Thesaurus`] over the embedded Open English WordNet index.
pub struct EmbeddedThesaurus {
    index: Index,
}

impl EmbeddedThesaurus {
    /// Decode the embedded index. Fails only if the baked blob is corrupt or
    /// from an incompatible format version — i.e. effectively never at runtime.
    pub fn load() -> Result<Self, format::ReadError> {
        Ok(Self {
            index: Index::read(INDEX_BYTES)?,
        })
    }

    fn tables(&self) -> &Tables {
        self.index.tables()
    }
}

impl Thesaurus for EmbeddedThesaurus {
    fn search_prefix(&self, prefix: &str, limit: usize) -> Vec<WordId> {
        self.index.prefix(&prefix.to_lowercase(), limit)
    }

    fn lookup(&self, lemma: &str) -> Option<WordId> {
        let group = self.index.group_of(&lemma.to_lowercase())?;
        let members = self.tables().groups.get(group as usize)?;
        // Prefer an exact-case match (e.g. "polish" over "Polish"), else the
        // first member of the folded group.
        members
            .iter()
            .copied()
            .find(|&w| self.lemma(w) == Some(lemma))
            .or_else(|| members.first().copied())
    }

    fn lemma(&self, word: WordId) -> Option<&str> {
        self.tables()
            .words
            .get(word as usize)
            .map(|w| w.lemma.as_str())
    }

    fn entry(&self, word: WordId) -> Option<Entry> {
        build_entry(self.tables(), word)
    }

    fn word_count(&self) -> usize {
        self.tables().words.len()
    }

    fn word_at(&self, index: usize) -> Option<WordId> {
        (index < self.word_count()).then_some(index as WordId)
    }
}

/// Resolve a head word into a render-ready [`Entry`].
fn build_entry(t: &Tables, word: WordId) -> Option<Entry> {
    let w = t.words.get(word as usize)?;
    let mut senses = Vec::with_capacity(w.senses.len());

    for &sense_id in &w.senses {
        let Some(sense) = t.senses.get(sense_id as usize) else {
            continue;
        };
        let Some(synset) = t.synsets.get(sense.synset as usize) else {
            continue;
        };
        let Some(pos) = Pos::from_tag(synset.pos) else {
            continue;
        };

        // Synonyms: the other members of this synset.
        let mut synonyms: Vec<Link> = synset
            .members
            .iter()
            .filter(|&&m| m != word)
            .filter_map(|&m| link(t, m))
            .collect();
        dedup_links(&mut synonyms);

        // Antonyms come from sense-level relations (targets are senses).
        let mut antonyms: Vec<Link> = sense
            .rels
            .iter()
            .filter(|r| format::rel_is_antonym(r.kind))
            .filter_map(|r| sense_link(t, r.target))
            .collect();
        dedup_links(&mut antonyms);

        // Related groups: synset relations (target synset → representative
        // member) plus non-antonym sense relations (target sense → its word),
        // bucketed by relation kind.
        let mut by_kind: BTreeMap<u8, Vec<Link>> = BTreeMap::new();
        for r in &synset.rels {
            if format::rel_is_antonym(r.kind) {
                continue;
            }
            if let Some(rep) = t
                .synsets
                .get(r.target as usize)
                .and_then(|s| s.members.first())
                .and_then(|&m| link(t, m))
            {
                by_kind.entry(r.kind).or_default().push(rep);
            }
        }
        for r in &sense.rels {
            if format::rel_is_antonym(r.kind) {
                continue;
            }
            if let Some(l) = sense_link(t, r.target) {
                by_kind.entry(r.kind).or_default().push(l);
            }
        }

        let mut related: Vec<(u8, RelatedGroup)> = by_kind
            .into_iter()
            .filter_map(|(kind, mut links)| {
                dedup_links(&mut links);
                links.truncate(MAX_RELATED_PER_GROUP);
                (!links.is_empty()).then(|| {
                    (
                        format::rel_order(kind),
                        RelatedGroup {
                            label: format::rel_label(kind),
                            links,
                        },
                    )
                })
            })
            .collect();
        related.sort_by_key(|(order, _)| *order);
        let related = related.into_iter().map(|(_, g)| g).collect();

        senses.push(SenseView {
            pos,
            definition: synset.definition.clone(),
            examples: synset.examples.clone(),
            synonyms,
            antonyms,
            related,
        });
    }

    // Group senses by part of speech in canonical order; stable within a group.
    senses.sort_by_key(|s| s.pos.order());

    Some(Entry {
        word,
        lemma: w.lemma.clone(),
        senses,
    })
}

/// A link to a head word by id.
fn link(t: &Tables, word: u32) -> Option<Link> {
    t.words.get(word as usize).map(|w| Link {
        word,
        lemma: w.lemma.clone(),
    })
}

/// A link to the head word owning sense `sense_id`.
fn sense_link(t: &Tables, sense_id: u32) -> Option<Link> {
    let word = t.senses.get(sense_id as usize)?.word;
    link(t, word)
}

/// Drop duplicate links by word id, preserving first-seen order.
fn dedup_links(links: &mut Vec<Link>) {
    let mut seen = std::collections::HashSet::new();
    links.retain(|l| seen.insert(l.word));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_index_loads_and_resolves() {
        let thes = EmbeddedThesaurus::load().expect("embedded index loads");
        assert!(
            thes.word_count() > 100_000,
            "expected the full WordNet, got {} words",
            thes.word_count()
        );

        let id = thes.lookup("abandon").expect("'abandon' is a head word");
        assert_eq!(thes.lemma(id), Some("abandon"));

        let entry = thes.entry(id).expect("entry for 'abandon'");
        assert!(!entry.senses.is_empty());
        // At least one sense should carry synonyms (other synset members).
        assert!(
            entry.senses.iter().any(|s| !s.synonyms.is_empty()),
            "expected synonyms for 'abandon'"
        );
        // Senses are grouped by part of speech in canonical order.
        let orders: Vec<u8> = entry.senses.iter().map(|s| s.pos.order()).collect();
        assert!(orders.windows(2).all(|w| w[0] <= w[1]));

        let hits = thes.search_prefix("aband", 50);
        assert!(hits.iter().any(|&w| thes.lemma(w) == Some("abandon")));
    }
}

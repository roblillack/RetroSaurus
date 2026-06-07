//! A tiny, hand-built [`Thesaurus`] for snapshot tests and screenshots.
//!
//! The sample dataset is intentionally small and self-referential: every
//! synonym / antonym / related lemma it names also exists as a head word, so
//! cross-reference navigation can be exercised deterministically without the
//! multi-megabyte WordNet index.

use std::collections::HashMap;

use super::{Entry, Link, Pos, RelatedGroup, SenseView, Thesaurus, WordId};

/// One authored sense; cross-references are written as lemma strings and
/// resolved to [`WordId`]s when the entry is requested.
struct FixSense {
    pos: Pos,
    definition: &'static str,
    examples: &'static [&'static str],
    synonyms: &'static [&'static str],
    antonyms: &'static [&'static str],
    related: &'static [(&'static str, &'static [&'static str])],
}

struct FixWord {
    lemma: &'static str,
    senses: Vec<FixSense>,
}

/// In-memory thesaurus over a fixed list of head words.
pub struct Fixture {
    words: Vec<FixWord>,
    /// Lower-cased lemma → word id, for search and cross-reference resolution.
    index: HashMap<String, WordId>,
}

impl Fixture {
    fn from_words(words: Vec<FixWord>) -> Self {
        let mut index = HashMap::new();
        for (id, w) in words.iter().enumerate() {
            index.insert(w.lemma.to_lowercase(), id as WordId);
        }
        Self { words, index }
    }

    /// Resolve a slice of lemma strings to navigable [`Link`]s, dropping any
    /// that aren't head words in this fixture.
    fn links(&self, lemmas: &[&'static str]) -> Vec<Link> {
        lemmas
            .iter()
            .filter_map(|l| {
                self.index.get(&l.to_lowercase()).map(|&word| Link {
                    word,
                    lemma: self.words[word as usize].lemma.to_string(),
                })
            })
            .collect()
    }

    /// The canonical sample dataset used by tests and screenshots.
    pub fn sample() -> Self {
        Self::from_words(vec![
            FixWord {
                lemma: "abandon",
                senses: vec![
                    FixSense {
                        pos: Pos::Verb,
                        definition: "give up with the intent of never claiming again",
                        examples: &["abandon the ship", "We abandoned the house in winter"],
                        synonyms: &["forsake", "desert", "give up"],
                        antonyms: &["hold"],
                        related: &[("more general", &["leave"])],
                    },
                    FixSense {
                        pos: Pos::Noun,
                        definition: "a feeling of extreme emotional intensity",
                        examples: &["he danced with abandon"],
                        synonyms: &["wildness"],
                        antonyms: &[],
                        related: &[],
                    },
                ],
            },
            FixWord {
                lemma: "forsake",
                senses: vec![FixSense {
                    pos: Pos::Verb,
                    definition: "leave someone who needs or counts on you",
                    examples: &["He forsook his friends"],
                    synonyms: &["abandon", "desert"],
                    antonyms: &["hold"],
                    related: &[("more general", &["leave"])],
                }],
            },
            FixWord {
                lemma: "desert",
                senses: vec![FixSense {
                    pos: Pos::Verb,
                    definition: "leave behind, especially in a difficult situation",
                    examples: &["The mother deserted her children"],
                    synonyms: &["abandon", "forsake"],
                    antonyms: &[],
                    related: &[],
                }],
            },
            FixWord {
                lemma: "give up",
                senses: vec![FixSense {
                    pos: Pos::Verb,
                    definition: "stop maintaining or insisting on something",
                    examples: &["give up the old ways"],
                    synonyms: &["abandon", "relinquish"],
                    antonyms: &[],
                    related: &[],
                }],
            },
            FixWord {
                lemma: "leave",
                senses: vec![FixSense {
                    pos: Pos::Verb,
                    definition: "go away from a place",
                    examples: &["She left the room"],
                    synonyms: &["depart", "go"],
                    antonyms: &["stay", "hold"],
                    related: &[],
                }],
            },
            FixWord {
                lemma: "hold",
                senses: vec![FixSense {
                    pos: Pos::Verb,
                    definition: "keep in a particular state or condition",
                    examples: &["hold one's ground"],
                    synonyms: &["keep", "retain"],
                    antonyms: &["abandon", "leave"],
                    related: &[],
                }],
            },
            FixWord {
                lemma: "happy",
                senses: vec![FixSense {
                    pos: Pos::Adjective,
                    definition: "enjoying or showing or marked by joy or pleasure",
                    examples: &["a happy smile", "spent many happy days on the beach"],
                    synonyms: &["glad", "cheerful"],
                    antonyms: &["unhappy"],
                    related: &[("similar to", &["cheerful"])],
                }],
            },
            FixWord {
                lemma: "glad",
                senses: vec![FixSense {
                    pos: Pos::Adjective,
                    definition: "showing or causing joy and pleasure",
                    examples: &["glad to see you"],
                    synonyms: &["happy", "cheerful"],
                    antonyms: &["unhappy"],
                    related: &[],
                }],
            },
            FixWord {
                lemma: "cheerful",
                senses: vec![FixSense {
                    pos: Pos::Adjective,
                    definition: "being full of or promoting cheer; having or showing good spirits",
                    examples: &["a cheerful person"],
                    synonyms: &["happy", "glad"],
                    antonyms: &["glum"],
                    related: &[],
                }],
            },
            FixWord {
                lemma: "unhappy",
                senses: vec![FixSense {
                    pos: Pos::Adjective,
                    definition: "experiencing or marked by or causing sadness or gloom",
                    examples: &["an unhappy marriage"],
                    synonyms: &["sad"],
                    antonyms: &["happy", "glad"],
                    related: &[],
                }],
            },
            FixWord {
                lemma: "dog",
                senses: vec![FixSense {
                    pos: Pos::Noun,
                    definition: "a member of the genus Canis that has been domesticated since prehistoric times",
                    examples: &["the dog barked all night"],
                    synonyms: &["domestic dog", "hound"],
                    antonyms: &[],
                    related: &[("more general", &["animal"]), ("more specific", &["puppy"])],
                }],
            },
            FixWord {
                lemma: "animal",
                senses: vec![FixSense {
                    pos: Pos::Noun,
                    definition: "a living organism characterized by voluntary movement",
                    examples: &["wild animals"],
                    synonyms: &["beast", "creature"],
                    antonyms: &[],
                    related: &[("more specific", &["dog"])],
                }],
            },
        ])
    }
}

impl Thesaurus for Fixture {
    fn search_prefix(&self, prefix: &str, limit: usize) -> Vec<WordId> {
        let prefix = prefix.to_lowercase();
        let mut ids: Vec<WordId> = self
            .words
            .iter()
            .enumerate()
            .filter(|(_, w)| w.lemma.to_lowercase().starts_with(&prefix))
            .map(|(id, _)| id as WordId)
            .collect();
        ids.sort_by(|&a, &b| {
            self.words[a as usize]
                .lemma
                .cmp(self.words[b as usize].lemma)
        });
        ids.truncate(limit);
        ids
    }

    fn lookup(&self, lemma: &str) -> Option<WordId> {
        self.index.get(&lemma.to_lowercase()).copied()
    }

    fn lemma(&self, word: WordId) -> Option<&str> {
        self.words.get(word as usize).map(|w| w.lemma)
    }

    fn entry(&self, word: WordId) -> Option<Entry> {
        let w = self.words.get(word as usize)?;
        let senses = w
            .senses
            .iter()
            .map(|s| SenseView {
                pos: s.pos,
                definition: s.definition.to_string(),
                examples: s.examples.iter().map(|e| e.to_string()).collect(),
                synonyms: self.links(s.synonyms),
                antonyms: self.links(s.antonyms),
                related: s
                    .related
                    .iter()
                    .map(|(label, lemmas)| RelatedGroup {
                        label,
                        links: self.links(lemmas),
                    })
                    .filter(|g| !g.links.is_empty())
                    .collect(),
            })
            .collect();
        Some(Entry {
            word,
            lemma: w.lemma.to_string(),
            senses,
        })
    }

    fn word_count(&self) -> usize {
        self.words.len()
    }

    fn word_at(&self, index: usize) -> Option<WordId> {
        (index < self.words.len()).then_some(index as WordId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_is_self_consistent() {
        let fx = Fixture::sample();
        assert!(fx.word_count() >= 10);
        // Every cross-reference resolves to a real head word.
        for id in 0..fx.word_count() {
            let entry = fx.entry(id as WordId).unwrap();
            for sense in &entry.senses {
                for link in sense
                    .synonyms
                    .iter()
                    .chain(&sense.antonyms)
                    .chain(sense.related.iter().flat_map(|g| &g.links))
                {
                    assert!(
                        fx.lemma(link.word).is_some(),
                        "dangling link {:?} from {}",
                        link,
                        entry.lemma
                    );
                }
            }
        }
    }

    #[test]
    fn prefix_search_is_case_insensitive_and_sorted() {
        let fx = Fixture::sample();
        let ids = fx.search_prefix("A", 10);
        let lemmas: Vec<&str> = ids.iter().map(|&id| fx.lemma(id).unwrap()).collect();
        assert_eq!(lemmas, vec!["abandon", "animal"]);
    }

    #[test]
    fn lookup_round_trips() {
        let fx = Fixture::sample();
        let id = fx.lookup("Happy").unwrap();
        assert_eq!(fx.lemma(id), Some("happy"));
    }
}

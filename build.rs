//! Build-time index generator.
//!
//! Resolves the Open English WordNet WN-LMF source (a local override, a cached
//! download, or a fresh fetch), streams it into the compact id-addressed graph,
//! and writes `$OUT_DIR/retrosaurus.dat` for the library to `include_bytes!`.
//! All of the heavy XML / gzip / hashing work lives here so none of it links
//! into the shipped binary.

// The on-disk format is shared verbatim with the library's reader.
#[path = "src/thesaurus/format.rs"]
mod format;

use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::read::GzDecoder;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use sha2::{Digest, Sha256};

use format::{
    FORMAT_VERSION, Rel, SenseRec, SynsetRec, Tables, WordRec, peek_header, pos_tag, rel_tag,
    write_index,
};

const SOURCE_URL: &str = "https://en-word.net/static/english-wordnet-2025.xml.gz";
const SOURCE_FILE: &str = "english-wordnet-2025.xml.gz";
const SOURCE_SHA256_HEX: &str = "9ca6d1dcb75f822fdd66617f7d9da48142ace38dd544d6ad5e2feca1674ad3fe";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/thesaurus/format.rs");
    println!("cargo:rerun-if-env-changed=RETROSAURUS_WORDNET_XML");
    println!("cargo:rerun-if-env-changed=RETROSAURUS_WORDNET_CACHE");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR set by cargo"));
    let dat_path = out_dir.join("retrosaurus.dat");

    let (source_path, source_sha) = resolve_source();
    if let Ok(p) = env::var("RETROSAURUS_WORDNET_XML") {
        println!("cargo:rerun-if-changed={p}");
    }

    // Skip regeneration if a current blob already exists.
    if let Ok(existing) = fs::read(&dat_path)
        && peek_header(&existing) == Some((FORMAT_VERSION, source_sha))
    {
        return;
    }

    let (fst_entries, tables) = build_tables(&source_path);
    let blob = write_index(&fst_entries, &tables, source_sha).expect("serialize index");
    fs::write(&dat_path, &blob).expect("write retrosaurus.dat");
}

// --- Source resolution -------------------------------------------------------

/// Returns the path to a WN-LMF `.xml.gz` and its sha-256. Prefers a local
/// override (`RETROSAURUS_WORDNET_XML`), then a verified cache, fetching once if
/// neither is present.
fn resolve_source() -> (PathBuf, [u8; 32]) {
    if let Ok(p) = env::var("RETROSAURUS_WORDNET_XML") {
        let path = PathBuf::from(&p);
        let sha = sha256_file(&path)
            .unwrap_or_else(|e| panic!("cannot read RETROSAURUS_WORDNET_XML ({p}): {e}"));
        return (path, sha);
    }

    // Cache the download so repeated builds skip the fetch. Default to OUT_DIR
    // so `cargo publish` stays happy — build scripts must not write outside
    // OUT_DIR. CI sets RETROSAURUS_WORDNET_CACHE to a stable, cached path so it
    // doesn't re-download on every run.
    let cache_dir = match env::var_os("RETROSAURUS_WORDNET_CACHE") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR set by cargo")),
    };
    let cache = cache_dir.join(SOURCE_FILE);
    if !cache.exists() {
        fs::create_dir_all(&cache_dir).expect("create WordNet cache dir");
        download(SOURCE_URL, &cache);
    }

    let sha = sha256_file(&cache).expect("hash cached source");
    let expected = decode_hex(SOURCE_SHA256_HEX);
    if sha != expected {
        panic!(
            "WordNet source checksum mismatch at {}:\n  got      {}\n  expected {}\nDelete the cache to re-download, or point RETROSAURUS_WORDNET_XML at a trusted copy.",
            cache.display(),
            encode_hex(&sha),
            SOURCE_SHA256_HEX,
        );
    }
    (cache, sha)
}

/// Fetch `url` to `dest` using whatever downloader the system provides. We shell
/// out rather than linking a Rust TLS stack into the build dependencies.
fn download(url: &str, dest: &Path) {
    println!("cargo:warning=RetroSaurus: downloading WordNet from {url} (first build only)");
    let attempts: [(&str, Vec<String>); 2] = [
        (
            "curl",
            vec![
                "-sSL".into(),
                "--fail".into(),
                "-o".into(),
                dest.display().to_string(),
                url.into(),
            ],
        ),
        (
            "wget",
            vec![
                "-q".into(),
                "-O".into(),
                dest.display().to_string(),
                url.into(),
            ],
        ),
    ];
    for (tool, args) in attempts {
        match Command::new(tool).args(&args).status() {
            Ok(s) if s.success() && dest.exists() => return,
            _ => {
                let _ = fs::remove_file(dest);
            }
        }
    }
    panic!(
        "could not download {url} (tried curl and wget).\nFetch it manually and set RETROSAURUS_WORDNET_XML to the file, e.g.:\n  curl -sSL -o {} {url}",
        dest.display(),
    );
}

fn sha256_file(path: &Path) -> std::io::Result<[u8; 32]> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

fn decode_hex(hex: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("valid hex");
    }
    out
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// --- WN-LMF parsing ----------------------------------------------------------

/// Mutable parser state accumulated across the streaming pass. Ids stay as
/// strings here; they are compacted to `u32` indices afterwards.
#[derive(Default)]
struct Parser {
    /// entry id → written form.
    entry_form: HashMap<String, String>,
    /// Parallel arrays indexed by sense index.
    sense_ids: Vec<String>,
    sense_index: HashMap<String, u32>,
    sense_entry: Vec<String>,
    sense_synset: Vec<String>,
    sense_rels: Vec<Vec<(u8, String)>>,
    /// Parallel arrays indexed by synset index.
    synset_index: HashMap<String, u32>,
    syn_pos: Vec<u8>,
    syn_def: Vec<String>,
    syn_ex: Vec<Vec<String>>,
    syn_members: Vec<Vec<String>>,
    syn_rels: Vec<Vec<(u8, String)>>,

    // Cursors into the open element.
    cur_entry: Option<String>,
    cur_sense: Option<usize>,
    cur_synset: Option<usize>,
    in_definition: bool,
    in_example: bool,
    text: String,
}

impl Parser {
    fn open(&mut self, e: &BytesStart, empty: bool) {
        match e.name().as_ref() {
            b"LexicalEntry" => self.cur_entry = attr(e, b"id"),
            b"Lemma" => {
                if let (Some(entry), Some(form)) = (&self.cur_entry, attr(e, b"writtenForm")) {
                    self.entry_form.insert(entry.clone(), form);
                }
            }
            b"Sense" => {
                let (Some(id), Some(synset), Some(entry)) =
                    (attr(e, b"id"), attr(e, b"synset"), self.cur_entry.clone())
                else {
                    return;
                };
                let idx = self.sense_ids.len();
                self.sense_index.insert(id.clone(), idx as u32);
                self.sense_ids.push(id);
                self.sense_entry.push(entry);
                self.sense_synset.push(synset);
                self.sense_rels.push(Vec::new());
                if !empty {
                    self.cur_sense = Some(idx);
                }
            }
            b"SenseRelation" => {
                if let (Some(idx), Some(tag), Some(target)) = (
                    self.cur_sense,
                    attr(e, b"relType").as_deref().and_then(rel_tag),
                    attr(e, b"target"),
                ) {
                    self.sense_rels[idx].push((tag, target));
                }
            }
            b"Synset" => {
                let Some(id) = attr(e, b"id") else { return };
                let idx = self.syn_pos.len();
                self.synset_index.insert(id, idx as u32);
                self.syn_pos.push(
                    attr(e, b"partOfSpeech")
                        .as_deref()
                        .and_then(pos_tag)
                        .unwrap_or(0),
                );
                self.syn_def.push(String::new());
                self.syn_ex.push(Vec::new());
                self.syn_members.push(
                    attr(e, b"members")
                        .map(|m| m.split_whitespace().map(str::to_string).collect())
                        .unwrap_or_default(),
                );
                self.syn_rels.push(Vec::new());
                self.cur_synset = Some(idx);
            }
            b"SynsetRelation" => {
                if let (Some(idx), Some(tag), Some(target)) = (
                    self.cur_synset,
                    attr(e, b"relType").as_deref().and_then(rel_tag),
                    attr(e, b"target"),
                ) {
                    self.syn_rels[idx].push((tag, target));
                }
            }
            b"Definition" if !empty => {
                self.in_definition = true;
                self.text.clear();
            }
            b"Example" if !empty => {
                self.in_example = true;
                self.text.clear();
            }
            _ => {}
        }
    }

    fn close(&mut self, name: &[u8]) {
        match name {
            b"LexicalEntry" => self.cur_entry = None,
            b"Sense" => self.cur_sense = None,
            b"Synset" => self.cur_synset = None,
            b"Definition" => {
                if let Some(idx) = self.cur_synset
                    && self.syn_def[idx].is_empty()
                {
                    self.syn_def[idx] = self.text.trim().to_string();
                }
                self.in_definition = false;
            }
            b"Example" => {
                if let Some(idx) = self.cur_synset {
                    let text = self.text.trim().to_string();
                    if !text.is_empty() {
                        self.syn_ex[idx].push(text);
                    }
                }
                self.in_example = false;
            }
            _ => {}
        }
    }
}

/// Stream the WN-LMF source and compact it into [`Tables`] plus the sorted fst
/// entries `(lower-cased lemma, group id)`.
fn build_tables(source: &Path) -> (Vec<(String, u64)>, Tables) {
    let file = fs::File::open(source).expect("open WordNet source");
    let mut reader = Reader::from_reader(BufReader::new(GzDecoder::new(BufReader::new(file))));
    let mut parser = Parser::default();
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .expect("WN-LMF parse error")
        {
            Event::Eof => break,
            Event::Start(e) => parser.open(&e, false),
            Event::Empty(e) => parser.open(&e, true),
            Event::End(e) => parser.close(e.name().as_ref()),
            Event::Text(e) if parser.in_definition || parser.in_example => {
                if let Ok(t) = e.unescape() {
                    parser.text.push_str(&t);
                }
            }
            _ => {}
        }
        buf.clear();
    }

    compact(parser)
}

/// Assign compact ids and resolve every string reference.
fn compact(p: Parser) -> (Vec<(String, u64)>, Tables) {
    // Distinct written forms → word ids (sorted for reproducible builds).
    let mut forms: Vec<String> = p.entry_form.values().cloned().collect();
    forms.sort();
    forms.dedup();
    let form_word: HashMap<&str, u32> = forms
        .iter()
        .enumerate()
        .map(|(i, f)| (f.as_str(), i as u32))
        .collect();
    let entry_word = |entry: &str| -> Option<u32> {
        p.entry_form
            .get(entry)
            .and_then(|f| form_word.get(f.as_str()).copied())
    };

    // Senses + per-word sense lists.
    let mut word_senses: Vec<Vec<u32>> = vec![Vec::new(); forms.len()];
    let mut senses = Vec::with_capacity(p.sense_ids.len());
    for i in 0..p.sense_ids.len() {
        let word = entry_word(&p.sense_entry[i]);
        let synset = p.synset_index.get(&p.sense_synset[i]).copied();
        let rels = p.sense_rels[i]
            .iter()
            .filter_map(|(kind, tgt)| {
                p.sense_index.get(tgt).map(|&target| Rel {
                    kind: *kind,
                    target,
                })
            })
            .collect();
        senses.push(SenseRec {
            word: word.unwrap_or(u32::MAX),
            synset: synset.unwrap_or(u32::MAX),
            rels,
        });
        if let (Some(word), Some(_)) = (word, synset) {
            word_senses[word as usize].push(i as u32);
        }
    }

    // Synsets.
    let mut synsets = Vec::with_capacity(p.syn_pos.len());
    for i in 0..p.syn_pos.len() {
        let mut members: Vec<u32> = Vec::new();
        for eid in &p.syn_members[i] {
            if let Some(w) = entry_word(eid)
                && !members.contains(&w)
            {
                members.push(w);
            }
        }
        let rels = p.syn_rels[i]
            .iter()
            .filter_map(|(kind, tgt)| {
                p.synset_index.get(tgt).map(|&target| Rel {
                    kind: *kind,
                    target,
                })
            })
            .collect();
        synsets.push(SynsetRec {
            pos: p.syn_pos[i],
            definition: p.syn_def[i].clone(),
            examples: p.syn_ex[i].clone(),
            members,
            rels,
        });
    }

    // Words.
    let words: Vec<WordRec> = forms
        .iter()
        .enumerate()
        .map(|(i, f)| WordRec {
            lemma: f.clone(),
            senses: std::mem::take(&mut word_senses[i]),
        })
        .collect();

    // Groups (case/homograph folding) + fst entries.
    let mut groups_map: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (word, form) in forms.iter().enumerate() {
        groups_map
            .entry(form.to_lowercase())
            .or_default()
            .push(word as u32);
    }
    let mut groups = Vec::with_capacity(groups_map.len());
    let mut fst_entries = Vec::with_capacity(groups_map.len());
    for (group_id, (key, members)) in groups_map.into_iter().enumerate() {
        fst_entries.push((key, group_id as u64));
        groups.push(members);
    }

    (
        fst_entries,
        Tables {
            words,
            senses,
            synsets,
            groups,
        },
    )
}

fn attr(e: &BytesStart, key: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        (a.key.as_ref() == key)
            .then(|| a.unescape_value().ok().map(|v| v.into_owned()))
            .flatten()
    })
}

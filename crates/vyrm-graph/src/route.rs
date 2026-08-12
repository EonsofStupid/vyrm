//! The routing projection and its maintenance.
//!
//! A query returns a ranked **file list**, not fragments. The consuming rule is
//! that those files are read in full: a fragment is why an agent authors
//! `08-thing.md` beside `05-thing.md` instead of extending it, having never seen
//! the whole of `05`.
//!
//! Reading in full is expensive, so the saving does not come from the reading
//! rule. It comes from precision. Measured by `examples/route_vs_scan.rs`:
//! 5.61x fewer lines to read at a fixed five files on a 1,616-file repository,
//! 13.87x with the 1,000-line budget fill; 1.79x and 2.22x on a 33-file one.
//! The layer is worth engaging in the first case and not the second, and the
//! difference is countable before the cost is paid.
//!
//! ## Maintenance
//!
//! [`Index::refresh`] re-extracts only what changed, identified by modification
//! time and content digest (`SPEC.md` §8.2). [`Index::ground`] rebuilds from
//! scratch and differences the result against the incrementally maintained index;
//! divergence halts rather than being repaired (§8.3).
//!
//! Definitions are keyed by path, not by position. Positional keys survive a full
//! rebuild but not an incremental one, where a removed file shifts every index
//! after it.
//!
//! ## Ranking
//!
//! Scoring uses two signals: declaration sites (dominant, by construction) and
//! reference density (capped). Reference-graph centrality — PageRank over
//! file-to-file edges drawn from each file's references to names other files
//! define, the precedent set by aider's repository map — is computed but kept
//! out of the score: the 2026-08-11 decomposition run measured that weighting
//! it in (at 30.0) cost lines on every ratio (fixed-5 fell 5.61x to 4.20x on
//! the 1,616-file repository, budget fill 14.07x to 13.32x) and improved the
//! declaration-first rate on no query, because central files are systematically
//! larger. It serves as a sort tie-breaker instead, ranking the definer the
//! repository actually leans on above an otherwise equal one, and it can only
//! reorder files already related to the query.
//!
//! [`Index::route_budget`] fills a line budget instead of taking a fixed count,
//! so a query whose definer is small is not padded out with reference-heavy
//! filler. Same run, same queries: budget fill at 1,000 lines took the
//! lines-to-read ratio from 5.61x to 14.07x with the declaration still ranked
//! first on every query that has one.

use crate::profile::{Language, Profile};
use crate::symbols::{self, Occurrence, Role};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// FNV-1a. Stable across processes and Rust versions, unlike `DefaultHasher`,
/// which matters once an index is persisted rather than rebuilt each run.
fn digest(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedFile {
    pub path: PathBuf,
    pub language: Language,
    pub lines: usize,
    pub occurrences: Vec<Occurrence>,
    /// Identifier-like terms to the number of lines mentioning each. Routing
    /// answers from this rather than re-reading the file, so the index is the
    /// projection in fact and not only in name.
    pub terms: BTreeMap<String, usize>,
    /// Content digest, for detecting modification.
    pub digest: u64,
    /// Modification time and length, used to skip reading an unchanged file at
    /// all. This is where the refresh cost actually goes.
    pub mtime_secs: u64,
    pub byte_len: u64,
}

/// What a refresh did. Reported so that "nothing changed" is visible rather than
/// indistinguishable from "refresh did not run".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Refresh {
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
    /// Files whose modification time and length matched, so they were never read.
    pub skipped_unread: usize,
    /// Files read but found identical by digest — touched, not modified.
    pub read_but_identical: usize,
    pub duration_ms: u128,
}

impl Refresh {
    pub fn is_noop(&self) -> bool {
        self.added == 0 && self.changed == 0 && self.removed == 0
    }

    pub fn render(&self) -> String {
        format!(
            "+{} ~{} -{} (unread {}, identical {}) in {} ms",
            self.added,
            self.changed,
            self.removed,
            self.skipped_unread,
            self.read_but_identical,
            self.duration_ms
        )
    }
}

/// Result of grounding the index against a full rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grounding {
    pub agreed: bool,
    pub only_in_incremental: Vec<PathBuf>,
    pub only_in_rebuild: Vec<PathBuf>,
    pub differing: Vec<PathBuf>,
    pub duration_ms: u128,
}

impl Grounding {
    pub fn render(&self) -> String {
        if self.agreed {
            return format!("grounded: incremental index agrees with full rebuild ({} ms)", self.duration_ms);
        }
        format!(
            "DIVERGENCE: {} stale, {} missing, {} differing — projection quarantined",
            self.only_in_incremental.len(),
            self.only_in_rebuild.len(),
            self.differing.len()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Justification {
    pub defines: Vec<String>,
    pub reference_lines: usize,
    pub imports_a_definer: bool,
}

impl Justification {
    pub fn render(&self) -> String {
        let mut parts = Vec::new();
        if !self.defines.is_empty() {
            parts.push(format!("defines {}", self.defines.join(", ")));
        }
        if self.reference_lines > 0 {
            parts.push(format!("{} reference line(s)", self.reference_lines));
        }
        if self.imports_a_definer {
            parts.push("imports a defining module".to_string());
        }
        parts.join("; ")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutedFile {
    pub path: PathBuf,
    pub score: f64,
    pub lines: usize,
    /// Reference-graph centrality in [0, 1], normalized to the most central file.
    /// Carried separately from the justification so a ranking driven by
    /// centrality is diagnosable rather than mysterious.
    pub centrality: f64,
    pub justification: Justification,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Index {
    files: BTreeMap<PathBuf, IndexedFile>,
    definitions: BTreeMap<String, BTreeSet<PathBuf>>,
    /// Reference-graph centrality per file, normalized to [0, 1]. Derived from
    /// `files` and `definitions` whenever they change, and recomputed rather
    /// than persisted: serde_json's default float parse can land one ULP off
    /// the written value (measured 2026-08-12 — a persisted 0.…657 loaded as
    /// 0.…656 and broke route parity), and more fundamentally a projection of
    /// a projection must not be able to drift from its source.
    #[serde(skip)]
    centrality: BTreeMap<PathBuf, f64>,
    /// Bumped by every refresh that changed anything. A caller can compare
    /// generations to know whether its view is current.
    generation: u64,
}

fn file_stats(path: &Path) -> (u64, u64) {
    std::fs::metadata(path)
        .map(|m| {
            let secs = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            (secs, m.len())
        })
        .unwrap_or((0, 0))
}

/// Lines mentioning each identifier-like term, lowercased.
///
/// Terms shorter than three characters are dropped: they carry no routing signal
/// and dominate the table.
fn term_table(text: &str) -> BTreeMap<String, usize> {
    let mut table: BTreeMap<String, usize> = BTreeMap::new();
    for line in text.lines() {
        let mut seen_on_line: BTreeSet<String> = BTreeSet::new();
        for word in line.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if word.len() >= 3 {
                seen_on_line.insert(word.to_lowercase());
            }
        }
        for term in seen_on_line {
            *table.entry(term).or_insert(0) += 1;
        }
    }
    table
}

/// The entity a file declares by its name alone.
///
/// Some entities have no declaration line: a Svelte component is declared by
/// its filename, and a module like `utils/terminology.ts` is the definition
/// site of `terminology` even when nothing inside it bears that name — the
/// 2026-08-12 evidence pass on the reference repository found `terminology`
/// existing only as two filename stems with eight importers. An entry file
/// (`index.ts`, `mod.rs`, `__init__.py`) declares its directory's name
/// instead of its own meaningless stem. `lib.rs` and `main.rs` are left as
/// their own stems: their parent is usually `src`, which names nothing.
///
/// The stem is taken up to the first dot, so `hardwareState.svelte.ts`
/// declares `hardwareState`. Non-identifier stems (`+page.svelte`) declare
/// nothing.
fn module_entity(path: &Path) -> Option<String> {
    let stem = path.file_name()?.to_str()?.split('.').next()?;
    let name = match stem {
        "index" | "mod" | "__init__" => path.parent()?.file_name()?.to_str()?,
        other => other,
    };
    let mut chars = name.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return None;
    }
    Some(name.to_string())
}

fn extract_file(path: &Path, text: &str) -> IndexedFile {
    let language = Language::of_path(path);
    let mut occurrences = symbols::extract(text, language);
    // Filename-level entity synthesis. Skipped when the file already declares
    // the name, so a real declaration line is never shadowed by line 1.
    if let Some(entity) = module_entity(path) {
        if !occurrences.iter().any(|o| o.role == Role::Definition && o.name == entity) {
            occurrences.push(Occurrence { name: entity, role: Role::Definition, line: 1 });
        }
    }
    let (mtime_secs, byte_len) = file_stats(path);
    IndexedFile {
        path: path.to_path_buf(),
        language,
        lines: text.lines().count(),
        occurrences,
        terms: term_table(text),
        digest: digest(text.as_bytes()),
        mtime_secs,
        byte_len,
    }
}

impl Index {
    /// Builds the index from scratch.
    pub fn build(profile: &Profile) -> std::io::Result<Index> {
        let mut index = Index::default();
        index.refresh(profile)?;
        Ok(index)
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn symbol_count(&self) -> usize {
        self.definitions.len()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Re-extracts only what changed.
    ///
    /// An unchanged file is identified by modification time and length and is
    /// never read. That is where the saving is: the cost of a refresh is
    /// proportional to what changed, not to the size of the repository.
    pub fn refresh(&mut self, profile: &Profile) -> std::io::Result<Refresh> {
        let started = Instant::now();
        let mut report = Refresh::default();
        let present = profile.indexable_files()?;
        let present_set: BTreeSet<PathBuf> = present.iter().cloned().collect();

        for path in present {
            let (mtime_secs, byte_len) = file_stats(&path);
            if let Some(existing) = self.files.get(&path) {
                if existing.mtime_secs == mtime_secs && existing.byte_len == byte_len {
                    report.skipped_unread += 1;
                    continue;
                }
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue; // binary or unreadable: skipped, not fatal
            };
            let candidate = extract_file(&path, &text);
            match self.files.get(&path) {
                Some(existing) if existing.digest == candidate.digest => {
                    // Touched but not modified. Record the new stats so the next
                    // refresh can skip the read entirely.
                    report.read_but_identical += 1;
                    self.files.insert(path, candidate);
                }
                Some(_) => {
                    report.changed += 1;
                    self.files.insert(path, candidate);
                }
                None => {
                    report.added += 1;
                    self.files.insert(path, candidate);
                }
            }
        }

        let removed: Vec<PathBuf> = self
            .files
            .keys()
            .filter(|p| !present_set.contains(*p))
            .cloned()
            .collect();
        report.removed = removed.len();
        for path in removed {
            self.files.remove(&path);
        }

        if !report.is_noop() {
            self.generation += 1;
            self.rebuild_definitions();
        }
        report.duration_ms = started.elapsed().as_millis();
        Ok(report)
    }

    /// Full recomputation, differenced against this index.
    ///
    /// `SPEC.md` §8.3: divergence is reported and the projection is treated as
    /// quarantined. It is not silently repaired, because an incremental path that
    /// drifts from its source is the failure this layer exists to prevent, and
    /// repairing it in place would hide the defect that caused it.
    pub fn ground(&self, profile: &Profile) -> std::io::Result<Grounding> {
        let started = Instant::now();
        let mut rebuilt = Index::default();
        rebuilt.refresh(profile)?;

        let mine: BTreeSet<&PathBuf> = self.files.keys().collect();
        let theirs: BTreeSet<&PathBuf> = rebuilt.files.keys().collect();

        let only_in_incremental: Vec<PathBuf> =
            mine.difference(&theirs).map(|p| (*p).clone()).collect();
        let only_in_rebuild: Vec<PathBuf> =
            theirs.difference(&mine).map(|p| (*p).clone()).collect();
        let differing: Vec<PathBuf> = mine
            .intersection(&theirs)
            .filter(|path| {
                let a = &self.files[**path];
                let b = &rebuilt.files[**path];
                a.digest != b.digest || a.occurrences != b.occurrences
            })
            .map(|p| (*p).clone())
            .collect();

        Ok(Grounding {
            agreed: only_in_incremental.is_empty()
                && only_in_rebuild.is_empty()
                && differing.is_empty(),
            only_in_incremental,
            only_in_rebuild,
            differing,
            duration_ms: started.elapsed().as_millis(),
        })
    }

    fn rebuild_definitions(&mut self) {
        self.definitions.clear();
        for (path, file) in &self.files {
            for occurrence in &file.occurrences {
                if occurrence.role == Role::Definition {
                    self.definitions
                        .entry(occurrence.name.clone())
                        .or_default()
                        .insert(path.clone());
                }
            }
        }
        self.rebuild_centrality();
    }

    /// PageRank over the file reference graph.
    ///
    /// An edge runs from a referencing file to each file defining the referenced
    /// name, weighted by the number of referencing lines, so rank flows toward
    /// the definers the rest of the repository leans on. Damping 0.85, a fixed
    /// 30 iterations rather than a convergence test: a fixed schedule over
    /// `BTreeMap` iteration order makes the result a pure function of the index
    /// contents, so an incremental index and a full rebuild of the same tree
    /// produce identical centrality without the comparison ever being run.
    fn rebuild_centrality(&mut self) {
        self.centrality.clear();
        let n = self.files.len();
        if n == 0 {
            return;
        }

        let paths: Vec<&PathBuf> = self.files.keys().collect();
        let index_of: BTreeMap<&PathBuf, usize> =
            paths.iter().enumerate().map(|(i, p)| (*p, i)).collect();

        // Defined names, lowercased to match the term table. Distinct names that
        // collide when lowercased merge their definer sets, which is the same
        // conflation the term table itself makes.
        let mut definers_of_term: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (name, definers) in &self.definitions {
            let entry = definers_of_term.entry(name.to_lowercase()).or_default();
            entry.extend(definers.iter().map(|p| index_of[p]));
        }
        for definers in definers_of_term.values_mut() {
            definers.sort_unstable();
            definers.dedup();
        }

        let mut out_edges: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
        for (from, path) in paths.iter().enumerate() {
            let mut weights: BTreeMap<usize, f64> = BTreeMap::new();
            for (term, reference_lines) in &self.files[*path].terms {
                if let Some(definers) = definers_of_term.get(term) {
                    for &to in definers {
                        if to != from {
                            *weights.entry(to).or_insert(0.0) += *reference_lines as f64;
                        }
                    }
                }
            }
            out_edges[from] = weights.into_iter().collect();
        }

        const DAMPING: f64 = 0.85;
        const ITERATIONS: usize = 30;
        let uniform = 1.0 / n as f64;
        let mut rank = vec![uniform; n];
        for _ in 0..ITERATIONS {
            let mut next = vec![(1.0 - DAMPING) * uniform; n];
            let mut dangling = 0.0;
            for (from, edges) in out_edges.iter().enumerate() {
                if edges.is_empty() {
                    dangling += rank[from];
                    continue;
                }
                let total: f64 = edges.iter().map(|(_, weight)| weight).sum();
                for &(to, weight) in edges {
                    next[to] += DAMPING * rank[from] * weight / total;
                }
            }
            let dangling_share = DAMPING * dangling * uniform;
            for value in next.iter_mut() {
                *value += dangling_share;
            }
            rank = next;
        }

        let max = rank.iter().cloned().fold(0.0f64, f64::max);
        if max > 0.0 {
            self.centrality = paths
                .iter()
                .enumerate()
                .map(|(i, path)| ((*path).clone(), rank[i] / max))
                .collect();
        }
    }

    /// Routes a query to a ranked file list.
    pub fn route(&self, query: &str, limit: usize) -> Vec<RoutedFile> {
        let definers: BTreeSet<PathBuf> =
            self.definitions.get(query).cloned().unwrap_or_default();
        let definer_stems: Vec<String> = definers
            .iter()
            .filter_map(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .collect();

        let lowered = query.to_lowercase();
        let mut routed: Vec<RoutedFile> = Vec::new();
        for (path, file) in &self.files {
            let defines: Vec<String> = file
                .occurrences
                .iter()
                .filter(|o| o.role == Role::Definition && o.name == query)
                .map(|o| o.name.clone())
                .collect();

            // Answered from the index, not from disk. Reading here would make
            // routing reflect the tree rather than the projection, and would
            // render grounding meaningless: a stale index would still return
            // current results.
            let reference_lines = file.terms.get(&lowered).copied().unwrap_or(0);

            let imports_a_definer = !definer_stems.is_empty()
                && !definers.contains(path)
                && file.occurrences.iter().any(|o| {
                    o.role == Role::Import && definer_stems.iter().any(|stem| o.name.contains(stem))
                });

            // A declaration must outrank any amount of usage. Reference weight is
            // capped, and the definition weight exceeds that cap, so a heavily
            // calling file can never displace the site that declares the name.
            //
            // Centrality is deliberately absent from the score. Weighted into it
            // (at 30.0), the 2026-08-11 decomposition run measured a cost and no
            // benefit on the reference queries: fixed-5 lines-to-read fell from
            // 5.61x to 4.20x because central files are systematically larger,
            // and top=def improved on no query. It acts as a sort tie-breaker
            // below instead, where it decides only what relevance cannot.
            const REFERENCE_CAP: f64 = 20.0;
            const DEFINITION_WEIGHT: f64 = 100.0;
            let score = (defines.len() as f64) * DEFINITION_WEIGHT
                + (reference_lines as f64).min(REFERENCE_CAP)
                + if imports_a_definer { 3.0 } else { 0.0 };
            let centrality = self.centrality.get(path).copied().unwrap_or(0.0);

            if score > 0.0 {
                routed.push(RoutedFile {
                    path: path.clone(),
                    score,
                    lines: file.lines,
                    centrality,
                    justification: Justification { defines, reference_lines, imports_a_definer },
                });
            }
        }

        routed.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    b.centrality.partial_cmp(&a.centrality).unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.path.cmp(&b.path))
        });
        routed.truncate(limit);
        routed
    }

    /// Routes a query, filling a line budget instead of taking a fixed count.
    ///
    /// Files are taken in rank order; one that would overflow the budget is
    /// skipped and the fill continues, so a smaller, lower-ranked file can still
    /// be routed (first-fit, not knapsack-optimal — rank order dominates and an
    /// optimal packing could displace a higher-ranked file). The top-ranked file
    /// is always included even when it alone exceeds the budget, because an
    /// empty answer is worse than an oversized one.
    ///
    /// The budget unit is lines, because lines are what the index already
    /// counts. A token estimate would add a lines-to-tokens model this layer has
    /// not measured; when one is measured, it belongs in the caller.
    pub fn route_budget(&self, query: &str, line_budget: usize) -> Vec<RoutedFile> {
        let ranked = self.route(query, usize::MAX);
        let mut spent = 0usize;
        let mut out = Vec::new();
        for file in ranked {
            if out.is_empty() || spent + file.lines <= line_budget {
                spent += file.lines;
                out.push(file);
            }
        }
        out
    }

    /// Serializes the index for persistence. Centrality is excluded: it is
    /// derived, so [`Index::from_bytes`] recomputes it instead of trusting a
    /// stored copy.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("an index serializes: its types are all serializable")
    }

    /// Restores a persisted index and recomputes its derived state, so a
    /// loaded index is bit-identical in behavior to the one that was saved.
    pub fn from_bytes(bytes: &[u8]) -> Result<Index, serde_json::Error> {
        let mut index: Index = serde_json::from_slice(bytes)?;
        index.rebuild_centrality();
        Ok(index)
    }

    /// Refreshes, then routes. The barrier a caller uses when it needs the answer
    /// to reflect the tree as it is now rather than as it was.
    pub fn route_fresh(
        &mut self,
        profile: &Profile,
        query: &str,
        limit: usize,
    ) -> std::io::Result<(Refresh, Vec<RoutedFile>)> {
        let refresh = self.refresh(profile)?;
        let routed = self.route(query, limit);
        Ok((refresh, routed))
    }

    /// Files whose text contains the query at all — the set a plain text scan
    /// would surface, retained as the comparison baseline.
    pub fn text_scan(&self, query: &str) -> Vec<PathBuf> {
        let lowered = query.to_lowercase();
        self.files
            .iter()
            .filter(|(_, file)| file.terms.contains_key(&lowered))
            .map(|(path, _)| path.clone())
            .collect()
    }
}

/// Attunes to a project and builds its routing index in one step.
pub fn attune_and_index(root: &Path) -> std::io::Result<(Profile, Index)> {
    let profile = Profile::attune(root)?;
    let index = Index::build(&profile)?;
    Ok((profile, index))
}

//! IDE workload engine.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

/// A single entry in the project index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    /// File path.
    pub path: String,
    /// Symbol names defined in this file.
    pub symbols: Vec<String>,
    /// Last-modified timestamp (epoch seconds).
    pub last_modified: u64,
    /// File size in bytes.
    pub size_bytes: u64,
}

/// Statistics about the project index.
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    /// Total indexed files.
    pub file_count: usize,
    /// Total symbols across all files.
    pub symbol_count: usize,
    /// Total bytes across all files.
    pub total_bytes: u64,
}

/// An index of all files and symbols in a project.
#[derive(Debug, Default)]
pub struct ProjectIndex {
    entries: HashMap<String, IndexEntry>,
}

impl ProjectIndex {
    /// Create an empty project index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace an entry for the given path.
    pub fn add(&mut self, entry: IndexEntry) {
        self.entries.insert(entry.path.clone(), entry);
    }

    /// Remove an entry by path. Returns true if the entry existed.
    pub fn remove(&mut self, path: &str) -> bool {
        self.entries.remove(path).is_some()
    }

    /// Get an entry by path.
    pub fn get(&self, path: &str) -> Option<&IndexEntry> {
        self.entries.get(path)
    }

    /// Search for entries containing the given symbol name.
    pub fn search_by_symbol(&self, symbol: &str) -> Vec<&IndexEntry> {
        let mut results: Vec<_> = self
            .entries
            .values()
            .filter(|e| e.symbols.iter().any(|s| s.contains(symbol)))
            .collect();
        results.sort_by(|a, b| a.path.cmp(&b.path));
        results
    }

    /// Search for entries whose path contains the given substring.
    pub fn search_by_path(&self, pattern: &str) -> Vec<&IndexEntry> {
        let mut results: Vec<_> = self
            .entries
            .values()
            .filter(|e| e.path.contains(pattern))
            .collect();
        results.sort_by(|a, b| a.path.cmp(&b.path));
        results
    }

    /// Return aggregate statistics about the index.
    pub fn stats(&self) -> IndexStats {
        let symbol_count = self.entries.values().fold(0usize, |total, entry| {
            total.saturating_add(entry.symbols.len())
        });
        let total_bytes = self
            .entries
            .values()
            .fold(0u64, |total, entry| total.saturating_add(entry.size_bytes));
        IndexStats {
            file_count: self.entries.len(),
            symbol_count,
            total_bytes,
        }
    }
}

/// Status of a language server process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LspStatus {
    /// Server is being initialized.
    Starting,
    /// Server is running and accepting requests.
    Running,
    /// Server has been stopped gracefully.
    Stopped,
    /// Server crashed with an error message.
    Crashed(String),
}

/// State recorded for a language-server process owned by an external process
/// supervisor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspProcess {
    /// Language identifier (e.g. "rust", "typescript").
    pub language_id: String,
    /// Command used to launch the server.
    pub command: String,
    /// Command-line arguments.
    pub args: Vec<String>,
    /// Current status.
    pub status: LspStatus,
    /// OS process ID when running.
    pub pid: Option<u32>,
}

/// Tracks language-server definitions and observed lifecycle state.
///
/// This type does not spawn, signal, or monitor operating-system processes; the
/// application owns that work and records transitions here.
#[derive(Debug, Default)]
pub struct LspManager {
    processes: HashMap<String, LspProcess>,
}

impl LspManager {
    /// Create a new empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a language server definition.
    pub fn register(&mut self, process: LspProcess) {
        self.processes.insert(process.language_id.clone(), process);
    }

    /// Record that a language server is running with the given PID.
    pub fn start(&mut self, language_id: &str, pid: u32) -> anyhow::Result<()> {
        if pid == 0 {
            anyhow::bail!("language server PID must be non-zero");
        }
        let proc = self
            .processes
            .get_mut(language_id)
            .ok_or_else(|| anyhow::anyhow!("no server registered for '{language_id}'"))?;
        proc.status = LspStatus::Running;
        proc.pid = Some(pid);
        Ok(())
    }

    /// Record that a language server has stopped.
    pub fn stop(&mut self, language_id: &str) -> anyhow::Result<()> {
        let proc = self
            .processes
            .get_mut(language_id)
            .ok_or_else(|| anyhow::anyhow!("no server registered for '{language_id}'"))?;
        proc.status = LspStatus::Stopped;
        proc.pid = None;
        Ok(())
    }

    /// Get a reference to a language server by language id.
    pub fn get(&self, language_id: &str) -> Option<&LspProcess> {
        self.processes.get(language_id)
    }

    /// List all registered language servers.
    pub fn list(&self) -> Vec<&LspProcess> {
        let mut processes: Vec<_> = self.processes.values().collect();
        processes.sort_by(|a, b| a.language_id.cmp(&b.language_id));
        processes
    }

    /// Count currently running servers.
    pub fn running_count(&self) -> usize {
        self.processes
            .values()
            .filter(|p| p.status == LspStatus::Running)
            .count()
    }
}

/// A single search result entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchEntry {
    /// File path where the match was found.
    pub path: String,
    /// Line number (1-based).
    pub line: u32,
    /// Line content.
    pub content: String,
}

/// Parameters for a search query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    /// Search pattern.
    pub pattern: String,
    /// Whether matching is case-sensitive.
    pub case_sensitive: bool,
    /// Match only whole words.
    pub whole_word: bool,
    /// Treat pattern as a regular expression.
    pub regex: bool,
    /// Only search in paths matching these patterns.
    pub include_paths: Option<Vec<String>>,
    /// Skip paths matching these patterns.
    pub exclude_paths: Option<Vec<String>>,
}

/// An in-memory collection of line-level entries searched with deterministic
/// linear scans.
#[derive(Debug, Default)]
pub struct SearchIndex {
    entries: Vec<SearchEntry>,
}

struct SearchMatcher<'a> {
    query: &'a SearchQuery,
    regex: Option<Regex>,
}

impl<'a> SearchMatcher<'a> {
    fn new(query: &'a SearchQuery) -> anyhow::Result<Self> {
        const MAX_PATTERN_BYTES: usize = 64 * 1024;
        if query.pattern.is_empty() {
            anyhow::bail!("search pattern must not be empty");
        }
        if query.pattern.len() > MAX_PATTERN_BYTES {
            anyhow::bail!("search pattern exceeds {MAX_PATTERN_BYTES} bytes");
        }
        let regex = if query.regex || query.whole_word || !query.case_sensitive {
            let source = if query.regex {
                query.pattern.clone()
            } else {
                regex::escape(&query.pattern)
            };
            let pattern = if query.whole_word {
                format!(r"\b(?:{source})\b")
            } else {
                source
            };
            Some(
                RegexBuilder::new(&pattern)
                    .case_insensitive(!query.case_sensitive)
                    .size_limit(1024 * 1024)
                    .build()
                    .map_err(|error| anyhow::anyhow!("invalid search pattern: {error}"))?,
            )
        } else {
            None
        };
        Ok(Self { query, regex })
    }

    fn matches(&self, entry: &SearchEntry) -> bool {
        if let Some(includes) = &self.query.include_paths
            && !includes
                .iter()
                .any(|pattern| entry.path.contains(pattern.as_str()))
        {
            return false;
        }
        if let Some(excludes) = &self.query.exclude_paths
            && excludes
                .iter()
                .any(|pattern| entry.path.contains(pattern.as_str()))
        {
            return false;
        }

        self.regex.as_ref().map_or_else(
            || entry.content.contains(&self.query.pattern),
            |regex| regex.is_match(&entry.content),
        )
    }
}

#[derive(Clone, Copy)]
struct RankedSearchEntry<'a> {
    entry: &'a SearchEntry,
    insertion_index: usize,
}

impl PartialEq for RankedSearchEntry<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for RankedSearchEntry<'_> {}

impl PartialOrd for RankedSearchEntry<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RankedSearchEntry<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.entry
            .path
            .cmp(&other.entry.path)
            .then(self.entry.line.cmp(&other.entry.line))
            .then(self.insertion_index.cmp(&other.insertion_index))
    }
}

impl SearchIndex {
    /// Create a new empty search index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an entry to the index.
    pub fn add(&mut self, entry: SearchEntry) {
        self.entries.push(entry);
    }

    /// Search the index with the given query.
    ///
    /// Invalid, empty, or excessively large patterns return an error rather
    /// than being confused with a valid query that has no matches.
    pub fn search(&self, query: &SearchQuery) -> anyhow::Result<Vec<&SearchEntry>> {
        let matcher = SearchMatcher::new(query)?;

        let mut results: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| matcher.matches(entry))
            .collect();
        results.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
        Ok(results)
    }

    /// Search the index and return at most `max_results` entries.
    ///
    /// The result buffer remains bounded by `max_results` even when every indexed
    /// entry matches. Results use the same deterministic path/line/insertion order
    /// as [`SearchIndex::search`].
    pub fn search_with_limit(
        &self,
        query: &SearchQuery,
        max_results: usize,
    ) -> anyhow::Result<Vec<&SearchEntry>> {
        let matcher = SearchMatcher::new(query)?;
        let mut results = BinaryHeap::new();
        for (insertion_index, entry) in self.entries.iter().enumerate() {
            if !matcher.matches(entry) {
                continue;
            }
            let candidate = RankedSearchEntry {
                entry,
                insertion_index,
            };
            if results.len() < max_results {
                results.push(candidate);
            } else if results.peek().is_some_and(|largest| candidate < *largest) {
                results.pop();
                results.push(candidate);
            }
        }
        let mut results = results.into_vec();
        results.sort_unstable();
        Ok(results.into_iter().map(|ranked| ranked.entry).collect())
    }

    /// Remove all entries from the index.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Return the number of indexed entries.
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(path: &str, symbols: &[&str]) -> IndexEntry {
        IndexEntry {
            path: path.to_string(),
            symbols: symbols.iter().map(|s| s.to_string()).collect(),
            last_modified: 1000,
            size_bytes: 256,
        }
    }

    #[test]
    fn project_index_add_and_get() {
        let mut idx = ProjectIndex::new();
        idx.add(sample_entry("src/main.rs", &["main", "run"]));
        assert!(idx.get("src/main.rs").is_some());
        assert!(idx.get("missing").is_none());
    }

    #[test]
    fn project_index_remove() {
        let mut idx = ProjectIndex::new();
        idx.add(sample_entry("a.rs", &[]));
        assert!(idx.remove("a.rs"));
        assert!(!idx.remove("a.rs"));
    }

    #[test]
    fn project_index_search_by_symbol() {
        let mut idx = ProjectIndex::new();
        idx.add(sample_entry("lib.rs", &["Widget", "render"]));
        idx.add(sample_entry("util.rs", &["parse", "format"]));
        let results = idx.search_by_symbol("Widget");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "lib.rs");
    }

    #[test]
    fn project_index_search_by_path() {
        let mut idx = ProjectIndex::new();
        idx.add(sample_entry("src/lib.rs", &[]));
        idx.add(sample_entry("tests/test.rs", &[]));
        let results = idx.search_by_path("src/");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn project_index_stats() {
        let mut idx = ProjectIndex::new();
        idx.add(sample_entry("a.rs", &["x", "y"]));
        idx.add(sample_entry("b.rs", &["z"]));
        let stats = idx.stats();
        assert_eq!(stats.file_count, 2);
        assert_eq!(stats.symbol_count, 3);
        assert_eq!(stats.total_bytes, 512);
    }

    #[test]
    fn lsp_manager_register_and_start() {
        let mut mgr = LspManager::new();
        mgr.register(LspProcess {
            language_id: "rust".into(),
            command: "rust-analyzer".into(),
            args: vec![],
            status: LspStatus::Starting,
            pid: None,
        });
        assert!(mgr.start("rust", 1234).is_ok());
        assert_eq!(mgr.get("rust").unwrap().pid, Some(1234));
        assert_eq!(mgr.running_count(), 1);
    }

    #[test]
    fn lsp_manager_stop() {
        let mut mgr = LspManager::new();
        mgr.register(LspProcess {
            language_id: "ts".into(),
            command: "tsserver".into(),
            args: vec![],
            status: LspStatus::Running,
            pid: Some(99),
        });
        assert!(mgr.stop("ts").is_ok());
        assert_eq!(mgr.get("ts").unwrap().status, LspStatus::Stopped);
        assert_eq!(mgr.running_count(), 0);
    }

    #[test]
    fn lsp_manager_start_unknown() {
        let mut mgr = LspManager::new();
        assert!(mgr.start("go", 1).is_err());
        mgr.register(LspProcess {
            language_id: "rust".into(),
            command: "rust-analyzer".into(),
            args: vec![],
            status: LspStatus::Stopped,
            pid: None,
        });
        assert!(mgr.start("rust", 0).is_err());
    }

    #[test]
    fn lsp_manager_list() {
        let mut mgr = LspManager::new();
        mgr.register(LspProcess {
            language_id: "py".into(),
            command: "pyright".into(),
            args: vec![],
            status: LspStatus::Stopped,
            pid: None,
        });
        assert_eq!(mgr.list().len(), 1);
    }

    #[test]
    fn search_index_basic() {
        let mut idx = SearchIndex::new();
        idx.add(SearchEntry {
            path: "a.rs".into(),
            line: 1,
            content: "fn main() {}".into(),
        });
        idx.add(SearchEntry {
            path: "b.rs".into(),
            line: 5,
            content: "let x = 42;".into(),
        });
        let query = SearchQuery {
            pattern: "main".into(),
            case_sensitive: true,
            whole_word: false,
            regex: false,
            include_paths: None,
            exclude_paths: None,
        };
        assert_eq!(idx.search(&query).unwrap().len(), 1);
        assert_eq!(idx.count(), 2);
    }

    #[test]
    fn search_index_case_insensitive() {
        let mut idx = SearchIndex::new();
        idx.add(SearchEntry {
            path: "a.rs".into(),
            line: 1,
            content: "Hello World".into(),
        });
        let query = SearchQuery {
            pattern: "hello".into(),
            case_sensitive: false,
            whole_word: false,
            regex: false,
            include_paths: None,
            exclude_paths: None,
        };
        assert_eq!(idx.search(&query).unwrap().len(), 1);
    }

    #[test]
    fn search_index_whole_word() {
        let mut idx = SearchIndex::new();
        idx.add(SearchEntry {
            path: "a.rs".into(),
            line: 1,
            content: "format formatter".into(),
        });
        let query = SearchQuery {
            pattern: "format".into(),
            case_sensitive: true,
            whole_word: true,
            regex: false,
            include_paths: None,
            exclude_paths: None,
        };
        assert_eq!(idx.search(&query).unwrap().len(), 1);
        idx.add(SearchEntry {
            path: "b.rs".into(),
            line: 1,
            content: "format, formatter".into(),
        });
        assert_eq!(idx.search(&query).unwrap().len(), 2);
    }

    #[test]
    fn search_index_honors_regex() {
        let mut idx = SearchIndex::new();
        idx.add(SearchEntry {
            path: "a.rs".into(),
            line: 1,
            content: "let answer = 42;".into(),
        });
        idx.add(SearchEntry {
            path: "b.rs".into(),
            line: 1,
            content: "let answer = nope;".into(),
        });
        let query = SearchQuery {
            pattern: r"answer\s*=\s*\d+".into(),
            case_sensitive: true,
            whole_word: false,
            regex: true,
            include_paths: None,
            exclude_paths: None,
        };
        assert_eq!(idx.search(&query).unwrap().len(), 1);
    }

    #[test]
    fn search_index_exclude_paths() {
        let mut idx = SearchIndex::new();
        idx.add(SearchEntry {
            path: "src/a.rs".into(),
            line: 1,
            content: "match".into(),
        });
        idx.add(SearchEntry {
            path: "vendor/b.rs".into(),
            line: 1,
            content: "match".into(),
        });
        let query = SearchQuery {
            pattern: "match".into(),
            case_sensitive: true,
            whole_word: false,
            regex: false,
            include_paths: None,
            exclude_paths: Some(vec!["vendor/".into()]),
        };
        assert_eq!(idx.search(&query).unwrap().len(), 1);
    }

    #[test]
    fn search_index_clear() {
        let mut idx = SearchIndex::new();
        idx.add(SearchEntry {
            path: "x.rs".into(),
            line: 1,
            content: "data".into(),
        });
        idx.clear();
        assert_eq!(idx.count(), 0);
    }

    #[test]
    fn search_respects_max_results() {
        let mut idx = SearchIndex::new();
        for (line, path) in ["z.rs", "a.rs", "m.rs", "b.rs"].into_iter().enumerate() {
            idx.add(SearchEntry {
                path: path.into(),
                line: u32::try_from(line).unwrap(),
                content: "needle".into(),
            });
        }
        let query = SearchQuery {
            pattern: "needle".into(),
            case_sensitive: true,
            whole_word: false,
            regex: false,
            include_paths: None,
            exclude_paths: None,
        };
        let results = idx.search_with_limit(&query, 2).unwrap();
        let paths: Vec<_> = results.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, ["a.rs", "b.rs"]);
        assert!(idx.search_with_limit(&query, 0).unwrap().is_empty());
    }

    #[test]
    fn searches_are_deterministic_and_reject_empty_or_huge_patterns() {
        let mut index = SearchIndex::new();
        for path in ["z.rs", "a.rs"] {
            index.add(SearchEntry {
                path: path.into(),
                line: 1,
                content: "needle".into(),
            });
        }
        let mut query = SearchQuery {
            pattern: "needle".into(),
            case_sensitive: true,
            whole_word: false,
            regex: false,
            include_paths: None,
            exclude_paths: None,
        };
        let paths: Vec<_> = index
            .search(&query)
            .unwrap()
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        assert_eq!(paths, ["a.rs", "z.rs"]);
        query.pattern.clear();
        assert!(index.search(&query).is_err());
        query.pattern = "x".repeat(64 * 1024 + 1);
        assert!(index.search(&query).is_err());
    }
}

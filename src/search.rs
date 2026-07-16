use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use regex::Regex;
use rusqlite::{params, Connection, OpenFlags};

use crate::config::{ProductCodeMode, SourceConfig};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SearchScope {
    Current(PathBuf),
    CurrentSource(String),
    AllSources,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchGroupKind {
    Prefix,
    Contains,
    Fuzzy,
}

impl SearchGroupKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Prefix => "Starts with",
            Self::Contains => "Contains",
            Self::Fuzzy => "Similar codes",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProductSearchResult {
    pub product_id: i64,
    pub product_code: String,
    pub folder_name: String,
    pub folder_path: PathBuf,
    pub source_name: String,
    pub source_path: PathBuf,
    pub preview_path: Option<PathBuf>,
    pub image_paths: Vec<PathBuf>,
    pub image_count: usize,
    pub score: f32,
    pub group: SearchGroupKind,
}

#[derive(Clone, Debug)]
pub struct SearchGroup {
    pub kind: SearchGroupKind,
    pub results: Vec<ProductSearchResult>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchResponse {
    pub groups: Vec<SearchGroup>,
    pub elapsed: Duration,
}

impl SearchResponse {
    pub fn result_count(&self) -> usize {
        self.groups.iter().map(|group| group.results.len()).sum()
    }

    pub fn first(&self) -> Option<&ProductSearchResult> {
        self.groups.iter().find_map(|group| group.results.first())
    }

    pub fn flattened(&self) -> impl Iterator<Item = &ProductSearchResult> {
        self.groups.iter().flat_map(|group| group.results.iter())
    }
}

#[derive(Clone, Debug)]
pub struct SearchEngine {
    database_path: PathBuf,
}

#[derive(Debug)]
pub struct SearchCompletion {
    pub id: u64,
    pub query: String,
    pub result: Result<SearchResponse, String>,
}

pub struct SearchManager {
    engine: SearchEngine,
    result_tx: mpsc::Sender<SearchCompletion>,
    result_rx: mpsc::Receiver<SearchCompletion>,
    next_id: u64,
    latest_id: Option<u64>,
}

impl SearchManager {
    pub fn new(engine: SearchEngine) -> Self {
        let (result_tx, result_rx) = mpsc::channel();
        Self {
            engine,
            result_tx,
            result_rx,
            next_id: 1,
            latest_id: None,
        }
    }

    pub fn submit(&mut self, query: String, scope: SearchScope, limit: usize) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.latest_id = Some(id);
        let engine = self.engine.clone();
        let result_tx = self.result_tx.clone();
        let thread_query = query.clone();
        let spawn_result = thread::Builder::new()
            .name(format!("luxepix-search-{id}"))
            .spawn(move || {
                let result = engine.search(&thread_query, &scope, limit);
                let _ = result_tx.send(SearchCompletion {
                    id,
                    query: thread_query,
                    result,
                });
            });
        if let Err(error) = spawn_result {
            let _ = self.result_tx.send(SearchCompletion {
                id,
                query,
                result: Err(error.to_string()),
            });
        }
        id
    }

    pub fn drain_latest(&mut self, limit: usize) -> Vec<SearchCompletion> {
        let mut completions = Vec::new();
        for _ in 0..limit {
            let Ok(completion) = self.result_rx.try_recv() else {
                break;
            };
            if self.latest_id == Some(completion.id) {
                completions.push(completion);
            }
        }
        completions
    }
}

impl SearchEngine {
    pub fn new(database_path: PathBuf) -> Self {
        Self { database_path }
    }

    pub fn search(
        &self,
        query: &str,
        scope: &SearchScope,
        limit: usize,
    ) -> Result<SearchResponse, String> {
        let started = Instant::now();
        let normalized_query = normalize_code(query);
        if normalized_query.is_empty() || !self.database_path.is_file() {
            return Ok(SearchResponse {
                elapsed: started.elapsed(),
                ..SearchResponse::default()
            });
        }

        let connection = Connection::open_with_flags(
            &self.database_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| error.to_string())?;
        connection
            .busy_timeout(Duration::from_millis(120))
            .map_err(|error| error.to_string())?;
        connection
            .pragma_update(None, "case_sensitive_like", true)
            .map_err(|error| error.to_string())?;

        let limit = limit.clamp(1, 50);
        let prefix = query_products(
            &connection,
            &format!("{normalized_query}%"),
            scope,
            limit,
            SearchGroupKind::Prefix,
        )
        .map_err(|error| error.to_string())?;

        let groups = if prefix.is_empty() {
            let contains_limit = limit.div_ceil(2);
            let contains = query_products(
                &connection,
                &format!("%{normalized_query}%"),
                scope,
                contains_limit,
                SearchGroupKind::Contains,
            )
            .map_err(|error| error.to_string())?;
            let excluded: HashSet<i64> = contains.iter().map(|result| result.product_id).collect();
            let fuzzy = query_fuzzy_products(
                &connection,
                &normalized_query,
                scope,
                limit.saturating_sub(contains.len()),
                &excluded,
            )
            .map_err(|error| error.to_string())?;
            let mut groups = Vec::new();
            if !contains.is_empty() {
                groups.push(SearchGroup {
                    kind: SearchGroupKind::Contains,
                    results: contains,
                });
            }
            if !fuzzy.is_empty() {
                groups.push(SearchGroup {
                    kind: SearchGroupKind::Fuzzy,
                    results: fuzzy,
                });
            }
            groups
        } else {
            vec![SearchGroup {
                kind: SearchGroupKind::Prefix,
                results: prefix,
            }]
        };

        Ok(SearchResponse {
            groups,
            elapsed: started.elapsed(),
        })
    }
}

pub fn extract_product_code(source: &SourceConfig, folder_name: &str) -> String {
    let fallback = folder_name.trim();
    let extracted = match source.code_mode {
        ProductCodeMode::FolderName => None,
        ProductCodeMode::Prefix => extract_after_prefix(folder_name, &source.code_pattern),
        ProductCodeMode::Regex => extract_with_regex(folder_name, &source.code_pattern),
    };
    extracted
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

pub fn validate_extraction_rule(source: &SourceConfig) -> Result<(), String> {
    match source.code_mode {
        ProductCodeMode::FolderName => Ok(()),
        ProductCodeMode::Prefix if source.code_pattern.trim().is_empty() => {
            Err("Enter the fixed prefix to remove from matching folder names.".into())
        }
        ProductCodeMode::Prefix => Ok(()),
        ProductCodeMode::Regex if source.code_pattern.trim().is_empty() => {
            Err("Enter a regular expression.".into())
        }
        ProductCodeMode::Regex => Regex::new(source.code_pattern.trim())
            .map(|_| ())
            .map_err(|error| error.to_string()),
    }
}

pub fn normalize_code(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_uppercase)
        .collect()
}

pub fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn extract_after_prefix(folder_name: &str, prefix: &str) -> Option<String> {
    let prefix = prefix.trim();
    if prefix.is_empty() {
        return None;
    }
    let name = folder_name.trim();
    if name.len() < prefix.len()
        || !name
            .get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
    {
        return None;
    }

    let remainder = name[prefix.len()..].trim_start_matches(|character: char| {
        character.is_whitespace() || matches!(character, '-' | '_' | ':' | '#')
    });
    let code: String = remainder
        .chars()
        .take_while(|character| {
            !character.is_whitespace() && !matches!(character, ':' | '/' | '\\' | '(' | '[')
        })
        .collect();
    (!code.is_empty()).then_some(code)
}

fn extract_with_regex(folder_name: &str, pattern: &str) -> Option<String> {
    let regex = Regex::new(pattern.trim()).ok()?;
    let captures = regex.captures(folder_name)?;
    for index in 1..captures.len() {
        if let Some(value) = captures.get(index) {
            if !value.as_str().trim().is_empty() {
                return Some(value.as_str().trim().to_string());
            }
        }
    }
    captures
        .get(0)
        .map(|value| value.as_str().trim().to_string())
}

#[derive(Debug)]
struct ProductRow {
    product_id: i64,
    product_code: String,
    normalized_code: String,
    folder_name: String,
    folder_path: String,
    source_name: String,
    source_path: String,
    preview_path: Option<String>,
    image_count: i64,
}

fn query_products(
    connection: &Connection,
    pattern: &str,
    scope: &SearchScope,
    limit: usize,
    group: SearchGroupKind,
) -> rusqlite::Result<Vec<ProductSearchResult>> {
    let (current_path, current_descendants, source_path) = scope_parameters(scope);
    let mut statement = connection.prepare_cached(
        "SELECT
             p.id,
             p.product_code,
             p.normalized_code,
             p.folder_name,
             p.folder_path,
             s.name,
             s.path,
             p.preview_path,
             p.image_count
         FROM products p
         JOIN sources s ON s.id = p.source_id
         WHERE p.normalized_code LIKE ?1
           AND (?2 = '' OR p.normalized_folder_path = ?2 OR p.normalized_folder_path LIKE ?3 ESCAPE '!')
           AND (?4 = '' OR s.normalized_path = ?4)
         ORDER BY length(p.normalized_code), p.normalized_code, p.folder_name
         LIMIT ?5",
    )?;
    let rows = statement.query_map(
        params![
            pattern,
            current_path,
            current_descendants,
            source_path,
            limit as i64
        ],
        map_product_row,
    )?;

    rows.map(|row| {
        let row = row?;
        hydrate_result(connection, row, group, 1.0)
    })
    .collect()
}

fn query_fuzzy_products(
    connection: &Connection,
    query: &str,
    scope: &SearchScope,
    limit: usize,
    excluded: &HashSet<i64>,
) -> rusqlite::Result<Vec<ProductSearchResult>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let (current_path, current_descendants, source_path) = scope_parameters(scope);
    let first_character = query.chars().next().map(String::from).unwrap_or_default();
    let minimum_length = query.len().saturating_sub(5) as i64;
    let maximum_length = query.len().saturating_add(8) as i64;
    let mut statement = connection.prepare_cached(
        "SELECT
             p.id,
             p.product_code,
             p.normalized_code,
             p.folder_name,
             p.folder_path,
             s.name,
             s.path,
             p.preview_path,
             p.image_count
         FROM products p
         JOIN sources s ON s.id = p.source_id
         WHERE (?1 = '' OR substr(p.normalized_code, 1, 1) = ?1)
           AND length(p.normalized_code) BETWEEN ?2 AND ?3
           AND (?4 = '' OR p.normalized_folder_path = ?4 OR p.normalized_folder_path LIKE ?5 ESCAPE '!')
           AND (?6 = '' OR s.normalized_path = ?6)
         ORDER BY abs(length(p.normalized_code) - ?7), p.normalized_code
         LIMIT 5000",
    )?;
    let rows = statement.query_map(
        params![
            first_character,
            minimum_length,
            maximum_length,
            current_path,
            current_descendants,
            source_path,
            query.len() as i64
        ],
        map_product_row,
    )?;

    let mut scored = Vec::new();
    for row in rows {
        let row = row?;
        if excluded.contains(&row.product_id) {
            continue;
        }
        let score = strsim::normalized_levenshtein(query, &row.normalized_code) as f32;
        if score >= 0.42 {
            scored.push((score, row));
        }
    }
    scored.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| left.normalized_code.cmp(&right.normalized_code))
    });
    scored.truncate(limit);

    scored
        .into_iter()
        .map(|(score, row)| hydrate_result(connection, row, SearchGroupKind::Fuzzy, score))
        .collect()
}

fn scope_parameters(scope: &SearchScope) -> (String, String, String) {
    match scope {
        SearchScope::Current(path) => {
            let key = normalize_path_key(path);
            let descendants = if key.is_empty() {
                String::new()
            } else {
                format!("{}/%", escape_like(&key))
            };
            (key, descendants, String::new())
        }
        SearchScope::CurrentSource(path) => (
            String::new(),
            String::new(),
            normalize_path_key(Path::new(path)),
        ),
        SearchScope::AllSources => (String::new(), String::new(), String::new()),
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('!', "!!")
        .replace('%', "!%")
        .replace('_', "!_")
}

fn map_product_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProductRow> {
    Ok(ProductRow {
        product_id: row.get(0)?,
        product_code: row.get(1)?,
        normalized_code: row.get(2)?,
        folder_name: row.get(3)?,
        folder_path: row.get(4)?,
        source_name: row.get(5)?,
        source_path: row.get(6)?,
        preview_path: row.get(7)?,
        image_count: row.get(8)?,
    })
}

fn hydrate_result(
    connection: &Connection,
    row: ProductRow,
    group: SearchGroupKind,
    score: f32,
) -> rusqlite::Result<ProductSearchResult> {
    let mut image_statement = connection.prepare_cached(
        "SELECT path
         FROM images
         WHERE product_id = ?1
         ORDER BY is_preview DESC, path
         LIMIT 6",
    )?;
    let image_paths = image_statement
        .query_map([row.product_id], |image_row| image_row.get::<_, String>(0))?
        .map(|path| path.map(PathBuf::from))
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(ProductSearchResult {
        product_id: row.product_id,
        product_code: row.product_code,
        folder_name: row.folder_name,
        folder_path: PathBuf::from(row.folder_path),
        source_name: row.source_name,
        source_path: PathBuf::from(row.source_path),
        preview_path: row.preview_path.map(PathBuf::from),
        image_paths,
        image_count: row.image_count.max(0) as usize,
        score,
        group,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        extract_product_code, normalize_code, normalize_path_key, validate_extraction_rule,
    };
    use crate::config::{ProductCodeMode, SourceConfig};
    use std::path::Path;

    #[test]
    fn normalizes_product_codes_for_fast_case_insensitive_lookup() {
        assert_eq!(normalize_code(" sku-12 34_a "), "SKU1234A");
    }

    #[test]
    fn prefix_rule_extracts_the_first_code_token() {
        let source = SourceConfig {
            code_mode: ProductCodeMode::Prefix,
            code_pattern: "SKU".into(),
            ..SourceConfig::default()
        };

        assert_eq!(
            extract_product_code(&source, "sku- 1042 Blue Chrome"),
            "1042"
        );
        assert_eq!(
            extract_product_code(&source, "1043 Folder Fallback"),
            "1043 Folder Fallback"
        );
    }

    #[test]
    fn regex_rule_prefers_the_first_capture_group() {
        let source = SourceConfig {
            code_mode: ProductCodeMode::Regex,
            code_pattern: r"(?i)^product[-_ ]+([a-z0-9-]+)".into(),
            ..SourceConfig::default()
        };

        assert_eq!(
            extract_product_code(&source, "Product-AB-1042 Holiday"),
            "AB-1042"
        );
    }

    #[test]
    fn invalid_regex_is_rejected_before_reindexing() {
        let source = SourceConfig {
            code_mode: ProductCodeMode::Regex,
            code_pattern: "(".into(),
            ..SourceConfig::default()
        };

        assert!(validate_extraction_rule(&source).is_err());
    }

    #[test]
    fn path_keys_are_case_and_separator_insensitive() {
        assert_eq!(
            normalize_path_key(Path::new(r"N:\Products\1001\\")),
            "n:/products/1001"
        );
    }
}

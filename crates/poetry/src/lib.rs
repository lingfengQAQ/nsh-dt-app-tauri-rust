use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub use nsh_core as core;

#[derive(Debug, thiserror::Error)]
pub enum PoetryError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, PoetryError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Poem {
    pub id: i64,
    pub title: String,
    pub author: Option<String>,
    pub dynasty: Option<String>,
    pub paragraphs: Vec<String>,
    pub source: Option<String>,
}

impl Poem {
    pub fn text(&self) -> String {
        self.paragraphs.join("\n")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaInfo {
    pub tables: Vec<TableInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableInfo {
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    pub sql: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub not_null: bool,
    pub primary_key: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexedPoem {
    pub id: i64,
    pub title: String,
    pub author: Option<String>,
    pub dynasty: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexedClause {
    pub id: i64,
    pub poem_id: i64,
    pub position: i64,
    pub text: String,
    pub normalized_text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharIndexEntry {
    pub ch: char,
    pub clause_id: i64,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharFrequency {
    pub ch: char,
    pub document_count: u32,
    pub total_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    pub limit: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self { limit: 20 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PoemMatch {
    pub poem: Poem,
    pub score: f32,
    pub matched_chars: usize,
    pub matched_clause: Option<String>,
}

pub struct PoetryLibrary {
    connection: Connection,
    clause_index_connection: Option<Connection>,
    index_connection: Option<Connection>,
}

impl PoetryLibrary {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let connection = Connection::open(path)?;
        configure_read_connection(&connection)?;
        Ok(Self {
            connection,
            clause_index_connection: open_clause_index(path)?,
            index_connection: open_legacy_index(path)?,
        })
    }

    pub fn from_connection(connection: Connection) -> Self {
        Self {
            connection,
            clause_index_connection: None,
            index_connection: None,
        }
    }

    pub fn inspect_schema(&self) -> Result<SchemaInfo> {
        inspect_schema(&self.connection)
    }

    pub fn load(&self, id: i64) -> Result<Option<Poem>> {
        load_poem(&self.connection, id)
    }

    pub fn search(&self, query: &str, options: SearchOptions) -> Result<Vec<Poem>> {
        search_poems(&self.connection, query, options)
    }

    pub fn find_poem_from_chars(&self, chars: &str, limit: usize) -> Result<Vec<PoemMatch>> {
        if let Some(clause_index_connection) = &self.clause_index_connection {
            let indexed = find_poem_from_chars_clause_indexed(
                &self.connection,
                clause_index_connection,
                chars,
                limit,
            )?;
            if !indexed.is_empty() {
                return Ok(indexed);
            }
        }

        if let Some(index_connection) = &self.index_connection {
            let indexed =
                find_poem_from_chars_indexed(&self.connection, index_connection, chars, limit)?;
            if !indexed.is_empty() {
                return Ok(indexed);
            }
        }

        find_poem_from_chars(&self.connection, chars, limit)
    }
}

const STOP_WORDS: &[&str] = &[
    "\u{786e}\u{5b9a}",
    "\u{53d6}\u{6d88}",
    "\u{786e}\u{8ba4}",
    "\u{8fd4}\u{56de}",
    "\u{5b8c}\u{6210}",
    "\u{63d0}\u{4ea4}",
    "\u{5173}\u{95ed}",
    "\u{7ee7}\u{7eed}",
    "\u{91cd}\u{8bd5}",
    "\u{4e0b}\u{4e00}\u{9898}",
    "\u{4e0a}\u{4e00}\u{9898}",
    "\u{91cd}\u{7f6e}",
    "\u{9009}\u{62e9}",
];

const POEM_TRIGGER_PHRASES: &[&str] = &[
    "\u{8bf7}\u{4ece}\u{4ee5}\u{4e0b}\u{5b57}\u{4e2d}\u{9009}\u{51fa}\u{4e00}\u{53e5}\u{8bd7}\u{8bcd}",
    "\u{8bf7}\u{4ece}\u{4e0b}\u{5217}\u{5b57}\u{4e2d}\u{9009}\u{51fa}\u{4e00}\u{53e5}\u{8bd7}\u{8bcd}",
    "\u{4ece}\u{4ee5}\u{4e0b}\u{5b57}\u{4e2d}\u{9009}\u{51fa}\u{4e00}\u{53e5}\u{8bd7}\u{8bcd}",
    "\u{4ece}\u{4e0b}\u{5217}\u{5b57}\u{4e2d}\u{9009}\u{51fa}\u{4e00}\u{53e5}\u{8bd7}\u{8bcd}",
    "\u{7528}\u{8fd9}\u{4e9b}\u{5b57}\u{7ec4}\u{6210}\u{4e00}\u{53e5}\u{8bd7}",
    "\u{7528}\u{4e0b}\u{9762}\u{7684}\u{5b57}\u{7ec4}\u{6210}\u{8bd7}\u{53e5}",
    "\u{8fd9}\u{4e9b}\u{5b57}\u{80fd}\u{7ec4}\u{6210}\u{4ec0}\u{4e48}\u{8bd7}\u{53e5}",
];

pub fn detect_poem_task(text: &str) -> bool {
    let formatted = format_poem_question_text(text);
    detect_poem_task_in_formatted(&formatted)
}

pub fn clean_poem_chars(text: &str) -> Option<String> {
    let formatted = format_poem_question_text(text);
    if !detect_poem_task_in_formatted(&formatted) {
        return None;
    }

    let extracted = extract_poem_chars_from_formatted(&formatted);
    let chars = normalize_poem_chars(&extracted);
    (!chars.is_empty()).then_some(chars)
}

pub fn format_poem_question_text(text: &str) -> String {
    let mut lines: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    if lines.is_empty() {
        return String::new();
    }

    while lines
        .last()
        .is_some_and(|line| is_stop_word(line) || is_alphanumeric_only(line))
    {
        lines.pop();
    }

    if lines.is_empty() {
        return String::new();
    }

    let single_char_count = lines
        .iter()
        .filter(|line| line.chars().count() <= 2)
        .count();
    let single_char_ratio = single_char_count as f32 / lines.len() as f32;
    if single_char_ratio >= 0.6 {
        return lines.join("");
    }

    merge_short_poem_lines(&lines)
}

fn detect_poem_task_in_formatted(text: &str) -> bool {
    POEM_TRIGGER_PHRASES
        .iter()
        .any(|phrase| text.contains(phrase))
        || (text.contains('\u{8bd7}')
            && (text.contains("\u{9009}\u{51fa}") || text.contains("\u{7ec4}\u{6210}"))
            && (text.contains("\u{4ee5}\u{4e0b}\u{5b57}")
                || text.contains("\u{4e0b}\u{5217}\u{5b57}")
                || text.contains("\u{8fd9}\u{4e9b}\u{5b57}")
                || text.contains("\u{4e0b}\u{9762}\u{7684}\u{5b57}")))
}

fn extract_poem_chars_from_formatted(text: &str) -> String {
    let mut result = text.trim();
    if let Some(index) = result.find('\u{8bd7}') {
        let start = index + '\u{8bd7}'.len_utf8();
        result = &result[start..];
        result = trim_leading_poem_suffix(result);
    }

    let result = remove_question_prefix(result);
    remove_noise_after_stop_word(result).trim().to_string()
}

fn trim_leading_poem_suffix(mut text: &str) -> &str {
    loop {
        let trimmed =
            text.trim_start_matches(|ch: char| ch.is_whitespace() || is_cjk_punctuation(ch));
        let Some(first) = trimmed.chars().next() else {
            return trimmed;
        };
        if matches!(
            first,
            '\u{8bcd}' | '\u{53e5}' | '\u{6587}' | '\u{ff1a}' | ':' | '\u{ff0c}' | ',' | '\u{3002}'
        ) {
            text = &trimmed[first.len_utf8()..];
        } else {
            return trimmed;
        }
    }
}

fn remove_question_prefix(text: &str) -> &str {
    let mut result = text.trim_start();

    let digit_len = result
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_digit())
        .map(|(index, ch)| index + ch.len_utf8())
        .last()
        .unwrap_or(0);
    if digit_len > 0 {
        result = result[digit_len..].trim_start();
        if let Some(stripped) = result.strip_prefix('\u{5206}') {
            result = stripped.trim_start();
        }
    }

    if let Some(stripped) = result.strip_prefix('\u{7b2c}') {
        let digit_len = stripped
            .char_indices()
            .take_while(|(_, ch)| ch.is_ascii_digit())
            .map(|(index, ch)| index + ch.len_utf8())
            .last()
            .unwrap_or(0);
        if digit_len > 0 {
            let after_digits = stripped[digit_len..].trim_start();
            if let Some(first) = after_digits
                .chars()
                .next()
                .filter(|ch| matches!(ch, '\u{9898}' | '.' | '\u{3001}'))
            {
                result = after_digits[first.len_utf8()..].trim_start();
            }
        }
    }

    result
}

fn remove_noise_after_stop_word(text: &str) -> &str {
    STOP_WORDS
        .iter()
        .filter_map(|word| text.find(word))
        .min()
        .map_or(text, |index| &text[..index])
}

fn merge_short_poem_lines(lines: &[String]) -> String {
    let mut merged = Vec::new();
    let mut buffer = Vec::new();

    for line in lines {
        if line.chars().count() <= 2 && !line.chars().any(is_cjk_punctuation) {
            buffer.push(line.as_str());
        } else {
            if !buffer.is_empty() {
                merged.push(buffer.join(""));
                buffer.clear();
            }
            merged.push(line.clone());
        }
    }

    if !buffer.is_empty() {
        merged.push(buffer.join(""));
    }

    merged.join("\n")
}

fn normalize_poem_chars(text: &str) -> String {
    normalize_text(text)
        .chars()
        .filter(|ch| is_cjk_letter(*ch))
        .collect()
}

fn is_stop_word(text: &str) -> bool {
    STOP_WORDS.iter().any(|word| text == *word)
}

fn is_alphanumeric_only(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|ch| ch.is_ascii_alphanumeric())
}

fn is_cjk_letter(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x2a6df | 0x2a700..=0x2b73f | 0x2b740..=0x2b81f | 0x2b820..=0x2ceaf
    )
}
pub fn inspect_schema(connection: &Connection) -> Result<SchemaInfo> {
    let mut statement = connection.prepare(
        "SELECT name, sql FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;

    let mut tables = Vec::new();
    for row in rows {
        let (name, sql) = row?;
        let columns = table_columns(connection, &name)?;
        tables.push(TableInfo { name, columns, sql });
    }

    Ok(SchemaInfo { tables })
}

pub fn load_poem(connection: &Connection, id: i64) -> Result<Option<Poem>> {
    let columns = poem_columns(connection)?;
    let mut select_columns = vec!["id".to_string()];
    for column in [
        "title",
        "author",
        "dynasty",
        "source",
        "content",
        "paragraphs",
    ] {
        if columns.contains(column) {
            select_columns.push(column.to_string());
        } else {
            select_columns.push(format!("NULL AS {column}"));
        }
    }

    let sql = format!(
        "SELECT {} FROM poems WHERE id = ?1 LIMIT 1",
        select_columns.join(", ")
    );
    connection
        .query_row(&sql, [id], poem_from_row)
        .optional()
        .map_err(Into::into)
}

pub fn search_poems(
    connection: &Connection,
    query: &str,
    options: SearchOptions,
) -> Result<Vec<Poem>> {
    let normalized = normalize_text(query);
    if normalized.is_empty() {
        return Ok(Vec::new());
    }

    let columns = poem_columns(connection)?;
    let mut predicates = Vec::new();
    for column in ["title", "author", "dynasty", "content", "paragraphs"] {
        if columns.contains(column) {
            predicates.push(format!("{column} LIKE ?1"));
        }
    }

    if predicates.is_empty() {
        return Ok(Vec::new());
    }

    let sql = format!(
        "SELECT id FROM poems WHERE {} ORDER BY id LIMIT ?2",
        predicates.join(" OR ")
    );
    let pattern = format!("%{query}%");
    let mut statement = connection.prepare(&sql)?;
    let ids = statement
        .query_map((&pattern, options.limit as i64), |row| row.get::<_, i64>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut poems = Vec::new();
    for id in ids {
        if let Some(poem) = load_poem(connection, id)? {
            if normalize_text(&poem.title).contains(&normalized)
                || poem
                    .author
                    .as_deref()
                    .map(normalize_text)
                    .is_some_and(|author| author.contains(&normalized))
                || normalize_text(&poem.text()).contains(&normalized)
            {
                poems.push(poem);
            }
        }
    }
    Ok(poems)
}

pub fn find_poem_from_chars(
    connection: &Connection,
    chars: &str,
    limit: usize,
) -> Result<Vec<PoemMatch>> {
    let normalized_chars = normalize_text(chars);
    let query_counts = char_counts(&normalized_chars);
    if query_counts.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let mut statement = connection.prepare("SELECT id FROM poems ORDER BY id")?;
    let ids = statement
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut matches = Vec::new();
    for id in ids {
        let Some(poem) = load_poem(connection, id)? else {
            continue;
        };
        let Some(best_clause) = best_clause_match(&poem, &query_counts) else {
            continue;
        };
        matches.push(PoemMatch {
            poem,
            score: clause_score(best_clause.chars().count(), query_counts.values().sum()),
            matched_chars: best_clause.chars().count(),
            matched_clause: Some(best_clause),
        });
    }

    sort_and_trim_matches(&mut matches, limit);
    Ok(matches)
}

fn find_poem_from_chars_indexed(
    connection: &Connection,
    index_connection: &Connection,
    chars: &str,
    limit: usize,
) -> Result<Vec<PoemMatch>> {
    let normalized_chars = normalize_text(chars);
    let query_counts = char_counts(&normalized_chars);
    if query_counts.is_empty() || limit == 0 || !has_legacy_index_schema(index_connection)? {
        return Ok(Vec::new());
    }

    let mut char_frequencies =
        indexed_char_frequencies(index_connection, query_counts.keys().copied())?;
    if char_frequencies.is_empty() {
        return Ok(Vec::new());
    }
    char_frequencies.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

    let total_query_chars = query_counts.values().sum();
    let search_chars = char_frequencies
        .into_iter()
        .take(8)
        .map(|(_, ch)| ch)
        .collect::<Vec<_>>();

    let mut candidates = indexed_candidates_for_chars(
        index_connection,
        &search_chars[..search_chars.len().min(4)],
        &query_counts,
        20_000,
    )?;
    if candidates.is_empty() && search_chars.len() > 4 {
        candidates =
            indexed_candidates_for_chars(index_connection, &search_chars, &query_counts, 40_000)?;
    }

    candidates.sort_by(|left, right| {
        let left_len = left.matched_clause.chars().count();
        let right_len = right.matched_clause.chars().count();
        right_len
            .cmp(&left_len)
            .then_with(|| right.hit_count.cmp(&left.hit_count))
            .then_with(|| left.poem_idx.cmp(&right.poem_idx))
    });

    let mut matches = Vec::new();
    let mut seen_clauses = HashSet::new();
    for candidate in candidates {
        if !seen_clauses.insert(candidate.matched_clause.clone()) {
            continue;
        }
        let poem_id = candidate.poem_idx + 1;
        let Some(poem) = load_poem(connection, poem_id)? else {
            continue;
        };
        let matched_chars = candidate.matched_clause.chars().count();
        matches.push(PoemMatch {
            poem,
            score: clause_score(matched_chars, total_query_chars),
            matched_chars,
            matched_clause: Some(candidate.matched_clause),
        });
        if matches.len() >= limit.saturating_mul(3).max(limit) {
            break;
        }
    }

    sort_and_trim_matches(&mut matches, limit);
    Ok(matches)
}

fn find_poem_from_chars_clause_indexed(
    connection: &Connection,
    clause_index_connection: &Connection,
    chars: &str,
    limit: usize,
) -> Result<Vec<PoemMatch>> {
    let normalized_chars = normalize_poem_chars(chars);
    let query_counts = char_counts(&normalized_chars);
    if query_counts.is_empty() || limit == 0 || !has_clause_index_schema(clause_index_connection)? {
        return Ok(Vec::new());
    }

    let candidate_chars = normalized_chars
        .chars()
        .take(MAX_CLAUSE_INDEX_QUERY_CHARS)
        .collect::<Vec<_>>();
    if candidate_chars.len() < 5 {
        return Ok(Vec::new());
    }

    let total_query_chars = query_counts.values().sum();
    let mut matches = Vec::new();
    let mut seen_clauses = HashSet::new();
    let target_matches = limit.saturating_mul(3).max(limit);

    for clause_len in [7usize, 5] {
        if candidate_chars.len() < clause_len {
            continue;
        }

        let keys = clause_subset_keys(&candidate_chars, clause_len);
        let candidates = clause_index_candidates_for_keys(
            clause_index_connection,
            &keys,
            clause_len,
            target_matches.saturating_mul(80).max(500),
        )?;

        for candidate in candidates {
            if !seen_clauses.insert(candidate.matched_clause.clone()) {
                continue;
            }
            if !is_subset_counts(&char_counts(&candidate.matched_clause), &query_counts) {
                continue;
            }
            let Some(poem) = load_poem(connection, candidate.poem_id)? else {
                continue;
            };
            matches.push(PoemMatch {
                poem,
                score: clause_score(candidate.clause_len, total_query_chars),
                matched_chars: candidate.clause_len,
                matched_clause: Some(candidate.matched_clause),
            });
            if matches.len() >= target_matches {
                break;
            }
        }

        if matches.len() >= limit {
            break;
        }
    }

    sort_and_trim_matches(&mut matches, limit);
    Ok(matches)
}

const MAX_CLAUSE_INDEX_QUERY_CHARS: usize = 16;
const CLAUSE_INDEX_BATCH_SIZE: usize = 900;

#[derive(Debug)]
struct ClauseIndexMatchCandidate {
    poem_id: i64,
    matched_clause: String,
    clause_len: usize,
}

fn clause_subset_keys(chars: &[char], clause_len: usize) -> Vec<String> {
    let mut sorted_chars = chars.to_vec();
    sorted_chars.sort_unstable();

    let mut keys = Vec::new();
    let mut current = Vec::with_capacity(clause_len);
    collect_clause_subset_keys(&sorted_chars, clause_len, 0, &mut current, &mut keys);
    keys
}

fn collect_clause_subset_keys(
    chars: &[char],
    clause_len: usize,
    start: usize,
    current: &mut Vec<char>,
    keys: &mut Vec<String>,
) {
    if current.len() == clause_len {
        keys.push(current.iter().collect());
        return;
    }

    let remaining = clause_len - current.len();
    if chars.len().saturating_sub(start) < remaining {
        return;
    }

    let max_start = chars.len() - remaining;
    let mut previous = None;
    for index in start..=max_start {
        let ch = chars[index];
        if previous == Some(ch) {
            continue;
        }
        previous = Some(ch);
        current.push(ch);
        collect_clause_subset_keys(chars, clause_len, index + 1, current, keys);
        current.pop();
    }
}

fn clause_index_candidates_for_keys(
    connection: &Connection,
    keys: &[String],
    clause_len: usize,
    limit: usize,
) -> Result<Vec<ClauseIndexMatchCandidate>> {
    if keys.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let mut candidates = Vec::new();
    for chunk in keys.chunks(CLAUSE_INDEX_BATCH_SIZE) {
        let remaining = limit.saturating_sub(candidates.len());
        if remaining == 0 {
            break;
        }

        let placeholders = std::iter::repeat("?")
            .take(chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT poem_id, clause FROM clause_key_index WHERE len = ? AND key IN ({placeholders}) ORDER BY poem_id LIMIT ?"
        );
        let mut params = Vec::with_capacity(chunk.len() + 2);
        params.push(rusqlite::types::Value::Integer(clause_len as i64));
        params.extend(
            chunk
                .iter()
                .map(|key| rusqlite::types::Value::Text(key.clone())),
        );
        params.push(rusqlite::types::Value::Integer(remaining as i64));

        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(params), |row| {
            Ok(ClauseIndexMatchCandidate {
                poem_id: row.get(0)?,
                matched_clause: row.get(1)?,
                clause_len,
            })
        })?;
        for row in rows {
            candidates.push(row?);
        }
    }

    Ok(candidates)
}

#[derive(Debug)]
struct IndexedMatchCandidate {
    poem_idx: i64,
    matched_clause: String,
    hit_count: usize,
}

fn indexed_candidates_for_chars(
    connection: &Connection,
    search_chars: &[char],
    query_counts: &HashMap<char, usize>,
    limit: usize,
) -> Result<Vec<IndexedMatchCandidate>> {
    if search_chars.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = std::iter::repeat("?")
        .take(search_chars.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"SELECT poem_idx, normalized_clause, COUNT(DISTINCT "char") AS hit_count
           FROM char_index
           WHERE "char" IN ({placeholders})
           GROUP BY poem_idx, normalized_clause
           ORDER BY hit_count DESC
           LIMIT ?"#
    );
    let mut params = search_chars
        .iter()
        .map(|ch| rusqlite::types::Value::Text(ch.to_string()))
        .collect::<Vec<_>>();
    params.push(rusqlite::types::Value::Integer(limit as i64));

    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(params), |row| {
        Ok(IndexedMatchCandidate {
            poem_idx: row.get(0)?,
            matched_clause: row.get(1)?,
            hit_count: row.get::<_, i64>(2)? as usize,
        })
    })?;

    let mut candidates = Vec::new();
    for row in rows {
        let candidate = row?;
        let clause_len = candidate.matched_clause.chars().count();
        if !matches_poem_clause_len(clause_len) {
            continue;
        }
        if !is_subset_counts(&char_counts(&candidate.matched_clause), query_counts) {
            continue;
        }
        candidates.push(candidate);
    }
    Ok(candidates)
}

pub fn normalize_text(text: &str) -> String {
    text.replace("&nbsp;", "")
        .replace("&nbsp", "")
        .chars()
        .filter_map(|ch| match ch {
            '\u{3000}' | '\u{00a0}' => None,
            _ if ch.is_whitespace() || ch.is_ascii_punctuation() => None,
            _ if is_cjk_punctuation(ch) => None,
            _ => Some(ch),
        })
        .collect()
}

pub fn char_counts(text: &str) -> HashMap<char, usize> {
    let mut counts = HashMap::new();
    for ch in text.chars() {
        *counts.entry(ch).or_insert(0) += 1;
    }
    counts
}

pub fn count_intersection(left: &HashMap<char, usize>, right: &HashMap<char, usize>) -> usize {
    left.iter()
        .map(|(ch, left_count)| right.get(ch).copied().unwrap_or(0).min(*left_count))
        .sum()
}

fn open_clause_index(poetry_db_path: &Path) -> Result<Option<Connection>> {
    let Some(index_path) = clause_index_path(poetry_db_path) else {
        return Ok(None);
    };
    if !index_path.exists() {
        return Ok(None);
    }

    let connection = Connection::open(index_path)?;
    configure_read_connection(&connection)?;
    if has_clause_index_schema(&connection)? {
        Ok(Some(connection))
    } else {
        Ok(None)
    }
}

fn clause_index_path(poetry_db_path: &Path) -> Option<PathBuf> {
    poetry_db_path
        .parent()
        .map(|parent| parent.join("poetry_clause_index.db"))
}

fn open_legacy_index(poetry_db_path: &Path) -> Result<Option<Connection>> {
    let Some(index_path) = legacy_index_path(poetry_db_path) else {
        return Ok(None);
    };
    if !index_path.exists() {
        return Ok(None);
    }

    let connection = Connection::open(index_path)?;
    configure_read_connection(&connection)?;
    if has_legacy_index_schema(&connection)? {
        Ok(Some(connection))
    } else {
        Ok(None)
    }
}

fn legacy_index_path(poetry_db_path: &Path) -> Option<PathBuf> {
    poetry_db_path
        .parent()
        .map(|parent| parent.join("poetry_index.db"))
}

fn configure_read_connection(connection: &Connection) -> Result<()> {
    connection.pragma_update(None, "cache_size", -131_072i64)?;
    connection.pragma_update(None, "mmap_size", 268_435_456i64)?;
    Ok(())
}

fn has_legacy_index_schema(connection: &Connection) -> Result<bool> {
    let exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'char_index' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Ok(false);
    }

    let columns = table_columns(connection, "char_index")?
        .into_iter()
        .map(|column| column.name)
        .collect::<HashSet<_>>();
    Ok(columns.contains("char")
        && columns.contains("poem_idx")
        && columns.contains("normalized_clause"))
}

fn has_clause_index_schema(connection: &Connection) -> Result<bool> {
    let exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'clause_key_index' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Ok(false);
    }

    let columns = table_columns(connection, "clause_key_index")?
        .into_iter()
        .map(|column| column.name)
        .collect::<HashSet<_>>();
    Ok(columns.contains("key")
        && columns.contains("len")
        && columns.contains("poem_id")
        && columns.contains("clause"))
}

fn indexed_char_frequencies(
    connection: &Connection,
    chars: impl Iterator<Item = char>,
) -> Result<Vec<(i64, char)>> {
    let mut frequencies = Vec::new();
    let mut statement =
        connection.prepare(r#"SELECT COUNT(*) FROM char_index WHERE "char" = ?1"#)?;
    for ch in chars {
        let count = statement.query_row([ch.to_string()], |row| row.get::<_, i64>(0))?;
        if count > 0 {
            frequencies.push((count, ch));
        }
    }
    Ok(frequencies)
}

fn best_clause_match(poem: &Poem, query_counts: &HashMap<char, usize>) -> Option<String> {
    poem.paragraphs
        .iter()
        .flat_map(|paragraph| split_poem_clauses(paragraph))
        .filter(|clause| {
            let len = clause.chars().count();
            matches_poem_clause_len(len) && is_subset_counts(&char_counts(clause), query_counts)
        })
        .max_by(|left, right| {
            left.chars()
                .count()
                .cmp(&right.chars().count())
                .then_with(|| right.cmp(left))
        })
}

fn split_poem_clauses(text: &str) -> Vec<String> {
    let mut clauses = Vec::new();
    let mut buffer = String::new();
    for ch in text.chars() {
        if ch.is_whitespace() || ch.is_ascii_punctuation() || is_cjk_punctuation(ch) {
            push_normalized_clause(&mut clauses, &mut buffer);
        } else {
            buffer.push(ch);
        }
    }
    push_normalized_clause(&mut clauses, &mut buffer);
    clauses
}

fn push_normalized_clause(clauses: &mut Vec<String>, buffer: &mut String) {
    if buffer.is_empty() {
        return;
    }
    let normalized = normalize_text(buffer);
    if !normalized.is_empty() {
        clauses.push(normalized);
    }
    buffer.clear();
}

fn matches_poem_clause_len(len: usize) -> bool {
    len == 5 || len == 7
}

fn is_subset_counts(needle: &HashMap<char, usize>, haystack: &HashMap<char, usize>) -> bool {
    needle
        .iter()
        .all(|(ch, count)| haystack.get(ch).copied().unwrap_or(0) >= *count)
}

fn clause_score(matched_chars: usize, query_chars: usize) -> f32 {
    (matched_chars as f32 / target_clause_len(query_chars) as f32).min(1.0)
}

fn target_clause_len(query_chars: usize) -> usize {
    if query_chars >= 7 {
        7
    } else {
        5
    }
}

fn sort_and_trim_matches(matches: &mut Vec<PoemMatch>, limit: usize) {
    matches.sort_by(|left, right| {
        let left_clause = left.matched_clause.as_deref().unwrap_or_default();
        let right_clause = right.matched_clause.as_deref().unwrap_or_default();
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.matched_chars.cmp(&left.matched_chars))
            .then_with(|| left.poem.id.cmp(&right.poem.id))
            .then_with(|| left_clause.cmp(right_clause))
    });
    matches.truncate(limit);
}

pub fn create_index_schema_sql() -> &'static str {
    r#"
CREATE TABLE IF NOT EXISTS poems (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    author TEXT,
    dynasty TEXT,
    source TEXT
);

CREATE TABLE IF NOT EXISTS clauses (
    id INTEGER PRIMARY KEY,
    poem_id INTEGER NOT NULL REFERENCES poems(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    text TEXT NOT NULL,
    normalized_text TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS char_index (
    ch TEXT NOT NULL,
    clause_id INTEGER NOT NULL REFERENCES clauses(id) ON DELETE CASCADE,
    count INTEGER NOT NULL,
    PRIMARY KEY (ch, clause_id)
);

CREATE TABLE IF NOT EXISTS char_freq (
    ch TEXT PRIMARY KEY,
    document_count INTEGER NOT NULL,
    total_count INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_clauses_poem_id ON clauses(poem_id);
CREATE INDEX IF NOT EXISTS idx_char_index_clause_id ON char_index(clause_id);
"#
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<ColumnInfo>> {
    let sql = format!("PRAGMA table_info({})", quote_identifier(table));
    let mut statement = connection.prepare(&sql)?;
    let columns = statement
        .query_map([], |row| {
            Ok(ColumnInfo {
                name: row.get(1)?,
                data_type: row.get(2)?,
                not_null: row.get::<_, i64>(3)? != 0,
                primary_key: row.get::<_, i64>(5)? != 0,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(columns)
}

fn poem_columns(connection: &Connection) -> Result<HashSet<String>> {
    Ok(table_columns(connection, "poems")?
        .into_iter()
        .map(|column| column.name)
        .collect())
}

fn poem_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Poem> {
    let id = row.get("id")?;
    let title: Option<String> = row.get("title")?;
    let author: Option<String> = row.get("author")?;
    let dynasty: Option<String> = row.get("dynasty")?;
    let source: Option<String> = row.get("source")?;
    let content: Option<String> = row.get("content")?;
    let paragraphs_column: Option<String> = row.get("paragraphs")?;
    let parsed = parse_poem_content(content.as_deref(), paragraphs_column.as_deref());

    Ok(Poem {
        id,
        title: parsed.title.or(title).unwrap_or_default(),
        author: parsed.author.or(author),
        dynasty,
        paragraphs: parsed.paragraphs,
        source,
    })
}

#[derive(Default)]
struct ParsedPoemContent {
    title: Option<String>,
    author: Option<String>,
    paragraphs: Vec<String>,
}

fn parse_poem_content(content: Option<&str>, paragraphs: Option<&str>) -> ParsedPoemContent {
    if let Some(content) = content {
        if let Ok(json) = serde_json::from_str::<LegacyContent>(content) {
            return ParsedPoemContent {
                title: json.title,
                author: json.author,
                paragraphs: json.content,
            };
        }
        if !content.trim().is_empty() {
            return ParsedPoemContent {
                paragraphs: split_paragraphs(content),
                ..ParsedPoemContent::default()
            };
        }
    }

    ParsedPoemContent {
        paragraphs: paragraphs.map(split_paragraphs).unwrap_or_default(),
        ..ParsedPoemContent::default()
    }
}

#[derive(Deserialize)]
struct LegacyContent {
    title: Option<String>,
    author: Option<String>,
    #[serde(default, alias = "paragraphs")]
    content: Vec<String>,
}

fn split_paragraphs(text: &str) -> Vec<String> {
    if let Ok(values) = serde_json::from_str::<Vec<String>>(text) {
        return values;
    }

    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn is_cjk_punctuation(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3000..=0x303f | 0xfe10..=0xfe1f | 0xfe30..=0xfe4f | 0xff00..=0xff0f | 0xff1a..=0xff20 | 0xff3b..=0xff40 | 0xff5b..=0xff65
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_and_cleans_poem_task() {
        let raw = "40分\n请\n从\n以下\n字\n中\n选出\n一句诗词\n应\n怜\n屐\n齿\n印\n苍\n苔\n确定";

        assert!(detect_poem_task(raw));
        assert_eq!(clean_poem_chars(raw).as_deref(), Some("应怜屐齿印苍苔"));
    }

    #[test]
    fn keeps_normal_question_out_of_poetry_cleaning() {
        let raw = "李白写过什么诗？\n确定";

        assert!(!detect_poem_task(raw));
        assert_eq!(clean_poem_chars(raw), None);
    }
    #[test]
    fn normalizes_text() {
        assert_eq!(
            normalize_text(
                " \u{5c71}\u{4e00}\u{7a0b}\u{ff0c}\u{6c34}\u{4e00}\u{7a0b}\u{3002}\n&nbsp;"
            ),
            "\u{5c71}\u{4e00}\u{7a0b}\u{6c34}\u{4e00}\u{7a0b}"
        );
    }

    #[test]
    fn counts_matching_chars() {
        let query = char_counts("aabc");
        let poem = char_counts("abbc");
        assert_eq!(count_intersection(&query, &poem), 3);
    }

    #[test]
    fn creates_index_schema_sql() {
        let sql = create_index_schema_sql();
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS poems"));
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS clauses"));
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS char_index"));
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS char_freq"));
    }

    #[test]
    fn loads_and_matches_legacy_content() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE poems (
                    id INTEGER PRIMARY KEY,
                    title TEXT NOT NULL,
                    author TEXT,
                    content TEXT NOT NULL,
                    dynasty TEXT,
                    source TEXT
                );
                INSERT INTO poems (id, title, author, content, dynasty, source)
                VALUES (1, 'Song A', 'Author A', '{"title":"Song A","author":"Author A","content":["mountain road","river road"]}', 'Dynasty A', 'test');
                INSERT INTO poems (id, title, author, content, dynasty, source)
                VALUES (2, 'Moon Song', 'Author B', '{"title":"Moon Song","author":"Author B","content":["moonlit","frost"]}', 'Dynasty B', 'test');
                "#,
            )
            .unwrap();

        let poem = load_poem(&connection, 1).unwrap().unwrap();
        assert_eq!(poem.title, "Song A");
        assert_eq!(poem.paragraphs, vec!["mountain road", "river road"]);

        let matches = find_poem_from_chars(&connection, "tilnoom", 1).unwrap();
        assert_eq!(matches[0].poem.title, "Moon Song");
        assert_eq!(matches[0].matched_chars, 7);
        assert_eq!(matches[0].matched_clause.as_deref(), Some("moonlit"));
    }
}

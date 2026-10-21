//! CSV import: rows are read from the file in batches and inserted inside a
//! single transaction, so a bad row leaves the table as it was.

use std::fs::File;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::Progress;
use crate::db::{bulk, meta, Session};
use crate::error::{AppError, AppResult};
use crate::model::TableRef;
use crate::sql::builder::{qualified_table, quote_ident, quote_literal};
use crate::sql::Dialect;

const PREVIEW_ROWS: usize = 20;
const DEFAULT_BATCH: usize = 500;
/// Keeps one INSERT well under MySQL's default max_allowed_packet (64 MB).
const MAX_BATCH_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvPreview {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub delimiter: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMapping {
    pub column: String,
    /// Index of the CSV field that fills `column`.
    pub source: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvImportOptions {
    pub delimiter: String,
    pub has_header: bool,
    /// Empty fields become NULL instead of ''.
    pub empty_as_null: bool,
    /// Delete existing rows first (in the same transaction).
    pub truncate_first: bool,
    pub mapping: Vec<ColumnMapping>,
    #[serde(default)]
    pub rows_per_batch: Option<usize>,
}

/// The delimiter that splits the first line into the most fields.
pub fn sniff_delimiter(first_line: &str) -> u8 {
    [b',', b';', b'\t', b'|']
        .into_iter()
        .max_by_key(|d| first_line.bytes().filter(|b| b == d).count())
        .filter(|d| first_line.as_bytes().contains(d))
        .unwrap_or(b',')
}

fn delimiter_byte(d: &str) -> AppResult<u8> {
    match d {
        "\\t" | "\t" | "tab" => Ok(b'\t'),
        _ if d.len() == 1 => Ok(d.as_bytes()[0]),
        _ => Err(AppError::Invalid(format!(
            "delimiter must be one character, got `{d}`"
        ))),
    }
}

fn reader(path: &Path, delimiter: u8, has_header: bool) -> AppResult<csv::Reader<File>> {
    Ok(csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(has_header)
        .flexible(true)
        .from_reader(File::open(path)?))
}

fn csv_error(row: u64, e: csv::Error) -> AppError {
    if matches!(e.kind(), csv::ErrorKind::Utf8 { .. }) {
        return AppError::Invalid(format!(
            "Row {row}: the file isn't UTF-8 text. Save it as UTF-8 CSV and try again."
        ));
    }
    AppError::Invalid(format!("Row {row}: {e}"))
}

pub fn preview(path: &Path, delimiter: Option<&str>, has_header: bool) -> AppResult<CsvPreview> {
    let size = std::fs::metadata(path)?.len();
    let delimiter = match delimiter {
        Some(d) => delimiter_byte(d)?,
        None => {
            let mut head = String::new();
            use std::io::BufRead;
            std::io::BufReader::new(File::open(path)?).read_line(&mut head)?;
            sniff_delimiter(&head)
        }
    };
    let mut rdr = reader(path, delimiter, has_header)?;
    let mut headers = if has_header {
        rdr.headers()
            .map_err(|e| csv_error(0, e))?
            .iter()
            .map(String::from)
            .collect()
    } else {
        Vec::new()
    };
    if let Some(first) = headers.first_mut() {
        *first = first.trim_start_matches('\u{feff}').to_string();
    }
    let mut rows = Vec::new();
    for (i, record) in rdr.records().take(PREVIEW_ROWS).enumerate() {
        let record = record.map_err(|e| csv_error(i as u64 + 1, e))?;
        rows.push(record.iter().map(String::from).collect::<Vec<_>>());
    }
    if !has_header {
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        headers = (1..=width).map(|i| format!("Column {i}")).collect();
    }
    let delimiter = if delimiter == b'\t' {
        "\\t".to_string()
    } else {
        (delimiter as char).to_string()
    };
    Ok(CsvPreview {
        headers,
        rows,
        delimiter,
        size,
    })
}

pub async fn import_csv(
    s: &Session,
    table: &TableRef,
    path: &Path,
    opts: &CsvImportOptions,
    progress: &dyn Progress,
) -> AppResult<u64> {
    if opts.mapping.is_empty() {
        return Err(AppError::Invalid("map at least one column".into()));
    }
    let d = s.dialect();
    let columns = meta::columns(s, table).await?;
    for m in &opts.mapping {
        if !columns.iter().any(|c| c.name == m.column) {
            return Err(AppError::Invalid(format!(
                "`{}` is not a column of {}",
                m.column, table.table
            )));
        }
    }
    let identity = opts.mapping.iter().any(|m| {
        columns
            .iter()
            .any(|c| c.name == m.column && c.is_auto_increment)
    });
    let name = qualified_table(d, table);
    let list = opts
        .mapping
        .iter()
        .map(|m| quote_ident(d, &m.column))
        .collect::<Vec<_>>()
        .join(", ");
    let overriding = if d == Dialect::Postgres && identity {
        "OVERRIDING SYSTEM VALUE "
    } else {
        ""
    };
    let (before, after) = if d == Dialect::Mssql && identity {
        (
            format!("SET IDENTITY_INSERT {name} ON;\n"),
            format!(";\nSET IDENTITY_INSERT {name} OFF"),
        )
    } else {
        (String::new(), String::new())
    };
    let per = opts
        .rows_per_batch
        .unwrap_or(DEFAULT_BATCH)
        .clamp(1, if d == Dialect::Mssql { 1000 } else { 5000 });

    let mut batches = Batches {
        reader: reader(path, delimiter_byte(&opts.delimiter)?, opts.has_header)?,
        dialect: d,
        prefix: format!("{before}INSERT INTO {name} ({list}) {overriding}VALUES"),
        suffix: after,
        sources: opts.mapping.iter().map(|m| m.source).collect(),
        per,
        empty_as_null: opts.empty_as_null,
        progress,
        size: std::fs::metadata(path)?.len(),
        rows: 0,
        first: opts.truncate_first.then(|| format!("DELETE FROM {name}")),
        done: false,
    };
    s.apply_batches(table.database.as_deref(), &mut batches)
        .await?;
    progress.report(
        batches.size,
        Some(batches.size),
        &format!("{} rows", batches.rows),
    );
    Ok(batches.rows)
}

struct Batches<'a> {
    reader: csv::Reader<File>,
    dialect: Dialect,
    prefix: String,
    suffix: String,
    sources: Vec<usize>,
    per: usize,
    empty_as_null: bool,
    progress: &'a dyn Progress,
    size: u64,
    rows: u64,
    first: Option<String>,
    done: bool,
}

impl Batches<'_> {
    fn tuple(&self, record: &csv::StringRecord, row: u64) -> AppResult<String> {
        let needed = self.sources.iter().max().copied().unwrap_or(0) + 1;
        if record.len() < needed {
            return Err(AppError::Invalid(format!(
                "Row {row} has {} fields, but the mapping uses field {needed}",
                record.len()
            )));
        }
        let values: Vec<String> = self
            .sources
            .iter()
            .map(|&i| {
                let field = &record[i];
                if field.is_empty() && self.empty_as_null {
                    "NULL".to_string()
                } else {
                    quote_literal(self.dialect, field)
                }
            })
            .collect();
        Ok(format!("({})", values.join(", ")))
    }

    fn next_batch(&mut self) -> AppResult<Option<String>> {
        if self.progress.cancelled() {
            return Err(bulk::cancelled());
        }
        let mut tuples = Vec::with_capacity(self.per);
        let mut bytes = 0;
        let mut record = csv::StringRecord::new();
        while tuples.len() < self.per && bytes < MAX_BATCH_BYTES {
            let row = self.rows + 1;
            if !self
                .reader
                .read_record(&mut record)
                .map_err(|e| csv_error(row, e))?
            {
                self.done = true;
                break;
            }
            let tuple = self.tuple(&record, row)?;
            bytes += tuple.len();
            tuples.push(tuple);
            self.rows += 1;
        }
        let position = self.reader.position().byte();
        self.progress
            .report(position, Some(self.size), &format!("{} rows", self.rows));
        if tuples.is_empty() {
            return Ok(None);
        }
        Ok(Some(format!(
            "{}\n{}{}",
            self.prefix,
            tuples.join(",\n"),
            self.suffix
        )))
    }
}

impl Iterator for Batches<'_> {
    type Item = AppResult<String>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(first) = self.first.take() {
            return Some(Ok(first));
        }
        if self.done {
            return None;
        }
        match self.next_batch() {
            Ok(Some(batch)) => Some(Ok(batch)),
            Ok(None) => None,
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_common_delimiters() {
        assert_eq!(sniff_delimiter("a,b,c"), b',');
        assert_eq!(sniff_delimiter("a;b;c,d"), b';');
        assert_eq!(sniff_delimiter("a\tb\tc"), b'\t');
        assert_eq!(sniff_delimiter("single"), b',');
    }

    #[test]
    fn previews_with_and_without_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.csv");
        std::fs::write(&path, "\u{feff}id;name\n1;\"Ana; Jr\"\n2;Budi\n").unwrap();
        let p = preview(&path, None, true).unwrap();
        assert_eq!(p.delimiter, ";");
        assert_eq!(p.headers, vec!["id", "name"]);
        assert_eq!(p.rows[0], vec!["1", "Ana; Jr"]);
        let p = preview(&path, Some(";"), false).unwrap();
        assert_eq!(p.headers, vec!["Column 1", "Column 2"]);
        assert_eq!(p.rows.len(), 3);
    }

    #[test]
    fn rejects_bad_delimiters() {
        assert!(delimiter_byte(";;").is_err());
        assert_eq!(delimiter_byte("\\t").unwrap(), b'\t');
    }
}

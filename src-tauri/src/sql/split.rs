//! A small SQL lexer that knows just enough to split scripts into statements
//! and to list the bare keywords of each one (used by the read-only guard).
//!
//! It understands strings, quoted identifiers, comments, Postgres
//! dollar-quoting, MySQL `DELIMITER`, SQL Server `GO` batches and
//! `CREATE TRIGGER ... BEGIN ... END` bodies.

use super::Dialect;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub text: String,
    /// Unquoted words, upper-cased, in order. Strings, comments and quoted
    /// identifiers are excluded, so `'DROP'` or `"delete"` never show up.
    pub words: Vec<String>,
    /// MySQL `/*! ... */` or MariaDB `/*M! ... */`: comments the server runs.
    pub has_executable_comment: bool,
}

impl Statement {
    pub fn first_word(&self) -> Option<&str> {
        self.words.first().map(String::as_str)
    }
}

pub fn split(sql: &str, dialect: Dialect) -> Vec<Statement> {
    Splitter::new(sql, dialect).collect()
}

/// Statements one at a time, so a large script never needs all of them in
/// memory at once.
pub fn split_iter(sql: &str, dialect: Dialect) -> impl Iterator<Item = Statement> + Send + '_ {
    Splitter::new(sql, dialect)
}

struct Splitter<'a> {
    src: &'a str,
    bytes: &'a [u8],
    dialect: Dialect,
    pos: usize,
    start: usize,
    words: Vec<String>,
    delimiter: String,
    block_depth: usize,
    has_code: bool,
    has_executable_comment: bool,
    out: std::collections::VecDeque<Statement>,
    finished: bool,
}

impl<'a> Splitter<'a> {
    fn new(src: &'a str, dialect: Dialect) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            dialect,
            pos: 0,
            start: 0,
            words: Vec::new(),
            delimiter: ";".to_string(),
            block_depth: 0,
            has_code: false,
            has_executable_comment: false,
            out: std::collections::VecDeque::new(),
            finished: false,
        }
    }

    /// Lex until the next statement is complete (or the input ends).
    fn advance(&mut self) {
        while self.out.is_empty() && !self.finished {
            if self.pos >= self.bytes.len() {
                self.flush(self.bytes.len());
                self.finished = true;
            } else if self.at_delimiter() {
                let end = self.pos;
                self.pos += self.delimiter.len();
                self.flush(end);
            } else {
                self.step();
            }
        }
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn at_delimiter(&self) -> bool {
        if self.dialect == Dialect::Mssql || self.block_depth > 0 {
            return false;
        }
        self.bytes[self.pos..].starts_with(self.delimiter.as_bytes())
    }

    fn step(&mut self) {
        let c = self.bytes[self.pos];
        let is_comment = self.at_line_comment()
            || (c == b'#' && self.dialect == Dialect::Mysql)
            || (c == b'/' && self.peek(1) == Some(b'*'));
        // Words decide for themselves: `GO` and `DELIMITER` are not code.
        if !is_comment && !c.is_ascii_whitespace() && !is_word_start(c) {
            self.has_code = true;
        }
        match c {
            b'-' if self.at_line_comment() => self.skip_line(),
            b'#' if self.dialect == Dialect::Mysql => self.skip_line(),
            b'/' if self.peek(1) == Some(b'*') => self.skip_block_comment(),
            b'\'' => self.skip_quoted(b'\'', self.backslash_escapes()),
            b'"' => self.skip_quoted(b'"', self.dialect == Dialect::Mysql),
            b'`' if matches!(self.dialect, Dialect::Mysql | Dialect::Sqlite) => {
                self.skip_quoted(b'`', false)
            }
            b'[' if matches!(self.dialect, Dialect::Mssql | Dialect::Sqlite) => self.skip_bracket(),
            b'$' if self.dialect == Dialect::Postgres => self.dollar_or_symbol(),
            _ if is_word_start(c) => self.word(),
            _ => self.pos += utf8_len(c),
        }
    }

    fn backslash_escapes(&self) -> bool {
        if self.dialect == Dialect::Mysql {
            return true;
        }
        // Postgres E'...' strings
        self.dialect == Dialect::Postgres
            && self.pos > 0
            && matches!(self.bytes[self.pos - 1], b'E' | b'e')
            && (self.pos < 2 || !is_word_char(self.bytes[self.pos - 2]))
    }

    /// `--` starts a comment, except in MySQL where it needs a following
    /// space or control character (`1--1` is arithmetic there).
    fn at_line_comment(&self) -> bool {
        if self.peek(0) != Some(b'-') || self.peek(1) != Some(b'-') {
            return false;
        }
        if self.dialect != Dialect::Mysql {
            return true;
        }
        !matches!(self.peek(2), Some(c) if c > b' ')
    }

    fn skip_line(&mut self) {
        while let Some(c) = self.peek(0) {
            if c == b'\n' {
                break;
            }
            self.pos += 1;
        }
    }

    fn skip_block_comment(&mut self) {
        // Postgres and SQL Server nest block comments; MySQL and SQLite don't.
        let nested = matches!(self.dialect, Dialect::Postgres | Dialect::Mssql);
        if self.dialect == Dialect::Mysql {
            let rest = &self.bytes[self.pos + 2..];
            if rest.starts_with(b"!") || rest.starts_with(b"M!") {
                self.has_executable_comment = true;
            }
        }
        let mut depth = 0usize;
        while self.pos < self.bytes.len() {
            if self.peek(0) == Some(b'/') && self.peek(1) == Some(b'*') {
                if depth == 0 || nested {
                    depth += 1;
                }
                self.pos += 2;
            } else if self.peek(0) == Some(b'*') && self.peek(1) == Some(b'/') {
                depth -= 1;
                self.pos += 2;
                if depth == 0 {
                    return;
                }
            } else {
                self.pos += 1;
            }
        }
    }

    fn skip_quoted(&mut self, quote: u8, backslash: bool) {
        self.pos += 1;
        while let Some(c) = self.peek(0) {
            if backslash && c == b'\\' {
                self.pos = (self.pos + 2).min(self.bytes.len());
                continue;
            }
            self.pos += 1;
            if c == quote {
                if self.peek(0) == Some(quote) {
                    self.pos += 1; // doubled quote is an escaped quote
                } else {
                    return;
                }
            }
        }
    }

    fn skip_bracket(&mut self) {
        self.pos += 1;
        while let Some(c) = self.peek(0) {
            self.pos += 1;
            if c == b']' {
                if self.peek(0) == Some(b']') {
                    self.pos += 1;
                } else {
                    return;
                }
            }
        }
    }

    fn dollar_or_symbol(&mut self) {
        let rest = &self.src[self.pos + 1..];
        let tag_len = rest
            .bytes()
            .take_while(|&b| b.is_ascii_alphanumeric() || b == b'_')
            .count();
        let starts_with_digit = rest.bytes().next().is_some_and(|b| b.is_ascii_digit());
        if starts_with_digit || rest.as_bytes().get(tag_len) != Some(&b'$') {
            self.pos += 1; // `$1` parameter or a lone dollar
            return;
        }
        let tag = &self.src[self.pos..self.pos + tag_len + 2];
        let body_start = self.pos + tag.len();
        match self.src[body_start..].find(tag) {
            Some(i) => self.pos = body_start + i + tag.len(),
            None => self.pos = self.bytes.len(),
        }
    }

    fn word(&mut self) {
        let begin = self.pos;
        while let Some(c) = self.peek(0) {
            if !is_word_char(c) {
                break;
            }
            self.pos += utf8_len(c);
        }
        let word = self.src[begin..self.pos].to_ascii_uppercase();

        if self.dialect == Dialect::Mssql && word == "GO" && self.is_alone_on_line(begin) {
            self.skip_line();
            self.flush(begin);
            return;
        }
        if self.dialect == Dialect::Mysql && word == "DELIMITER" && self.words.is_empty() {
            self.change_delimiter();
            return;
        }
        self.has_code = true;
        self.track_blocks(&word);
        self.words.push(word);
    }

    fn is_alone_on_line(&self, begin: usize) -> bool {
        let line_start = self.src[..begin].rfind('\n').map_or(0, |i| i + 1);
        let before_blank = self.src[line_start..begin].trim().is_empty();
        let line_end = self.src[self.pos..]
            .find('\n')
            .map_or(self.src.len(), |i| self.pos + i);
        let after = self.src[self.pos..line_end].trim();
        // `GO 5` (repeat count) and trailing comments are allowed.
        let after_ok = after.is_empty()
            || after.starts_with("--")
            || after.bytes().all(|b| b.is_ascii_digit() || b == b' ');
        before_blank && after_ok
    }

    fn change_delimiter(&mut self) {
        let line_end = self.src[self.pos..]
            .find('\n')
            .map_or(self.src.len(), |i| self.pos + i);
        let delimiter = self.src[self.pos..line_end].trim();
        if !delimiter.is_empty() {
            self.delimiter = delimiter.to_string();
        }
        self.pos = line_end;
        self.start = line_end;
    }

    /// Inside `CREATE TRIGGER/PROCEDURE/FUNCTION ... BEGIN ... END` a `;`
    /// does not end the statement.
    fn track_blocks(&mut self, word: &str) {
        if !self.is_routine_definition() {
            return;
        }
        match word {
            "BEGIN" | "CASE" => self.block_depth += 1,
            "END" => self.block_depth = self.block_depth.saturating_sub(1),
            _ => {}
        }
    }

    fn is_routine_definition(&self) -> bool {
        if !matches!(self.dialect, Dialect::Sqlite | Dialect::Mysql) || self.delimiter != ";" {
            return false;
        }
        if self.words.first().map(String::as_str) != Some("CREATE") {
            return false;
        }
        self.words
            .iter()
            .take(6)
            .any(|w| matches!(w.as_str(), "TRIGGER" | "PROCEDURE" | "FUNCTION" | "EVENT"))
    }

    fn flush(&mut self, end: usize) {
        let text = self.src[self.start.min(end)..end].trim();
        let words = std::mem::take(&mut self.words);
        let has_executable_comment = std::mem::take(&mut self.has_executable_comment);
        if std::mem::take(&mut self.has_code) || has_executable_comment {
            self.out.push_back(Statement {
                text: text.to_string(),
                words,
                has_executable_comment,
            });
        }
        self.start = self.pos;
        self.block_depth = 0;
    }
}

impl Iterator for Splitter<'_> {
    type Item = Statement;

    fn next(&mut self) -> Option<Statement> {
        self.advance();
        self.out.pop_front()
    }
}

fn is_word_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'@' || c == b'#' || c >= 0x80
}

fn is_word_char(c: u8) -> bool {
    is_word_start(c) || c.is_ascii_digit() || c == b'$'
}

fn utf8_len(first: u8) -> usize {
    match first {
        0xF0..=0xFF => 4,
        0xE0..=0xEF => 3,
        0xC0..=0xDF => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(sql: &str, d: Dialect) -> Vec<String> {
        split(sql, d).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn splits_on_semicolons_and_drops_empty_statements() {
        let got = texts(
            "select 1;  ;\nselect 2;\n-- trailing comment\n",
            Dialect::Postgres,
        );
        assert_eq!(got, vec!["select 1", "select 2"]);
    }

    #[test]
    fn ignores_semicolons_inside_strings_identifiers_and_comments() {
        let sql = r#"select 'a;b', "c;d" /* e; */ -- f;
            from t; select 2"#;
        let got = texts(sql, Dialect::Postgres);
        assert_eq!(got.len(), 2);
        assert!(got[0].contains("'a;b'"));
    }

    #[test]
    fn handles_doubled_quotes_and_mysql_backslash_escapes() {
        assert_eq!(
            texts("select 'it''s;'; select 2", Dialect::Postgres).len(),
            2
        );
        assert_eq!(texts(r"select 'a\';b'; select 2", Dialect::Mysql).len(), 2);
        // In standard SQL a backslash is just a character.
        assert_eq!(texts(r"select 'a\'; select 2", Dialect::Sqlite).len(), 2);
    }

    #[test]
    fn handles_postgres_dollar_quoting_and_params() {
        let sql = "create function f() returns int as $body$ begin; return 1; end $body$ \
                   language plpgsql; select $1, $$x;y$$";
        let got = texts(sql, Dialect::Postgres);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[1].ends_with("$$x;y$$"));
    }

    #[test]
    fn handles_postgres_escape_strings() {
        assert_eq!(
            texts(r"select E'a\';b'; select 2", Dialect::Postgres).len(),
            2
        );
    }

    #[test]
    fn mssql_splits_on_go_lines_only() {
        let sql = "select 1; select 2\nGO\nselect ago from t\n  go  \nselect 3";
        let got = texts(sql, Dialect::Mssql);
        assert_eq!(
            got,
            vec!["select 1; select 2", "select ago from t", "select 3"]
        );
    }

    #[test]
    fn mysql_delimiter_directive() {
        let sql = "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END //\n\
                   DELIMITER ;\nSELECT 3;";
        let got = texts(sql, Dialect::Mysql);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0].starts_with("CREATE PROCEDURE") && got[0].ends_with("END"));
        assert_eq!(got[1], "SELECT 3");
    }

    #[test]
    fn sqlite_trigger_body_is_one_statement() {
        let sql = "CREATE TRIGGER t AFTER INSERT ON a BEGIN \
                   UPDATE b SET n = CASE WHEN 1 THEN 2 END; INSERT INTO c VALUES (1); END; \
                   SELECT 1";
        let got = texts(sql, Dialect::Sqlite);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0].ends_with("END"));
    }

    #[test]
    fn transaction_begin_still_splits() {
        let got = texts("BEGIN; UPDATE t SET a = 1; COMMIT;", Dialect::Sqlite);
        assert_eq!(got.len(), 3);
    }

    #[test]
    fn words_exclude_strings_comments_and_quoted_names() {
        let s = &split(
            r#"select 'drop', "delete" -- update
            from [insert] /* alter */ where x = 1"#,
            Dialect::Mssql,
        )[0];
        assert_eq!(s.words, vec!["SELECT", "FROM", "WHERE", "X"]);
    }

    #[test]
    fn unicode_text_does_not_panic() {
        let got = texts(
            "select 'héllo — wörld'; select naïve from tàble",
            Dialect::Postgres,
        );
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn unterminated_constructs_do_not_panic() {
        for sql in [
            "select 'abc",
            "select /* abc",
            "select $tag$ abc",
            "select [abc",
            "--",
        ] {
            for d in [
                Dialect::Postgres,
                Dialect::Mysql,
                Dialect::Sqlite,
                Dialect::Mssql,
            ] {
                let _ = split(sql, d);
            }
        }
    }
}

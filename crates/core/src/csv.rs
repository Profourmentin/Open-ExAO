//! Import et export CSV.
//!
//! L'import détecte le séparateur (`;`, tabulation ou `,`) et accepte la virgule
//! décimale. La première ligne est l'en-tête : `t (s)` ou `t [s]` donne la colonne
//! `t` d'unité `s`. Les cellules non numériques sont laissées vides et signalées.

use std::fmt;

use crate::format::{format_exact, parse_number};
use crate::table::{Cell, Table, is_valid_name};

/// Format d'écriture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsvFormat {
    pub delimiter: char,
    pub decimal_comma: bool,
}

impl CsvFormat {
    /// `;` comme séparateur et virgule décimale (ouvre bien dans un tableur français).
    pub const FRENCH: CsvFormat = CsvFormat {
        delimiter: ';',
        decimal_comma: true,
    };
    /// `,` comme séparateur et point décimal.
    pub const INTERNATIONAL: CsvFormat = CsvFormat {
        delimiter: ',',
        decimal_comma: false,
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvError {
    Empty,
    UnterminatedQuote,
}

impl fmt::Display for CsvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CsvError::Empty => write!(f, "le fichier CSV est vide"),
            CsvError::UnterminatedQuote => write!(f, "guillemet non fermé dans le fichier CSV"),
        }
    }
}

impl std::error::Error for CsvError {}

/// Résultat d'un import : le tableau et les avertissements à montrer à l'utilisateur.
#[derive(Debug, Clone)]
pub struct CsvImport {
    pub table: Table,
    pub warnings: Vec<String>,
    pub delimiter: char,
}

const MAX_WARNINGS: usize = 20;

/// Devine le séparateur d'après la première ligne (hors guillemets).
pub fn detect_delimiter(text: &str) -> char {
    let mut counts = [(';', 0usize), ('\t', 0), (',', 0)];
    let mut in_quotes = false;
    for c in text.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            '\n' | '\r' if !in_quotes => break,
            _ if !in_quotes => {
                for (d, n) in &mut counts {
                    if c == *d {
                        *n += 1;
                    }
                }
            }
            _ => {}
        }
    }
    // En cas d'égalité, l'ordre ; puis tabulation puis virgule l'emporte.
    let mut best = (',', 0);
    for (d, n) in counts {
        if n > best.1 {
            best = (d, n);
        }
    }
    best.0
}

/// Découpe le texte en enregistrements et champs (guillemets, `""`, retours à la ligne inclus).
fn parse_records(text: &str, delimiter: char) -> Result<Vec<Vec<String>>, CsvError> {
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    let mut field_started = false;

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' => {
                in_quotes = true;
                field_started = true;
            }
            c if c == delimiter => {
                record.push(std::mem::take(&mut field));
                field_started = true;
            }
            '\r' => {}
            '\n' => {
                if field_started || !field.is_empty() || !record.is_empty() {
                    record.push(std::mem::take(&mut field));
                    records.push(std::mem::take(&mut record));
                }
                field_started = false;
            }
            _ => {
                field.push(c);
                field_started = true;
            }
        }
    }
    if in_quotes {
        return Err(CsvError::UnterminatedQuote);
    }
    if field_started || !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}

/// Sépare `t (s)` ou `t [s]` en (`t`, `s`).
fn split_header(header: &str) -> (String, String) {
    let h = header.trim();
    for (open, close) in [('(', ')'), ('[', ']')] {
        if h.ends_with(close) {
            if let Some(pos) = h.rfind(open) {
                let name = h[..pos].trim().to_string();
                let unit = h[pos + 1..h.len() - 1].trim().to_string();
                return (name, unit);
            }
        }
    }
    (h.to_string(), String::new())
}

/// Transforme un en-tête quelconque en nom de colonne valide.
fn sanitize_name(raw: &str, fallback_index: usize) -> String {
    let mut name: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() || name.chars().all(|c| c == '_') {
        return format!("col{}", fallback_index + 1);
    }
    if !is_valid_name(&name) {
        name.insert(0, '_');
    }
    name
}

fn is_header_row(fields: &[String]) -> bool {
    // Une ligne dont tous les champs sont numériques n'est pas un en-tête.
    !fields
        .iter()
        .all(|f| f.trim().is_empty() || parse_number(f).is_some())
}

/// Lit un texte CSV. La première ligne est l'en-tête, sauf si elle ne contient que des nombres.
pub fn import_csv(text: &str) -> Result<CsvImport, CsvError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let delimiter = detect_delimiter(text);
    let records = parse_records(text, delimiter)?;
    let Some(first) = records.first() else {
        return Err(CsvError::Empty);
    };

    let mut warnings = Vec::new();
    let mut skipped = 0usize;
    let mut warn = |warnings: &mut Vec<String>, message: String| {
        if warnings.len() < MAX_WARNINGS {
            warnings.push(message);
        } else {
            skipped += 1;
        }
    };

    let width = records.iter().map(Vec::len).max().unwrap_or(0);
    let has_header = is_header_row(first);
    let body_start = usize::from(has_header);

    let mut table = Table::with_rows(records.len() - body_start);
    let mut names_used: Vec<String> = Vec::new();
    for c in 0..width {
        let (raw_name, unit) = if has_header {
            first.get(c).map(|h| split_header(h)).unwrap_or_default()
        } else {
            (String::new(), String::new())
        };
        let mut name = sanitize_name(&raw_name, c);
        if name != raw_name.trim() && !raw_name.trim().is_empty() {
            warn(
                &mut warnings,
                format!("en-tête « {} » renommé en « {name} »", raw_name.trim()),
            );
        }
        let base = name.clone();
        let mut n = 2;
        while names_used.contains(&name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        names_used.push(name.clone());
        table
            .add_data_column(&name, &unit)
            .expect("le nom a été validé et rendu unique");
    }
    if !has_header {
        warn(
            &mut warnings,
            "aucune ligne d'en-tête détectée : colonnes nommées col1, col2…".to_string(),
        );
    }

    for (r, record) in records.iter().enumerate().skip(body_start) {
        let row = r - body_start;
        // Lignes courtes : les cellules manquantes restent vides, sans avertissement.
        for (c, raw) in record.iter().enumerate() {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            match parse_number(raw) {
                Some(x) => table.set_data_cell(c, row, Some(x)),
                None => warn(
                    &mut warnings,
                    format!(
                        "ligne {}, colonne « {} » : « {raw} » n'est pas un nombre, cellule laissée vide",
                        r + 1,
                        names_used[c]
                    ),
                ),
            }
        }
    }
    if skipped > 0 {
        warnings.push(format!("… et {skipped} autre(s) avertissement(s)"));
    }
    table.recompute();
    Ok(CsvImport {
        table,
        warnings,
        delimiter,
    })
}

fn quote_if_needed(field: &str, delimiter: char) -> String {
    if field.contains(delimiter)
        || field.contains('"')
        || field.contains('\n')
        || field.contains('\r')
    {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Écrit le tableau en CSV. Les colonnes calculées sont écrites comme des valeurs ;
/// les cellules vides et les cellules en erreur sont écrites vides.
pub fn export_csv(table: &Table, format: CsvFormat) -> String {
    let d = format.delimiter;
    let mut out = String::new();
    let headers: Vec<String> = table
        .columns()
        .iter()
        .map(|c| {
            let h = if c.unit().is_empty() {
                c.name().to_string()
            } else {
                format!("{} ({})", c.name(), c.unit())
            };
            quote_if_needed(&h, d)
        })
        .collect();
    out.push_str(&headers.join(&d.to_string()));
    out.push('\n');
    for row in 0..table.rows() {
        let line: Vec<String> = (0..table.column_count())
            .map(|col| match table.cell(col, row) {
                Some(Cell::Num(x)) => format_exact(*x, format.decimal_comma),
                _ => String::new(),
            })
            .collect();
        out.push_str(&line.join(&d.to_string()));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell_num(t: &Table, c: usize, r: usize) -> Option<f64> {
        match t.cell(c, r) {
            Some(Cell::Num(x)) => Some(*x),
            _ => None,
        }
    }

    #[test]
    fn imports_french_csv() {
        let csv = "t (s);y (m)\n0;0\n0,1;0,049\n0,2;0,196\n";
        let imp = import_csv(csv).unwrap();
        assert_eq!(imp.delimiter, ';');
        assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
        let t = &imp.table;
        assert_eq!((t.rows(), t.column_count()), (3, 2));
        assert_eq!(t.column(0).unwrap().name(), "t");
        assert_eq!(t.column(0).unwrap().unit(), "s");
        assert_eq!(t.column(1).unwrap().unit(), "m");
        assert_eq!(cell_num(t, 1, 1), Some(0.049));
        assert_eq!(cell_num(t, 0, 2), Some(0.2));
    }

    #[test]
    fn imports_international_and_tab_csv() {
        let imp = import_csv("a,b\n1.5,2\n3,4.25\n").unwrap();
        assert_eq!(imp.delimiter, ',');
        assert_eq!(cell_num(&imp.table, 0, 0), Some(1.5));
        assert_eq!(cell_num(&imp.table, 1, 1), Some(4.25));

        let imp = import_csv("a\tb\n1,5\t2\n").unwrap();
        assert_eq!(imp.delimiter, '\t');
        assert_eq!(cell_num(&imp.table, 0, 0), Some(1.5));
    }

    #[test]
    fn handles_crlf_bom_blank_lines_and_missing_final_newline() {
        let imp = import_csv("\u{feff}a;b\r\n1;2\r\n\r\n3;4").unwrap();
        assert_eq!(imp.table.rows(), 2);
        assert_eq!(imp.table.column(0).unwrap().name(), "a");
        assert_eq!(cell_num(&imp.table, 1, 1), Some(4.0));
    }

    #[test]
    fn non_numeric_cells_become_empty_with_a_warning() {
        let imp = import_csv("a;b\n1;abc\n2;\n3;4\n").unwrap();
        assert_eq!(imp.table.cell(1, 0), Some(&Cell::Empty));
        assert_eq!(imp.table.cell(1, 1), Some(&Cell::Empty));
        assert_eq!(imp.warnings.len(), 1);
        assert!(imp.warnings[0].contains("ligne 2") && imp.warnings[0].contains("abc"));
    }

    #[test]
    fn ragged_rows_are_padded() {
        let imp = import_csv("a;b;c\n1;2\n4;5;6;7\n").unwrap();
        assert_eq!(imp.table.column_count(), 4);
        assert_eq!(imp.table.cell(2, 0), Some(&Cell::Empty));
        assert_eq!(cell_num(&imp.table, 3, 1), Some(7.0));
    }

    #[test]
    fn headers_are_sanitized_and_made_unique() {
        let imp = import_csv("temps total;x;x;1abc;\n1;2;3;4;5\n").unwrap();
        let names: Vec<&str> = imp.table.columns().iter().map(|c| c.name()).collect();
        assert_eq!(names, ["temps_total", "x", "x_2", "_1abc", "col5"]);
        assert!(imp.warnings.iter().any(|w| w.contains("temps total")));
    }

    #[test]
    fn headerless_file_is_detected() {
        let imp = import_csv("1;2\n3;4\n").unwrap();
        assert_eq!(imp.table.rows(), 2);
        assert_eq!(imp.table.column(0).unwrap().name(), "col1");
        assert!(
            imp.warnings
                .iter()
                .any(|w| w.contains("aucune ligne d'en-tête"))
        );
    }

    #[test]
    fn quoted_fields() {
        let csv = "\"nom, long\";\"v (m/s)\"\n1;2\n";
        let imp = import_csv(csv).unwrap();
        assert_eq!(imp.table.column(1).unwrap().unit(), "m/s");
        let records = parse_records("a;\"x;y\";\"he said \"\"hi\"\"\"\n", ';').unwrap();
        assert_eq!(records, vec![vec!["a", "x;y", "he said \"hi\""]]);
        let records = parse_records("\"multi\nligne\";b\n", ';').unwrap();
        assert_eq!(records[0][0], "multi\nligne");
    }

    #[test]
    fn rejects_empty_and_unterminated() {
        assert_eq!(import_csv("").unwrap_err(), CsvError::Empty);
        assert_eq!(import_csv("\n\n").unwrap_err(), CsvError::Empty);
        assert_eq!(
            import_csv("a;\"b\n1;2").unwrap_err(),
            CsvError::UnterminatedQuote
        );
    }

    #[test]
    fn warnings_are_capped() {
        let mut csv = String::from("a\n");
        for _ in 0..50 {
            csv.push_str("oops\n");
        }
        let imp = import_csv(&csv).unwrap();
        assert_eq!(imp.warnings.len(), MAX_WARNINGS + 1);
        assert!(imp.warnings.last().unwrap().contains("30 autre"));
    }

    #[test]
    fn export_import_round_trip_french_and_international() {
        let mut t = Table::with_rows(3);
        t.add_data_column("t", "s").unwrap();
        t.add_data_column("x", "").unwrap();
        for (r, (a, b)) in [("0", "1,5"), ("0,1", ""), ("0,2", "-3e-5")]
            .iter()
            .enumerate()
        {
            t.set_cell_text(0, r, a).unwrap();
            t.set_cell_text(1, r, b).unwrap();
        }
        t.add_computed_column("y", "m", "t*2").unwrap();

        for fmt in [CsvFormat::FRENCH, CsvFormat::INTERNATIONAL] {
            let text = export_csv(&t, fmt);
            let back = import_csv(&text).unwrap();
            assert!(back.warnings.is_empty(), "{:?}", back.warnings);
            assert_eq!(back.delimiter, fmt.delimiter);
            let b = &back.table;
            assert_eq!((b.rows(), b.column_count()), (3, 3));
            assert_eq!(b.column(0).unwrap().unit(), "s");
            assert_eq!(b.column(2).unwrap().unit(), "m");
            for c in 0..3 {
                for r in 0..3 {
                    assert_eq!(b.cell(c, r), t.cell(c, r), "col {c} row {r}");
                }
            }
        }
        let french = export_csv(&t, CsvFormat::FRENCH);
        assert!(
            french.starts_with("t (s);x;y (m)\n0;1,5;0\n0,1;;0,2\n"),
            "{french}"
        );
    }

    #[test]
    fn export_quotes_units_with_delimiters() {
        let mut t = Table::with_rows(0);
        t.add_data_column("v", "m;s").unwrap();
        let text = export_csv(&t, CsvFormat::FRENCH);
        assert_eq!(text, "\"v (m;s)\"\n");
    }
}

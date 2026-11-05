//! Open-ExAO en ligne de commande : même noyau que l'application graphique.
//!
//! ```text
//! openexao <fichier.csv|fichier.exao|--demo> [--calc "nom=formule"]... [--out fichier] [--intl]
//! ```

use std::process::ExitCode;

use openexao_core::csv::{CsvFormat, export_csv, import_csv};
use openexao_core::format::format_number;
use openexao_core::{Cell, Project, Table, sample};

const USAGE: &str = "\
Open-ExAO (preuve de concept) : tableur de mesures

Utilisation :
  openexao <fichier.csv | fichier.exao | --demo> [options]

Options :
  --calc \"nom=formule\"   ajoute une colonne calculée (répétable), ex. --calc \"v=y/t\"
  --out <fichier>        écrit le résultat : .exao (projet) ou .csv
  --intl                 CSV international (séparateur « , » et point décimal)
  --help                 affiche cette aide

Les formules utilisent le point décimal et les noms des colonnes :
  + - * / ^ ( ), sqrt abs sin cos tan asin acos atan atan2 exp ln log10 pow min max, pi";

struct Options {
    input: Option<String>,
    demo: bool,
    calcs: Vec<String>,
    out: Option<String>,
    intl: bool,
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut opts = Options {
        input: None,
        demo: false,
        calcs: Vec::new(),
        out: None,
        intl: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--demo" => opts.demo = true,
            "--intl" => opts.intl = true,
            "--calc" => opts
                .calcs
                .push(args.next().ok_or("--calc attend « nom=formule »")?),
            "--out" => opts.out = Some(args.next().ok_or("--out attend un nom de fichier")?),
            other if other.starts_with("--") => return Err(format!("option inconnue : {other}")),
            other => {
                if opts.input.replace(other.to_string()).is_some() {
                    return Err("un seul fichier d'entrée est accepté".into());
                }
            }
        }
    }
    if opts.input.is_none() && !opts.demo {
        return Ok(None);
    }
    Ok(Some(opts))
}

fn load(opts: &Options) -> Result<(String, Table), String> {
    if opts.demo {
        return Ok(("Chute libre (démo)".into(), sample::chute_libre()));
    }
    let path = opts.input.as_deref().unwrap_or_default();
    let text = std::fs::read_to_string(path).map_err(|e| format!("lecture de {path} : {e}"))?;
    if path.ends_with(".exao") {
        let project = Project::from_json_str(&text).map_err(|e| e.to_string())?;
        Ok((project.title, project.table))
    } else {
        let imported = import_csv(&text).map_err(|e| e.to_string())?;
        for warning in &imported.warnings {
            eprintln!("avertissement : {warning}");
        }
        Ok((path.to_string(), imported.table))
    }
}

fn print_table(title: &str, table: &Table) {
    println!(
        "{title}  ({} lignes × {} colonnes)",
        table.rows(),
        table.column_count()
    );
    let headers: Vec<String> = table
        .columns()
        .iter()
        .map(|c| match (c.unit().is_empty(), c.formula()) {
            (true, None) => c.name().to_string(),
            (false, None) => format!("{} ({})", c.name(), c.unit()),
            (true, Some(f)) => format!("{} = {f}", c.name()),
            (false, Some(f)) => format!("{} ({}) = {f}", c.name(), c.unit()),
        })
        .collect();
    let rows: Vec<Vec<String>> = (0..table.rows())
        .map(|r| {
            (0..table.column_count())
                .map(|c| match table.cell(c, r) {
                    Some(Cell::Num(x)) => format_number(*x, 6, true),
                    Some(Cell::Err(e)) => e.label().to_string(),
                    _ => String::new(),
                })
                .collect()
        })
        .collect();
    let widths: Vec<usize> = (0..headers.len())
        .map(|c| {
            rows.iter()
                .map(|row| row[c].chars().count())
                .chain(std::iter::once(headers[c].chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |cells: &[String]| {
        cells
            .iter()
            .zip(&widths)
            .map(|(cell, w)| format!("{cell:>w$}", w = *w))
            .collect::<Vec<_>>()
            .join(" │ ")
    };
    println!("{}", line(&headers));
    println!(
        "{}",
        widths
            .iter()
            .map(|w| "─".repeat(*w))
            .collect::<Vec<_>>()
            .join("─┼─")
    );
    for row in &rows {
        println!("{}", line(row));
    }
    for column in table.columns() {
        if let Some(message) = column.formula_error() {
            println!("⚠ colonne « {} » : {message}", column.name());
        }
    }
}

fn run() -> Result<(), String> {
    let Some(opts) = parse_args()? else {
        println!("{USAGE}");
        return Ok(());
    };
    let (title, mut table) = load(&opts)?;
    for spec in &opts.calcs {
        let (name, formula) = spec
            .split_once('=')
            .ok_or_else(|| format!("--calc « {spec} » : forme attendue nom=formule"))?;
        table
            .add_computed_column(name.trim(), "", formula)
            .map_err(|e| format!("--calc « {spec} » : {e}"))?;
    }
    print_table(&title, &table);
    if let Some(out) = &opts.out {
        let content = if out.ends_with(".exao") {
            Project::new(&title, table).to_json_string()
        } else {
            let format = if opts.intl {
                CsvFormat::INTERNATIONAL
            } else {
                CsvFormat::FRENCH
            };
            export_csv(&table, format)
        };
        std::fs::write(out, content).map_err(|e| format!("écriture de {out} : {e}"))?;
        println!("écrit : {out}");
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("erreur : {message}");
            ExitCode::FAILURE
        }
    }
}

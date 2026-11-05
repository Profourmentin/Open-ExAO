//! État de l'application : le projet ouvert et les commandes qui le modifient.
//!
//! Cette couche ne contient aucune règle de calcul : elle traduit des commandes JSON
//! en appels au noyau (`openexao-core`) et décrit le résultat pour l'affichage.
//! Une interface native (egui, par exemple) appellerait directement le noyau de la
//! même façon.

use openexao_core::csv::{CsvFormat, export_csv, import_csv};
use openexao_core::format::format_number;
use openexao_core::json::Json;
use openexao_core::{Cell, Project, Table, sample};

const DEFAULT_TITLE: &str = "Sans titre";
const MAX_TITLE_CHARS: usize = 120;
/// Chiffres significatifs affichés dans les cellules (la valeur exacte reste en mémoire).
const DISPLAY_DIGITS: usize = 6;

pub struct Session {
    title: String,
    table: Table,
}

/// Formats de téléchargement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Project,
    CsvFrench,
    CsvInternational,
}

impl ExportKind {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "exao" => Some(ExportKind::Project),
            "csv" => Some(ExportKind::CsvFrench),
            "csv-intl" => Some(ExportKind::CsvInternational),
            _ => None,
        }
    }
}

pub struct Download {
    pub filename: String,
    pub mime: &'static str,
    pub content: String,
}

fn obj(pairs: Vec<(&str, Json)>) -> Json {
    Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn text<'a>(command: &'a Json, key: &str) -> Result<&'a str, String> {
    command
        .get(key)
        .and_then(Json::as_str)
        .ok_or_else(|| format!("commande invalide : champ « {key} » manquant"))
}

fn index(command: &Json, key: &str) -> Result<usize, String> {
    command
        .get(key)
        .and_then(Json::as_f64)
        .filter(|v| v.fract() == 0.0 && *v >= 0.0 && *v <= u32::MAX as f64)
        .map(|v| v as usize)
        .ok_or_else(|| format!("commande invalide : champ « {key} » manquant ou invalide"))
}

/// Nom de fichier sans dossier ni extension.
fn file_stem(name: &str) -> &str {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    base.rsplit_once('.').map_or(base, |(stem, _)| stem)
}

fn clean_title(title: &str) -> String {
    let title: String = title
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TITLE_CHARS)
        .collect();
    let title = title.trim();
    if title.is_empty() {
        DEFAULT_TITLE.to_string()
    } else {
        title.to_string()
    }
}

/// Nom de fichier sûr pour l'en-tête de téléchargement : ASCII, sans séparateur.
fn download_stem(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        let c = if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            c
        } else {
            '_'
        };
        if c != '_' || !out.ends_with('_') {
            out.push(c);
        }
    }
    let out = out.trim_matches('_');
    if out.is_empty() {
        "projet".to_string()
    } else {
        out.to_string()
    }
}

impl Session {
    pub fn new() -> Self {
        Session::from_table(DEFAULT_TITLE, blank_table())
    }

    pub fn demo() -> Self {
        Session::from_table("Chute libre (exemple)", sample::chute_libre())
    }

    pub fn from_table(title: &str, table: Table) -> Self {
        Session {
            title: clean_title(title),
            table,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn table(&self) -> &Table {
        &self.table
    }

    /// Ouvre un fichier d'après son extension : `.exao` est un projet, tout le reste
    /// est lu comme un CSV. Renvoie les avertissements à montrer à l'utilisateur.
    /// Si le fichier est refusé, la session n'est pas modifiée.
    pub fn open_file(&mut self, name: &str, content: &str) -> Result<Vec<String>, String> {
        if name.to_lowercase().ends_with(".exao") {
            let project = Project::from_json_str(content).map_err(|e| e.to_string())?;
            let title = if project.title.trim().is_empty() {
                file_stem(name)
            } else {
                &project.title
            };
            self.title = clean_title(title);
            self.table = project.table;
            Ok(Vec::new())
        } else {
            let imported = import_csv(content).map_err(|e| e.to_string())?;
            self.title = clean_title(file_stem(name));
            self.table = imported.table;
            Ok(imported.warnings)
        }
    }

    /// Décrit l'état complet pour l'affichage.
    pub fn snapshot(&self) -> Json {
        let t = &self.table;
        let columns = t
            .columns()
            .iter()
            .map(|c| {
                obj(vec![
                    ("name", Json::Str(c.name().to_string())),
                    ("unit", Json::Str(c.unit().to_string())),
                    (
                        "formula",
                        c.formula().map_or(Json::Null, |f| Json::Str(f.to_string())),
                    ),
                    (
                        "error",
                        c.formula_error()
                            .map_or(Json::Null, |e| Json::Str(e.to_string())),
                    ),
                ])
            })
            .collect();
        let cells = (0..t.column_count())
            .map(|col| {
                Json::Arr(
                    (0..t.rows())
                        .map(|row| {
                            let (display, kind) = match t.cell(col, row) {
                                Some(Cell::Num(x)) => {
                                    (format_number(*x, DISPLAY_DIGITS, true), "n")
                                }
                                Some(Cell::Err(e)) => (e.label().to_string(), "e"),
                                _ => (String::new(), "z"),
                            };
                            obj(vec![
                                ("d", Json::Str(display)),
                                ("e", Json::Str(t.cell_text(col, row))),
                                ("k", Json::Str(kind.to_string())),
                            ])
                        })
                        .collect(),
                )
            })
            .collect();
        obj(vec![
            ("title", Json::Str(self.title.clone())),
            ("rows", Json::Num(t.rows() as f64)),
            ("columns", Json::Arr(columns)),
            ("cells", Json::Arr(cells)),
        ])
    }

    /// Noms des colonnes dont la formule est cassée (nom inconnu, cycle, syntaxe).
    fn broken_formulas(&self) -> Vec<String> {
        self.table
            .columns()
            .iter()
            .filter(|c| c.formula_error().is_some())
            .map(|c| c.name().to_string())
            .collect()
    }

    /// Applique une commande. En cas d'erreur, le tableau n'est pas modifié.
    /// Renvoie les messages d'information à montrer à l'utilisateur.
    pub fn apply(&mut self, command: &Json) -> Result<Vec<String>, String> {
        let mut messages = Vec::new();
        let tbl = |e: openexao_core::TableError| e.to_string();
        match text(command, "op")? {
            "set_cell" => {
                let (col, row) = (index(command, "col")?, index(command, "row")?);
                self.table
                    .set_cell_text(col, row, text(command, "text")?)
                    .map_err(tbl)?;
            }
            "paste" => {
                let (col, row) = (index(command, "col")?, index(command, "row")?);
                let block = command
                    .get("block")
                    .and_then(Json::as_array)
                    .ok_or("commande invalide : champ « block » manquant")?
                    .iter()
                    .map(|line| {
                        line.as_array()
                            .ok_or("commande invalide : « block » doit être une liste de lignes")?
                            .iter()
                            .map(|cell| {
                                cell.as_str().map(str::to_string).ok_or(
                                    "commande invalide : les cellules de « block » sont du texte",
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.table.paste(col, row, &block).map_err(tbl)?;
            }
            "add_row" => {
                self.table.add_row();
            }
            "remove_row" => self.table.remove_row(index(command, "row")?).map_err(tbl)?,
            "add_data_column" => {
                self.table
                    .add_data_column(text(command, "name")?, text(command, "unit")?)
                    .map_err(tbl)?;
            }
            "add_computed_column" => {
                self.table
                    .add_computed_column(
                        text(command, "name")?,
                        text(command, "unit")?,
                        text(command, "formula")?,
                    )
                    .map_err(tbl)?;
            }
            "remove_column" => {
                self.table
                    .remove_column(index(command, "col")?)
                    .map_err(tbl)?;
                self.warn_broken(&mut messages);
            }
            "rename_column" => {
                self.table
                    .rename_column(index(command, "col")?, text(command, "name")?)
                    .map_err(tbl)?;
                self.warn_broken(&mut messages);
            }
            "set_unit" => self
                .table
                .set_unit(index(command, "col")?, text(command, "unit")?)
                .map_err(tbl)?,
            "set_formula" => {
                self.table
                    .set_formula(index(command, "col")?, text(command, "formula")?)
                    .map_err(tbl)?;
            }
            "set_title" => self.title = clean_title(text(command, "title")?),
            "new" => *self = Session::new(),
            "demo" => *self = Session::demo(),
            "open" => {
                messages = self.open_file(text(command, "name")?, text(command, "content")?)?;
            }
            other => return Err(format!("commande inconnue : « {other} »")),
        }
        Ok(messages)
    }

    fn warn_broken(&self, messages: &mut Vec<String>) {
        let broken = self.broken_formulas();
        if !broken.is_empty() {
            messages.push(format!("Formule à corriger dans : {}", broken.join(", ")));
        }
    }

    pub fn export(&self, kind: ExportKind) -> Download {
        let stem = download_stem(&self.title);
        match kind {
            ExportKind::Project => Download {
                filename: format!("{stem}.exao"),
                mime: "application/json; charset=utf-8",
                content: Project::new(&self.title, self.table.clone()).to_json_string(),
            },
            ExportKind::CsvFrench => Download {
                filename: format!("{stem}.csv"),
                mime: "text/csv; charset=utf-8",
                content: export_csv(&self.table, CsvFormat::FRENCH),
            },
            ExportKind::CsvInternational => Download {
                filename: format!("{stem}.csv"),
                mime: "text/csv; charset=utf-8",
                content: export_csv(&self.table, CsvFormat::INTERNATIONAL),
            },
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

/// Tableau de départ : deux colonnes de données et dix lignes vides.
fn blank_table() -> Table {
    let mut table = Table::with_rows(10);
    table.add_data_column("x", "").expect("nom valide");
    table.add_data_column("y", "").expect("nom valide");
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(json: &str) -> Json {
        Json::parse(json).unwrap()
    }

    fn display(s: &Session, col: usize, row: usize) -> String {
        let snapshot = s.snapshot();
        let cells = snapshot.get("cells").unwrap().as_array().unwrap();
        let cell = &cells[col].as_array().unwrap()[row];
        cell.get("d").unwrap().as_str().unwrap().to_string()
    }

    #[test]
    fn snapshot_describes_columns_and_cells() {
        let s = Session::demo();
        let snap = s.snapshot();
        assert_eq!(snap.get("rows").unwrap().as_f64(), Some(11.0));
        let columns = snap.get("columns").unwrap().as_array().unwrap();
        assert_eq!(columns.len(), 4);
        assert_eq!(columns[0].get("name").unwrap().as_str(), Some("t"));
        assert_eq!(columns[0].get("unit").unwrap().as_str(), Some("s"));
        assert!(columns[0].get("formula").unwrap().is_null());
        assert_eq!(
            columns[2].get("formula").unwrap().as_str(),
            Some("0.5*9.81*t^2")
        );
        // 0,5 * 9,81 * 1² = 4,905
        assert_eq!(display(&s, 2, 10), "4,905");
        assert_eq!(display(&s, 0, 1), "0,1");
    }

    #[test]
    fn editing_a_cell_recomputes_and_errors_leave_the_table_unchanged() {
        let mut s = Session::demo();
        s.apply(&cmd(r#"{"op":"set_cell","col":0,"row":10,"text":"2"}"#))
            .unwrap();
        assert_eq!(display(&s, 2, 10), "19,62");

        let err = s
            .apply(&cmd(r#"{"op":"set_cell","col":0,"row":10,"text":"abc"}"#))
            .unwrap_err();
        assert!(err.contains("n'est pas un nombre"), "{err}");
        assert_eq!(display(&s, 0, 10), "2");
        let err = s
            .apply(&cmd(r#"{"op":"set_cell","col":2,"row":0,"text":"1"}"#))
            .unwrap_err();
        assert!(err.contains("colonne est calculée"), "{err}");
    }

    #[test]
    fn invalid_commands_are_refused_with_a_reason() {
        let mut s = Session::new();
        assert!(
            s.apply(&cmd(r#"{"op":"nope"}"#))
                .unwrap_err()
                .contains("inconnue")
        );
        assert!(s.apply(&cmd(r#"{}"#)).unwrap_err().contains("« op »"));
        assert!(
            s.apply(&cmd(r#"{"op":"set_cell","col":-1,"row":0,"text":""}"#))
                .unwrap_err()
                .contains("« col »")
        );
        assert!(
            s.apply(&cmd(r#"{"op":"set_cell","col":0.5,"row":0,"text":""}"#))
                .is_err()
        );
        assert!(
            s.apply(&cmd(r#"{"op":"paste","col":0,"row":0,"block":[[1]]}"#))
                .unwrap_err()
                .contains("texte")
        );
        assert!(
            s.apply(&cmd(r#"{"op":"paste","col":0,"row":0,"block":"x"}"#))
                .is_err()
        );
    }

    #[test]
    fn columns_can_be_added_renamed_and_removed_with_warnings() {
        let mut s = Session::demo();
        s.apply(&cmd(
            r#"{"op":"add_computed_column","name":"v","unit":"m/s","formula":"y/t"}"#,
        ))
        .unwrap();
        assert_eq!(display(&s, 4, 1), "0,5");
        // renommer « y » casse « v », « y_th » n'est pas touchée, « ecart » (y - y_th) non plus
        let messages = s
            .apply(&cmd(r#"{"op":"rename_column","col":1,"name":"hauteur"}"#))
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert!(
            messages[0].contains('v') && messages[0].contains("ecart"),
            "{messages:?}"
        );
        s.apply(&cmd(
            r#"{"op":"set_formula","col":4,"formula":"hauteur/t"}"#,
        ))
        .unwrap();
        s.apply(&cmd(
            r#"{"op":"set_formula","col":3,"formula":"hauteur - y_th"}"#,
        ))
        .unwrap();
        assert!(s.broken_formulas().is_empty());

        let err = s
            .apply(&cmd(r#"{"op":"add_data_column","name":"t","unit":""}"#))
            .unwrap_err();
        assert!(err.contains("déjà"), "{err}");
        let err = s
            .apply(&cmd(r#"{"op":"add_data_column","name":"1x","unit":""}"#))
            .unwrap_err();
        assert!(err.contains("invalide"), "{err}");

        let messages = s.apply(&cmd(r#"{"op":"remove_column","col":0}"#)).unwrap();
        assert!(messages[0].contains("y_th"), "{messages:?}"); // dépendait de « t »
        s.apply(&cmd(r#"{"op":"set_unit","col":0,"unit":"cm"}"#))
            .unwrap();
        assert_eq!(s.table().column(0).unwrap().unit(), "cm");
    }

    #[test]
    fn rows_and_paste() {
        let mut s = Session::new(); // 10 lignes, colonnes x et y
        s.apply(&cmd(r#"{"op":"add_row"}"#)).unwrap();
        assert_eq!(s.table().rows(), 11);
        s.apply(&cmd(r#"{"op":"remove_row","row":0}"#)).unwrap();
        assert!(s.apply(&cmd(r#"{"op":"remove_row","row":99}"#)).is_err());
        s.apply(&cmd(
            r#"{"op":"paste","col":0,"row":0,"block":[["1,5","2"],["3","4"]]}"#,
        ))
        .unwrap();
        assert_eq!(display(&s, 0, 0), "1,5");
        assert_eq!(display(&s, 1, 1), "4");
        let err = s
            .apply(&cmd(
                r#"{"op":"paste","col":0,"row":0,"block":[["9","x"]]}"#,
            ))
            .unwrap_err();
        assert!(err.contains("« x »"), "{err}");
        assert_eq!(display(&s, 0, 0), "1,5"); // rien n'a été écrit
    }

    #[test]
    fn open_csv_and_project_and_refuse_bad_files() {
        let mut s = Session::demo();
        let messages = s
            .apply(&cmd(
                r#"{"op":"open","name":"C:\\mesures\\pendule.csv","content":"t (s);theta (rad)\r\n0;0,3\r\n0,5;abc\r\n"}"#,
            ))
            .unwrap();
        assert_eq!(s.title(), "pendule");
        assert_eq!(s.table().rows(), 2);
        assert_eq!(messages.len(), 1, "{messages:?}"); // « abc » signalé
        assert_eq!(display(&s, 0, 1), "0,5");

        // aller-retour par le fichier de projet
        let saved = s.export(ExportKind::Project);
        assert_eq!(saved.filename, "pendule.exao");
        let mut other = Session::new();
        let open = Json::Obj(vec![
            ("op".into(), Json::Str("open".into())),
            ("name".into(), Json::Str("x.exao".into())),
            ("content".into(), Json::Str(saved.content)),
        ]);
        other.apply(&open).unwrap();
        assert_eq!(other.title(), "pendule");
        assert_eq!(other.table().rows(), 2);

        // un fichier refusé laisse tout en place
        let before = other.snapshot();
        assert!(
            other
                .apply(&cmd(r#"{"op":"open","name":"a.exao","content":"{}"}"#))
                .unwrap_err()
                .contains("pas un projet")
        );
        assert!(
            other
                .apply(&cmd(r#"{"op":"open","name":"a.csv","content":""}"#))
                .is_err()
        );
        assert_eq!(other.snapshot(), before);
    }

    #[test]
    fn titles_and_download_names_are_cleaned() {
        let mut s = Session::new();
        s.apply(&cmd(
            r#"{"op":"set_title","title":"  Chute – été/2027 \u0007 "}"#,
        ))
        .unwrap();
        assert_eq!(s.title(), "Chute – été/2027");
        assert_eq!(s.export(ExportKind::CsvFrench).filename, "Chute_t_2027.csv");
        s.apply(&cmd(r#"{"op":"set_title","title":"   "}"#))
            .unwrap();
        assert_eq!(s.title(), "Sans titre");
        s.apply(&cmd(r#"{"op":"set_title","title":"é"}"#)).unwrap();
        assert_eq!(s.export(ExportKind::Project).filename, "projet.exao");
        let long = "a".repeat(500);
        let mut s2 = Session::new();
        s2.apply(&Json::Obj(vec![
            ("op".into(), Json::Str("set_title".into())),
            ("title".into(), Json::Str(long)),
        ]))
        .unwrap();
        assert_eq!(s2.title().chars().count(), MAX_TITLE_CHARS);
    }

    #[test]
    fn exports_use_the_requested_csv_dialect() {
        let s = Session::demo();
        let french = s.export(ExportKind::CsvFrench);
        assert!(
            french
                .content
                .starts_with("t (s);y (m);y_th (m);ecart (m)\n0;0;0;0\n")
        );
        assert!(french.content.contains("\n0,1;0,05;"));
        let intl = s.export(ExportKind::CsvInternational);
        assert!(intl.content.starts_with("t (s),y (m),y_th (m),ecart (m)\n"));
        assert!(intl.content.contains("\n0.1,0.05,"));
        assert_eq!(
            ExportKind::parse("csv-intl"),
            Some(ExportKind::CsvInternational)
        );
        assert_eq!(ExportKind::parse("zip"), None);
    }

    #[test]
    fn new_and_demo_reset_the_session() {
        let mut s = Session::demo();
        s.apply(&cmd(r#"{"op":"new"}"#)).unwrap();
        assert_eq!((s.table().rows(), s.table().column_count()), (10, 2));
        assert_eq!(s.title(), "Sans titre");
        s.apply(&cmd(r#"{"op":"demo"}"#)).unwrap();
        assert_eq!(s.table().rows(), 11);
    }
}

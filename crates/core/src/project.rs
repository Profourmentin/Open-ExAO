//! Fichier de projet `.exao` : un document JSON versionné.
//!
//! ```json
//! {
//!   "format": "open-exao",
//!   "schema": 1,
//!   "title": "Chute libre",
//!   "table": {
//!     "rows": 3,
//!     "columns": [
//!       { "name": "t", "unit": "s", "values": [0, 0.1, null] },
//!       { "name": "y_th", "unit": "m", "formula": "0.5*9.81*t^2" }
//!     ]
//!   }
//! }
//! ```
//!
//! Seules les colonnes de données stockent leurs valeurs ; les colonnes calculées
//! sont recalculées à l'ouverture. Un fichier d'un `schema` plus récent que celui
//! que ce logiciel connaît est refusé avec un message explicite.

use std::fmt;

use crate::json::{Json, JsonError};
use crate::table::{Table, TableError};

pub const FORMAT_NAME: &str = "open-exao";
pub const SCHEMA_VERSION: u32 = 1;
/// Garde-fou contre les fichiers aberrants.
const MAX_ROWS: usize = 1_000_000;

#[derive(Debug, Clone)]
pub struct Project {
    pub title: String,
    pub table: Table,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectError {
    Json(JsonError),
    NotAProject,
    TooNew(u32),
    Invalid(String),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::Json(e) => write!(f, "{e}"),
            ProjectError::NotAProject => write!(f, "ce fichier n'est pas un projet Open-ExAO"),
            ProjectError::TooNew(v) => write!(
                f,
                "ce projet utilise une version de fichier plus récente (schéma {v}) que celle de ce logiciel (schéma {SCHEMA_VERSION}) : mettez Open-ExAO à jour"
            ),
            ProjectError::Invalid(m) => write!(f, "projet invalide : {m}"),
        }
    }
}

impl std::error::Error for ProjectError {}

impl From<JsonError> for ProjectError {
    fn from(e: JsonError) -> Self {
        ProjectError::Json(e)
    }
}

impl From<TableError> for ProjectError {
    fn from(e: TableError) -> Self {
        ProjectError::Invalid(e.to_string())
    }
}

impl Project {
    pub fn new(title: &str, table: Table) -> Self {
        Project {
            title: title.to_string(),
            table,
        }
    }

    pub fn to_json(&self) -> Json {
        let t = &self.table;
        let columns: Vec<Json> = t
            .columns()
            .iter()
            .enumerate()
            .map(|(ci, column)| {
                let mut fields = vec![
                    ("name".to_string(), Json::Str(column.name().to_string())),
                    ("unit".to_string(), Json::Str(column.unit().to_string())),
                ];
                match column.formula() {
                    Some(formula) => {
                        fields.push(("formula".to_string(), Json::Str(formula.to_string())))
                    }
                    None => {
                        let values = (0..t.rows())
                            .map(|row| match t.cell(ci, row) {
                                Some(crate::table::Cell::Num(x)) => Json::Num(*x),
                                _ => Json::Null,
                            })
                            .collect();
                        fields.push(("values".to_string(), Json::Arr(values)));
                    }
                }
                Json::Obj(fields)
            })
            .collect();
        Json::Obj(vec![
            ("format".to_string(), Json::Str(FORMAT_NAME.to_string())),
            ("schema".to_string(), Json::Num(f64::from(SCHEMA_VERSION))),
            ("title".to_string(), Json::Str(self.title.clone())),
            (
                "table".to_string(),
                Json::Obj(vec![
                    ("rows".to_string(), Json::Num(t.rows() as f64)),
                    ("columns".to_string(), Json::Arr(columns)),
                ]),
            ),
        ])
    }

    pub fn to_json_string(&self) -> String {
        self.to_json().to_string_pretty()
    }

    pub fn from_json_str(text: &str) -> Result<Project, ProjectError> {
        let root = Json::parse(text)?;
        if root.get("format").and_then(Json::as_str) != Some(FORMAT_NAME) {
            return Err(ProjectError::NotAProject);
        }
        let schema = root
            .get("schema")
            .and_then(Json::as_f64)
            .filter(|v| v.fract() == 0.0 && *v >= 1.0 && *v <= f64::from(u32::MAX))
            .ok_or_else(|| ProjectError::Invalid("numéro de schéma manquant ou invalide".into()))?
            as u32;
        if schema > SCHEMA_VERSION {
            return Err(ProjectError::TooNew(schema));
        }
        let title = root
            .get("title")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string();

        let table_json = root
            .get("table")
            .ok_or_else(|| ProjectError::Invalid("champ « table » manquant".into()))?;
        let rows = table_json
            .get("rows")
            .and_then(Json::as_f64)
            .filter(|v| v.fract() == 0.0 && *v >= 0.0 && *v <= MAX_ROWS as f64)
            .ok_or_else(|| ProjectError::Invalid("nombre de lignes manquant ou invalide".into()))?
            as usize;
        let columns = table_json
            .get("columns")
            .and_then(Json::as_array)
            .ok_or_else(|| ProjectError::Invalid("liste « columns » manquante".into()))?;

        let mut table = Table::with_rows(rows);
        for (i, column) in columns.iter().enumerate() {
            let name = column
                .get("name")
                .and_then(Json::as_str)
                .ok_or_else(|| ProjectError::Invalid(format!("colonne {} sans nom", i + 1)))?;
            let unit = column.get("unit").and_then(Json::as_str).unwrap_or("");
            match column.get("formula") {
                Some(Json::Str(formula)) => {
                    table.add_computed_column(name, unit, formula)?;
                }
                Some(Json::Null) | None => {
                    let values =
                        column
                            .get("values")
                            .and_then(Json::as_array)
                            .ok_or_else(|| {
                                ProjectError::Invalid(format!(
                                    "colonne « {name} » sans valeurs ni formule"
                                ))
                            })?;
                    if values.len() != rows {
                        return Err(ProjectError::Invalid(format!(
                            "colonne « {name} » : {} valeurs pour {rows} lignes",
                            values.len()
                        )));
                    }
                    let index = table.add_data_column(name, unit)?;
                    for (row, value) in values.iter().enumerate() {
                        match value {
                            Json::Num(x) => table.set_data_cell(index, row, Some(*x)),
                            Json::Null => {}
                            _ => {
                                return Err(ProjectError::Invalid(format!(
                                    "colonne « {name} », ligne {} : nombre ou null attendu",
                                    row + 1
                                )));
                            }
                        }
                    }
                }
                Some(_) => {
                    return Err(ProjectError::Invalid(format!(
                        "colonne « {name} » : formule invalide"
                    )));
                }
            }
        }
        table.recompute();
        Ok(Project { title, table })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;
    use crate::table::Cell;

    #[test]
    fn round_trip_preserves_everything() {
        let project = Project::new("Chute \"libre\"", sample::chute_libre());
        let text = project.to_json_string();
        let back = Project::from_json_str(&text).unwrap();
        assert_eq!(back.title, project.title);
        let (a, b) = (&project.table, &back.table);
        assert_eq!((a.rows(), a.column_count()), (b.rows(), b.column_count()));
        for c in 0..a.column_count() {
            let (ca, cb) = (a.column(c).unwrap(), b.column(c).unwrap());
            assert_eq!(ca.name(), cb.name());
            assert_eq!(ca.unit(), cb.unit());
            assert_eq!(ca.formula(), cb.formula());
            for r in 0..a.rows() {
                assert_eq!(a.cell(c, r), b.cell(c, r), "col {c} row {r}");
            }
        }
    }

    #[test]
    fn computed_values_are_not_stored() {
        let text = Project::new("", sample::chute_libre()).to_json_string();
        assert!(text.contains("\"formula\": \"0.5*9.81*t^2\""), "{text}");
        let computed_block = text.split("\"name\": \"y_th\"").nth(1).unwrap();
        let until_next_column = computed_block.split('}').next().unwrap();
        assert!(!until_next_column.contains("values"), "{until_next_column}");
    }

    #[test]
    fn empty_cells_round_trip_as_null() {
        let mut t = Table::with_rows(2);
        t.add_data_column("a", "").unwrap();
        t.set_cell_text(0, 1, "7").unwrap();
        let text = Project::new("", t).to_json_string();
        assert!(text.contains("[null, 7]"), "{text}");
        let back = Project::from_json_str(&text).unwrap();
        assert_eq!(back.table.cell(0, 0), Some(&Cell::Empty));
        assert_eq!(back.table.cell(0, 1), Some(&Cell::Num(7.0)));
    }

    #[test]
    fn formulas_may_reference_later_columns() {
        let text = r#"{"format":"open-exao","schema":1,"table":{"rows":1,"columns":[
            {"name":"b","formula":"a+1"},{"name":"a","values":[2]}]}}"#;
        let p = Project::from_json_str(text).unwrap();
        assert_eq!(p.table.cell(0, 0), Some(&Cell::Num(3.0)));
    }

    #[test]
    fn refuses_foreign_and_newer_files() {
        assert_eq!(
            Project::from_json_str("{}").unwrap_err(),
            ProjectError::NotAProject
        );
        assert_eq!(
            Project::from_json_str(r#"{"format":"autre","schema":1}"#).unwrap_err(),
            ProjectError::NotAProject
        );
        assert_eq!(
            Project::from_json_str(r#"{"format":"open-exao","schema":2,"table":{}}"#).unwrap_err(),
            ProjectError::TooNew(2)
        );
        assert!(matches!(
            Project::from_json_str("pas du json"),
            Err(ProjectError::Json(_))
        ));
    }

    #[test]
    fn refuses_inconsistent_content() {
        let head = r#"{"format":"open-exao","schema":1,"table":"#;
        for body in [
            r#"{"rows":2,"columns":[{"name":"a","values":[1]}]}"#, // mauvaise longueur
            r#"{"rows":1,"columns":[{"name":"a","values":["x"]}]}"#, // valeur non numérique
            r#"{"rows":1,"columns":[{"name":"a"}]}"#,              // ni valeurs ni formule
            r#"{"rows":1,"columns":[{"name":"1a","values":[1]}]}"#, // nom invalide
            r#"{"rows":1,"columns":[{"name":"a","values":[1]},{"name":"a","values":[2]}]}"#, // doublon
            r#"{"rows":1,"columns":[{"name":"a","formula":""}]}"#, // formule vide
            r#"{"rows":1,"columns":[{"name":"a","formula":5}]}"#,  // formule de mauvais type
            r#"{"rows":-1,"columns":[]}"#,
            r#"{"rows":1.5,"columns":[]}"#,
            r#"{"rows":99999999,"columns":[]}"#,
            r#"{"columns":[]}"#,
            r#"{"rows":1}"#,
        ] {
            let text = format!("{head}{body}}}");
            assert!(
                matches!(Project::from_json_str(&text), Err(ProjectError::Invalid(_))),
                "devrait être refusé : {body}"
            );
        }
    }

    #[test]
    fn messages_are_in_french_and_actionable() {
        let err =
            Project::from_json_str(r#"{"format":"open-exao","schema":7,"table":{}}"#).unwrap_err();
        assert!(err.to_string().contains("mettez Open-ExAO à jour"));
    }
}

//! Noyau d'Open-ExAO (preuve de concept).
//!
//! Ce crate ne connaît ni l'interface graphique, ni la vidéo, ni le matériel :
//! il ne gère qu'un tableau de mesures avec colonnes calculées par formules.
//! Il n'utilise que la bibliothèque standard.
//!
//! - [`expr`] : lecture et évaluation des formules (`0.5*g*t^2`)
//! - [`table`] : tableau de colonnes nommées, colonnes de données et colonnes calculées
//! - [`csv`] : import et export CSV (séparateur et virgule décimale à la française)
//! - [`json`] et [`project`] : fichier de projet `.exao` (JSON versionné)
//! - [`format`] : lecture et affichage des nombres
//! - [`sample`] : jeu de données d'exemple

pub mod csv;
pub mod expr;
pub mod format;
pub mod json;
pub mod project;
pub mod sample;
pub mod table;

pub use expr::{CellError, Expr};
pub use project::Project;
pub use table::{Cell, Column, Table, TableError};

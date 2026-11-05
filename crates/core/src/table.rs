//! Tableau de mesures : colonnes nommées, colonnes de données et colonnes calculées.
//!
//! Une colonne de données contient des valeurs saisies ou importées. Une colonne
//! calculée est définie par une formule qui référence d'autres colonnes par leur
//! nom, ligne par ligne (`y_th = 0.5*9.81*t^2`). Le tableau se recalcule
//! automatiquement après chaque modification, dans l'ordre des dépendances.

use std::collections::HashMap;
use std::fmt;

use crate::expr::{CellError, EvalFail, Expr};
use crate::format::{format_exact, parse_number};

/// Valeur d'une cellule. Un nombre est toujours fini (jamais NaN ni infini).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cell {
    Empty,
    Num(f64),
    Err(CellError),
}

/// Erreur d'une opération sur le tableau.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableError {
    InvalidName(String),
    DuplicateName(String),
    OutOfRange,
    NotComputed,
    NotData,
    EmptyFormula,
    NotANumber(String),
    TooManyRows,
}

/// Nombre maximal de lignes qu'un collage peut atteindre.
pub const PASTE_MAX_ROWS: usize = 100_000;

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableError::InvalidName(n) => write!(
                f,
                "nom de colonne invalide « {n} » (lettres, chiffres et _, sans commencer par un chiffre)"
            ),
            TableError::DuplicateName(n) => write!(f, "une colonne s'appelle déjà « {n} »"),
            TableError::OutOfRange => write!(f, "colonne ou ligne inexistante"),
            TableError::NotComputed => write!(f, "cette colonne n'est pas une colonne calculée"),
            TableError::NotData => write!(f, "cette colonne est calculée : modifiez sa formule"),
            TableError::EmptyFormula => write!(f, "la formule est vide"),
            TableError::NotANumber(t) => write!(f, "« {t} » n'est pas un nombre"),
            TableError::TooManyRows => {
                write!(f, "le collage dépasse {PASTE_MAX_ROWS} lignes")
            }
        }
    }
}

impl std::error::Error for TableError {}

/// Un nom de colonne est un identifiant : lettre ou `_`, puis lettres, chiffres ou `_`.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}

#[derive(Debug, Clone)]
pub struct Column {
    name: String,
    unit: String,
    formula: Option<String>,
    compiled: Option<Expr>,
    formula_error: Option<String>,
    cells: Vec<Cell>,
}

impl Column {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn unit(&self) -> &str {
        &self.unit
    }

    pub fn is_computed(&self) -> bool {
        self.formula.is_some()
    }

    pub fn formula(&self) -> Option<&str> {
        self.formula.as_deref()
    }

    /// Message d'erreur de la formule (syntaxe, nom inconnu, dépendance circulaire).
    pub fn formula_error(&self) -> Option<&str> {
        self.formula_error.as_deref()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Table {
    columns: Vec<Column>,
    rows: usize,
}

impl Table {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_rows(rows: usize) -> Self {
        Table {
            columns: Vec::new(),
            rows,
        }
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    pub fn column(&self, index: usize) -> Option<&Column> {
        self.columns.get(index)
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }

    pub fn cell(&self, col: usize, row: usize) -> Option<&Cell> {
        self.columns.get(col)?.cells.get(row)
    }

    /// Texte d'une cellule pour l'édition : valeur exacte à virgule décimale, ou
    /// l'étiquette d'erreur. Vide pour une cellule vide.
    pub fn cell_text(&self, col: usize, row: usize) -> String {
        match self.cell(col, row) {
            Some(Cell::Num(x)) => format_exact(*x, true),
            Some(Cell::Err(e)) => e.label().to_string(),
            _ => String::new(),
        }
    }

    fn check_new_name(&self, name: &str, except: Option<usize>) -> Result<(), TableError> {
        if !is_valid_name(name) {
            return Err(TableError::InvalidName(name.to_string()));
        }
        let taken = self
            .columns
            .iter()
            .enumerate()
            .any(|(i, c)| c.name == name && Some(i) != except);
        if taken {
            return Err(TableError::DuplicateName(name.to_string()));
        }
        Ok(())
    }

    pub fn add_data_column(&mut self, name: &str, unit: &str) -> Result<usize, TableError> {
        self.check_new_name(name, None)?;
        self.columns.push(Column {
            name: name.to_string(),
            unit: unit.trim().to_string(),
            formula: None,
            compiled: None,
            formula_error: None,
            cells: vec![Cell::Empty; self.rows],
        });
        Ok(self.columns.len() - 1)
    }

    pub fn add_computed_column(
        &mut self,
        name: &str,
        unit: &str,
        formula: &str,
    ) -> Result<usize, TableError> {
        self.check_new_name(name, None)?;
        let formula = formula.trim();
        if formula.is_empty() {
            return Err(TableError::EmptyFormula);
        }
        self.columns.push(Column {
            name: name.to_string(),
            unit: unit.trim().to_string(),
            formula: Some(formula.to_string()),
            compiled: None,
            formula_error: None,
            cells: vec![Cell::Empty; self.rows],
        });
        self.recompute();
        Ok(self.columns.len() - 1)
    }

    pub fn remove_column(&mut self, index: usize) -> Result<(), TableError> {
        if index >= self.columns.len() {
            return Err(TableError::OutOfRange);
        }
        self.columns.remove(index);
        self.recompute();
        Ok(())
    }

    /// Renomme une colonne. Les formules ne sont pas réécrites : celles qui citaient
    /// l'ancien nom affichent « Nom inconnu » jusqu'à correction.
    pub fn rename_column(&mut self, index: usize, name: &str) -> Result<(), TableError> {
        if index >= self.columns.len() {
            return Err(TableError::OutOfRange);
        }
        self.check_new_name(name, Some(index))?;
        self.columns[index].name = name.to_string();
        self.recompute();
        Ok(())
    }

    pub fn set_unit(&mut self, index: usize, unit: &str) -> Result<(), TableError> {
        self.columns
            .get_mut(index)
            .ok_or(TableError::OutOfRange)?
            .unit = unit.trim().to_string();
        Ok(())
    }

    /// Change la formule d'une colonne calculée.
    pub fn set_formula(&mut self, index: usize, formula: &str) -> Result<(), TableError> {
        let column = self.columns.get_mut(index).ok_or(TableError::OutOfRange)?;
        if column.formula.is_none() {
            return Err(TableError::NotComputed);
        }
        let formula = formula.trim();
        if formula.is_empty() {
            return Err(TableError::EmptyFormula);
        }
        column.formula = Some(formula.to_string());
        self.recompute();
        Ok(())
    }

    pub fn add_row(&mut self) -> usize {
        for column in &mut self.columns {
            column.cells.push(Cell::Empty);
        }
        self.rows += 1;
        self.recompute();
        self.rows - 1
    }

    pub fn remove_row(&mut self, row: usize) -> Result<(), TableError> {
        if row >= self.rows {
            return Err(TableError::OutOfRange);
        }
        for column in &mut self.columns {
            column.cells.remove(row);
        }
        self.rows -= 1;
        self.recompute();
        Ok(())
    }

    /// Saisit une valeur dans une colonne de données. Texte vide : cellule vidée.
    /// Accepte `1,5`, `1.5`, `1 000,5`, `2e-3`.
    pub fn set_cell_text(&mut self, col: usize, row: usize, text: &str) -> Result<(), TableError> {
        if col >= self.columns.len() || row >= self.rows {
            return Err(TableError::OutOfRange);
        }
        if self.columns[col].is_computed() {
            return Err(TableError::NotData);
        }
        let cell = if text.trim().is_empty() {
            Cell::Empty
        } else {
            Cell::Num(
                parse_number(text)
                    .ok_or_else(|| TableError::NotANumber(text.trim().to_string()))?,
            )
        };
        self.columns[col].cells[row] = cell;
        self.recompute();
        Ok(())
    }

    /// Colle un bloc de texte (copié depuis un tableur) à partir de la cellule
    /// `(col, row)`. Le bloc peut dépasser le bas du tableau : les lignes manquantes
    /// sont créées. Une ligne plus courte que les autres laisse les cellules restantes
    /// intactes ; un texte vide vide la cellule.
    ///
    /// Tout est vérifié avant d'écrire : si une cellule du bloc n'est pas un nombre,
    /// ou vise une colonne calculée, ou sort des colonnes existantes, le tableau reste
    /// inchangé. Le recalcul n'a lieu qu'une fois, à la fin.
    pub fn paste(
        &mut self,
        col: usize,
        row: usize,
        block: &[Vec<String>],
    ) -> Result<(), TableError> {
        let width = block.iter().map(Vec::len).max().unwrap_or(0);
        if block.is_empty() || width == 0 {
            return Ok(());
        }
        if col.saturating_add(width) > self.columns.len() || row > self.rows {
            return Err(TableError::OutOfRange);
        }
        let end_row = row
            .checked_add(block.len())
            .filter(|&end| end <= PASTE_MAX_ROWS)
            .ok_or(TableError::TooManyRows)?;
        if self.columns[col..col + width]
            .iter()
            .any(Column::is_computed)
        {
            return Err(TableError::NotData);
        }
        let mut parsed: Vec<Vec<Option<Cell>>> = Vec::with_capacity(block.len());
        for line in block {
            let mut cells = Vec::with_capacity(width);
            for c in 0..width {
                cells.push(match line.get(c).map(|t| t.trim()) {
                    None => None,
                    Some("") => Some(Cell::Empty),
                    Some(text) => Some(Cell::Num(
                        parse_number(text)
                            .ok_or_else(|| TableError::NotANumber(text.to_string()))?,
                    )),
                });
            }
            parsed.push(cells);
        }
        for column in &mut self.columns {
            column.cells.resize(end_row.max(self.rows), Cell::Empty);
        }
        self.rows = self.rows.max(end_row);
        for (dr, cells) in parsed.into_iter().enumerate() {
            for (dc, cell) in cells.into_iter().enumerate() {
                if let Some(cell) = cell {
                    self.columns[col + dc].cells[row + dr] = cell;
                }
            }
        }
        self.recompute();
        Ok(())
    }

    /// Écrit une cellule de données sans recalculer (chargement de fichiers).
    /// Le code appelant doit appeler [`Table::recompute`] ensuite.
    pub(crate) fn set_data_cell(&mut self, col: usize, row: usize, value: Option<f64>) {
        if let Some(cell) = self.columns.get_mut(col).and_then(|c| c.cells.get_mut(row)) {
            *cell = value.map_or(Cell::Empty, Cell::Num);
        }
    }

    /// Recalcule toutes les colonnes calculées, dans l'ordre de leurs dépendances.
    pub fn recompute(&mut self) {
        let n = self.columns.len();
        let index: HashMap<String, usize> = self
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.clone(), i))
            .collect();

        let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut schedulable = vec![false; n];

        // 1. Analyse des formules et recherche des dépendances.
        for i in 0..n {
            let Some(formula) = self.columns[i].formula.clone() else {
                continue;
            };
            match Expr::parse(&formula) {
                Err(e) => {
                    self.mark_failed(i, format!("Syntaxe : {e}"), CellError::Syntax);
                }
                Ok(expr) => {
                    let unknown: Vec<&str> = expr
                        .variables()
                        .into_iter()
                        .filter(|v| !index.contains_key(*v) && *v != "pi")
                        .collect();
                    if !unknown.is_empty() {
                        let message = format!("Nom inconnu : {}", unknown.join(", "));
                        self.mark_failed(i, message, CellError::UnknownName);
                    } else {
                        deps[i] = expr
                            .variables()
                            .into_iter()
                            .filter_map(|v| index.get(v).copied())
                            .collect();
                        let column = &mut self.columns[i];
                        column.compiled = Some(expr);
                        column.formula_error = None;
                        schedulable[i] = true;
                    }
                }
            }
        }

        // 2. Ordre topologique (Kahn) sur les colonnes dont la formule est correcte.
        let mut indegree = vec![0usize; n];
        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];
        for i in 0..n {
            if !schedulable[i] {
                continue;
            }
            for &d in &deps[i] {
                if schedulable[d] {
                    indegree[i] += 1;
                    dependents[d].push(i);
                }
            }
        }
        let mut queue: Vec<usize> = (0..n)
            .filter(|&i| schedulable[i] && indegree[i] == 0)
            .collect();
        let mut done = vec![false; n];
        let mut head = 0;
        while head < queue.len() {
            let i = queue[head];
            head += 1;
            let cells = self.evaluate_column(i, &index);
            self.columns[i].cells = cells;
            done[i] = true;
            for &j in &dependents[i] {
                indegree[j] -= 1;
                if indegree[j] == 0 {
                    queue.push(j);
                }
            }
        }

        // 3. Ce qui reste est dans un cycle ou en dépend.
        for i in 0..n {
            if schedulable[i] && !done[i] {
                self.mark_failed(i, "Dépendance circulaire".to_string(), CellError::Cycle);
            }
        }
    }

    fn mark_failed(&mut self, i: usize, message: String, error: CellError) {
        let rows = self.rows;
        let column = &mut self.columns[i];
        column.compiled = None;
        column.formula_error = Some(message);
        column.cells = vec![Cell::Err(error); rows];
    }

    fn evaluate_column(&self, i: usize, index: &HashMap<String, usize>) -> Vec<Cell> {
        let Some(expr) = self.columns[i].compiled.as_ref() else {
            return vec![Cell::Err(CellError::Syntax); self.rows];
        };
        (0..self.rows)
            .map(|row| {
                let lookup = |name: &str| {
                    index
                        .get(name)
                        .map(|&ci| match self.columns[ci].cells[row] {
                            Cell::Num(x) => Ok(x),
                            Cell::Empty => Err(EvalFail::Empty),
                            Cell::Err(e) => Err(EvalFail::Error(e)),
                        })
                };
                match expr.eval(&lookup) {
                    Ok(x) => Cell::Num(x),
                    Err(EvalFail::Empty) => Cell::Empty,
                    Err(EvalFail::Error(e)) => Cell::Err(e),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Table {
        let mut t = Table::with_rows(3);
        t.add_data_column("t", "s").unwrap();
        for (row, v) in ["0", "1", "2"].iter().enumerate() {
            t.set_cell_text(0, row, v).unwrap();
        }
        t
    }

    fn num(t: &Table, col: usize, row: usize) -> f64 {
        match t.cell(col, row) {
            Some(Cell::Num(x)) => *x,
            other => panic!("cellule ({col},{row}) = {other:?}"),
        }
    }

    #[test]
    fn computed_column_follows_data() {
        let mut t = sample();
        let y = t.add_computed_column("y", "m", "0.5*10*t^2").unwrap();
        assert_eq!(num(&t, y, 2), 20.0);
        t.set_cell_text(0, 2, "3").unwrap();
        assert_eq!(num(&t, y, 2), 45.0);
    }

    #[test]
    fn dependency_order_does_not_depend_on_column_order() {
        let mut t = sample();
        // « b » est déclarée avant « a » mais dépend d'elle
        let b = t.add_computed_column("b", "", "a+1").unwrap();
        assert_eq!(t.cell(b, 0), Some(&Cell::Err(CellError::UnknownName)));
        let a = t.add_computed_column("a", "", "t*2").unwrap();
        assert_eq!(num(&t, a, 1), 2.0);
        assert_eq!(num(&t, b, 1), 3.0);
        assert_eq!(t.column(b).unwrap().formula_error(), None);
    }

    #[test]
    fn empty_cells_propagate_as_empty() {
        let mut t = sample();
        let y = t.add_computed_column("y", "", "t*2").unwrap();
        t.set_cell_text(0, 1, "").unwrap();
        assert_eq!(t.cell(y, 1), Some(&Cell::Empty));
        assert_eq!(num(&t, y, 2), 4.0);
    }

    #[test]
    fn errors_are_per_cell_and_propagate() {
        let mut t = sample();
        let inv = t.add_computed_column("inv", "", "1/t").unwrap();
        let twice = t.add_computed_column("twice", "", "inv*2").unwrap();
        assert_eq!(t.cell(inv, 0), Some(&Cell::Err(CellError::DivByZero)));
        assert_eq!(t.cell(twice, 0), Some(&Cell::Err(CellError::DivByZero)));
        assert_eq!(num(&t, inv, 1), 1.0);
        assert_eq!(num(&t, twice, 2), 1.0);
    }

    #[test]
    fn syntax_and_unknown_name_errors_are_reported() {
        let mut t = sample();
        let c = t.add_computed_column("c", "", "t +").unwrap();
        let err = t.column(c).unwrap().formula_error().unwrap();
        assert!(err.starts_with("Syntaxe"), "{err}");
        assert_eq!(t.cell(c, 0), Some(&Cell::Err(CellError::Syntax)));

        t.set_formula(c, "t + zz").unwrap();
        assert_eq!(
            t.column(c).unwrap().formula_error(),
            Some("Nom inconnu : zz")
        );

        t.set_formula(c, "t + 1").unwrap();
        assert_eq!(t.column(c).unwrap().formula_error(), None);
        assert_eq!(num(&t, c, 0), 1.0);
    }

    #[test]
    fn cycles_are_detected() {
        let mut t = sample();
        let a = t.add_computed_column("a", "", "b+1").unwrap();
        let b = t.add_computed_column("b", "", "a+1").unwrap();
        let c = t.add_computed_column("c", "", "b*2").unwrap(); // dépend du cycle
        let d = t.add_computed_column("d", "", "d").unwrap(); // se cite lui-même
        for col in [a, b, c, d] {
            assert_eq!(
                t.cell(col, 0),
                Some(&Cell::Err(CellError::Cycle)),
                "col {col}"
            );
            assert_eq!(
                t.column(col).unwrap().formula_error(),
                Some("Dépendance circulaire")
            );
        }
        // casser le cycle répare tout
        t.set_formula(a, "t+1").unwrap();
        assert_eq!(num(&t, b, 1), 3.0);
        assert_eq!(num(&t, c, 1), 6.0);
    }

    #[test]
    fn names_are_validated() {
        let mut t = sample();
        assert_eq!(
            t.add_data_column("t", ""),
            Err(TableError::DuplicateName("t".into()))
        );
        assert!(matches!(
            t.add_data_column("1x", ""),
            Err(TableError::InvalidName(_))
        ));
        assert!(matches!(
            t.add_data_column("a b", ""),
            Err(TableError::InvalidName(_))
        ));
        assert!(matches!(
            t.add_data_column("", ""),
            Err(TableError::InvalidName(_))
        ));
        assert!(t.add_data_column("Δt_2", "s").is_ok());
        assert!(t.rename_column(0, "Δt_2").is_err());
        assert!(t.rename_column(0, "temps").is_ok());
        assert_eq!(t.column_index("temps"), Some(0));
    }

    #[test]
    fn renaming_breaks_formulas_visibly() {
        let mut t = sample();
        let y = t.add_computed_column("y", "", "t*2").unwrap();
        t.rename_column(0, "temps").unwrap();
        assert_eq!(
            t.column(y).unwrap().formula_error(),
            Some("Nom inconnu : t")
        );
        t.set_formula(y, "temps*2").unwrap();
        assert_eq!(num(&t, y, 2), 4.0);
    }

    #[test]
    fn rows_and_columns_can_be_added_and_removed() {
        let mut t = sample();
        let y = t.add_computed_column("y", "", "t*2").unwrap();
        let row = t.add_row();
        assert_eq!(row, 3);
        assert_eq!(t.rows(), 4);
        assert_eq!(t.cell(y, 3), Some(&Cell::Empty));
        t.set_cell_text(0, 3, "10").unwrap();
        assert_eq!(num(&t, y, 3), 20.0);
        t.remove_row(0).unwrap();
        assert_eq!(t.rows(), 3);
        assert_eq!(num(&t, y, 0), 2.0);
        assert!(t.remove_row(9).is_err());
        t.remove_column(0).unwrap(); // supprime « t » : y devient invalide
        assert_eq!(
            t.column(0).unwrap().formula_error(),
            Some("Nom inconnu : t")
        );
        assert!(t.remove_column(5).is_err());
    }

    #[test]
    fn cell_input_rules() {
        let mut t = sample();
        let y = t.add_computed_column("y", "", "t").unwrap();
        assert_eq!(
            t.set_cell_text(0, 0, "abc"),
            Err(TableError::NotANumber("abc".into()))
        );
        assert_eq!(
            t.set_cell_text(0, 0, "inf"),
            Err(TableError::NotANumber("inf".into()))
        );
        assert_eq!(t.set_cell_text(y, 0, "1"), Err(TableError::NotData));
        assert_eq!(t.set_cell_text(0, 99, "1"), Err(TableError::OutOfRange));
        assert_eq!(t.set_formula(0, "1"), Err(TableError::NotComputed));
        assert_eq!(t.set_formula(y, "  "), Err(TableError::EmptyFormula));
        t.set_cell_text(0, 0, "1,25").unwrap();
        assert_eq!(t.cell_text(0, 0), "1,25");
        assert_eq!(t.cell_text(0, 1), "1");
    }

    fn block(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|r| r.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    #[test]
    fn paste_fills_a_block_grows_the_table_and_recomputes_once() {
        let mut t = sample(); // t = 0, 1, 2
        let x = t.add_data_column("x", "m").unwrap();
        let y = t.add_computed_column("y", "", "x*2").unwrap();
        // 4 lignes à partir de la ligne 2 : une ligne existante, trois nouvelles
        t.paste(x, 2, &block(&[&["1,5"], &["2"], &[""], &["4"]]))
            .unwrap();
        assert_eq!(t.rows(), 6);
        assert_eq!(num(&t, x, 2), 1.5);
        assert_eq!(num(&t, y, 2), 3.0);
        assert_eq!(t.cell(y, 4), Some(&Cell::Empty)); // cellule collée vide
        assert_eq!(num(&t, y, 5), 8.0);
        assert_eq!(t.cell(0, 5), Some(&Cell::Empty)); // « t » n'a pas été touchée
    }

    #[test]
    fn paste_two_columns_and_ragged_lines() {
        let mut t = sample();
        let x = t.add_data_column("x", "").unwrap();
        t.set_cell_text(x, 0, "9").unwrap();
        t.set_cell_text(x, 1, "9").unwrap();
        t.paste(0, 0, &block(&[&["10", "1"], &["20"]])).unwrap();
        assert_eq!(num(&t, 0, 0), 10.0);
        assert_eq!(num(&t, x, 0), 1.0);
        assert_eq!(num(&t, 0, 1), 20.0);
        assert_eq!(num(&t, x, 1), 9.0); // ligne trop courte : cellule intacte
    }

    #[test]
    fn paste_is_all_or_nothing() {
        let mut t = sample();
        let y = t.add_computed_column("y", "", "t").unwrap();
        let before: Vec<Cell> = (0..3).map(|r| *t.cell(0, r).unwrap()).collect();

        assert_eq!(
            t.paste(0, 0, &block(&[&["7"], &["abc"]])),
            Err(TableError::NotANumber("abc".into()))
        );
        assert_eq!(
            t.paste(0, 0, &block(&[&["7", "8"]])),
            Err(TableError::NotData)
        );
        assert_eq!(t.paste(y, 0, &block(&[&["7"]])), Err(TableError::NotData));
        assert_eq!(
            t.paste(5, 0, &block(&[&["7"]])),
            Err(TableError::OutOfRange)
        );
        assert_eq!(
            t.paste(0, 9, &block(&[&["7"]])),
            Err(TableError::OutOfRange)
        );
        let too_tall = vec![vec!["1".to_string()]; PASTE_MAX_ROWS + 1];
        assert_eq!(t.paste(0, 0, &too_tall), Err(TableError::TooManyRows));
        let after: Vec<Cell> = (0..3).map(|r| *t.cell(0, r).unwrap()).collect();
        assert_eq!(before, after);
        assert_eq!(t.rows(), 3);
        // un bloc vide ne fait rien
        assert_eq!(t.paste(0, 0, &[]), Ok(()));
        assert_eq!(t.paste(0, 0, &block(&[&[]])), Ok(()));
    }

    #[test]
    fn paste_can_append_right_below_the_last_row() {
        let mut t = sample();
        t.paste(0, 3, &block(&[&["3"]])).unwrap(); // row == rows : autorisé
        assert_eq!(t.rows(), 4);
        assert_eq!(num(&t, 0, 3), 3.0);
    }

    #[test]
    fn cell_text_shows_error_labels() {
        let mut t = sample();
        let inv = t.add_computed_column("inv", "", "1/t").unwrap();
        assert_eq!(t.cell_text(inv, 0), "#DIV/0");
    }
}

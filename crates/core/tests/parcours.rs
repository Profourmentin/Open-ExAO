//! Parcours complet d'un utilisateur : importer un CSV, ajouter des colonnes calculées,
//! enregistrer le projet, le rouvrir, exporter.

use std::path::PathBuf;

use openexao_core::csv::{CsvFormat, export_csv, import_csv};
use openexao_core::{Cell, Project, sample};

fn example(name: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "..", "examples", name]
        .iter()
        .collect();
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn example_csv_matches_the_built_in_sample() {
    let imported = import_csv(&example("chute_libre.csv")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let built_in = sample::chute_libre();
    assert_eq!(imported.table.rows(), built_in.rows());
    for col in 0..2 {
        for row in 0..built_in.rows() {
            assert_eq!(imported.table.cell(col, row), built_in.cell(col, row));
        }
    }
}

#[test]
fn import_compute_save_reload_export() {
    let mut table = import_csv(&example("chute_libre.csv")).unwrap().table;
    table
        .add_computed_column("y_th", "m", "0.5*9.81*t^2")
        .unwrap();
    table
        .add_computed_column("ecart_cm", "cm", "(y - y_th)*100")
        .unwrap();
    // dépend de deux colonnes, dont une calculée ; vaut 0/0 à la première ligne
    let ratio = table.add_computed_column("ratio", "", "y / y_th").unwrap();
    assert_eq!(
        table.cell(ratio, 0),
        Some(&Cell::Err(openexao_core::CellError::DivByZero))
    );

    let saved = Project::new("Chute libre", table).to_json_string();
    let reloaded = Project::from_json_str(&saved).unwrap();
    assert_eq!(reloaded.title, "Chute libre");
    let t = &reloaded.table;
    assert_eq!(t.column_count(), 5);
    assert_eq!(
        t.cell(ratio, 0),
        Some(&Cell::Err(openexao_core::CellError::DivByZero))
    );
    match t.cell(ratio, 10) {
        Some(Cell::Num(r)) => assert!((r - 4.9 / (0.5 * 9.81)).abs() < 1e-12),
        other => panic!("{other:?}"),
    }

    let csv = export_csv(t, CsvFormat::FRENCH);
    assert!(
        csv.starts_with("t (s);y (m);y_th (m);ecart_cm (cm);ratio\n0;0;0;0;\n"),
        "{csv}"
    );
}

#[test]
fn editing_after_reload_recomputes() {
    let project = Project::new("", sample::chute_libre());
    let mut table = Project::from_json_str(&project.to_json_string())
        .unwrap()
        .table;
    let y_th = table.column_index("y_th").unwrap();
    let before = table.cell(y_th, 10).copied();
    table.set_cell_text(0, 10, "2").unwrap(); // t = 2 s au lieu de 1 s
    assert_ne!(table.cell(y_th, 10).copied(), before);
    assert_eq!(table.cell(y_th, 10), Some(&Cell::Num(0.5 * 9.81 * 4.0)));
}

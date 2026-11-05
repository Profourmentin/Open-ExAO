//! Jeu de données d'exemple : chute libre d'une bille filmée (pointage vidéo simulé).

use crate::table::Table;

/// Position `y` (en m) d'une bille en chute libre, relevée tous les 0,1 s, avec la
/// colonne théorique `y_th = 0.5*g*t²` et l'écart mesure-théorie.
pub fn chute_libre() -> Table {
    const T: [&str; 11] = [
        "0", "0,1", "0,2", "0,3", "0,4", "0,5", "0,6", "0,7", "0,8", "0,9", "1",
    ];
    const Y: [&str; 11] = [
        "0", "0,05", "0,2", "0,44", "0,79", "1,22", "1,77", "2,4", "3,14", "3,97", "4,9",
    ];
    let mut table = Table::with_rows(T.len());
    let t = table.add_data_column("t", "s").expect("nom valide");
    let y = table.add_data_column("y", "m").expect("nom valide");
    for (row, (t_text, y_text)) in T.iter().zip(Y.iter()).enumerate() {
        table.set_cell_text(t, row, t_text).expect("nombre valide");
        table.set_cell_text(y, row, y_text).expect("nombre valide");
    }
    table
        .add_computed_column("y_th", "m", "0.5*9.81*t^2")
        .expect("formule valide");
    table
        .add_computed_column("ecart", "m", "y - y_th")
        .expect("formule valide");
    table
}

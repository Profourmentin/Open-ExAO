# Open-ExAO

Logiciel libre d'expérimentation assistée par ordinateur pour la physique-chimie au lycée
(en projet). **État actuel : preuve de concept d'un tableur de mesures**, sans graphique,
vidéo ni acquisition. Nom provisoire ; licence à choisir.

Ce que fait le prototype :

- tableau de mesures avec colonnes de données et **colonnes calculées** par formules
  (`0.5*9.81*t^2`, `y - y_th`), recalculées automatiquement ;
- import et export **CSV à la française** (séparateur `;`, virgule décimale, compatible Excel
  et LibreOffice) ou international ;
- fichier de projet `.exao` (JSON versionné) ;
- collage de blocs de cellules copiés depuis un tableur ;
- deux interfaces sur le même noyau : un tableur dans le navigateur, servi depuis votre
  ordinateur, et une ligne de commande.

Aucune donnée ne quitte l'ordinateur et aucune bibliothèque externe n'est nécessaire.

## Lancer le tableur

1. Installer Rust (une fois) : <https://rustup.rs>
2. Dans le dossier du projet :

```text
cargo run --release -p openexao-app
```

Le navigateur s'ouvre sur le tableur (sinon, ouvrir l'adresse affichée dans le terminal).
Arrêt : `Ctrl+C` dans le terminal.

Options : un fichier à ouvrir (`.csv` ou `.exao`), `--port N`, `--no-open`.

```text
cargo run --release -p openexao-app -- examples/chute_libre.csv
```

## Ligne de commande

```text
cargo run -q -p openexao-cli -- examples/chute_libre.csv --calc "v=sqrt(2*9.81*y)" --out resultat.exao
```

Formules : nombres (point décimal), noms de colonnes, `+ - * / ^`, parenthèses,
`sqrt abs sin cos tan asin acos atan atan2 exp ln log10 pow min max`, `pi`.

## Vérifier

```text
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Pour aller plus loin

- [Architecture, décisions à trancher et limites connues](docs/ARCHITECTURE.md)
- Écrit et testé sous Linux. Windows et macOS : à essayer.

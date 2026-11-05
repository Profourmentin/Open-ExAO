//! Lance Open-ExAO : un serveur local et le tableur dans votre navigateur.
//!
//! ```text
//! openexao-app [fichier.csv|fichier.exao] [--port N] [--no-open]
//! ```

use std::process::{Command, ExitCode, Stdio};

use openexao_app::{Server, Session};

const USAGE: &str = "\
Open-ExAO (preuve de concept) : un tableur de mesures dans votre navigateur

Utilisation :
  openexao-app [fichier] [options]

  fichier        un CSV ou un projet .exao à ouvrir au démarrage
                 (sans fichier : l'exemple « chute libre »)

Options :
  --port N       port d'écoute sur 127.0.0.1 (par défaut : un port libre)
  --no-open      n'ouvre pas le navigateur, affiche seulement l'adresse
  --help         affiche cette aide

Arrêt : Ctrl+C dans ce terminal.";

struct Options {
    file: Option<String>,
    port: u16,
    open_browser: bool,
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut opts = Options {
        file: None,
        port: 0,
        open_browser: true,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--no-open" => opts.open_browser = false,
            "--port" => {
                opts.port = args
                    .next()
                    .and_then(|p| p.parse().ok())
                    .ok_or("--port attend un numéro de port (0 à 65535)")?;
            }
            other if other.starts_with("--") => return Err(format!("option inconnue : {other}")),
            other => {
                if opts.file.replace(other.to_string()).is_some() {
                    return Err("un seul fichier est accepté".into());
                }
            }
        }
    }
    Ok(Some(opts))
}

/// Ouvre l'adresse dans le navigateur par défaut, si possible. Un échec est sans
/// gravité : l'adresse est aussi affichée dans le terminal.
fn open_in_browser(url: &str) {
    let mut command = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    let _ = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn run() -> Result<(), String> {
    let Some(opts) = parse_args()? else {
        println!("{USAGE}");
        return Ok(());
    };
    let mut session = Session::demo();
    if let Some(path) = &opts.file {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("lecture de {path} : {e}"))?;
        for warning in session.open_file(path, &content)? {
            eprintln!("avertissement : {warning}");
        }
    }
    let server = Server::bind(opts.port, session)
        .map_err(|e| format!("impossible d'écouter sur 127.0.0.1:{} : {e}", opts.port))?;
    let url = server.url();
    println!("Open-ExAO est prêt.\n\n  {url}\n\nArrêt : Ctrl+C dans ce terminal.");
    if opts.open_browser {
        open_in_browser(&url);
    }
    server.serve().map_err(|e| e.to_string())
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

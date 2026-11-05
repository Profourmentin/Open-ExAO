//! Serveur local : sert la page du tableur et une petite API JSON.
//!
//! Sécurité (le serveur modifie des données et lit des fichiers que le navigateur
//! lui envoie, donc une page web quelconque ne doit pas pouvoir lui parler) :
//! - écoute sur `127.0.0.1` uniquement ;
//! - un jeton secret aléatoire, généré à chaque lancement, est exigé pour tout :
//!   dans l'adresse (`?t=`) pour la page et les téléchargements, dans l'en-tête
//!   `X-Exao-Token` pour l'API ;
//! - l'en-tête `Host` doit être `127.0.0.1:port` ou `localhost:port` (contre le
//!   « DNS rebinding ») ;
//! - aucun chemin de fichier n'est accepté de l'extérieur : l'ouverture envoie le
//!   contenu du fichier, l'enregistrement est un téléchargement du navigateur.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, BufReader};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use openexao_core::json::Json;

use crate::http::{self, Request, Response};
use crate::session::{ExportKind, Session};

const INDEX_HTML: &str = include_str!("ui.html");
const TOKEN_PLACEHOLDER: &str = "__EXAO_TOKEN__";
const IO_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_CONNECTIONS: usize = 64;

struct Shared {
    token: String,
    port: u16,
    session: Mutex<Session>,
    active: AtomicUsize,
}

impl Shared {
    fn session(&self) -> MutexGuard<'_, Session> {
        // Une panique dans un autre fil ne doit pas bloquer l'application : on reprend
        // l'état tel quel (les commandes ne laissent jamais le tableau à moitié modifié).
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct Server {
    listener: TcpListener,
    shared: Arc<Shared>,
}

/// Jeton de 128 bits tiré de l'aléa du système (`RandomState` est initialisé par lui).
fn random_token() -> String {
    let mut token = String::with_capacity(32);
    for round in 0..2u8 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u8(round);
        token.push_str(&format!("{:016x}", hasher.finish()));
    }
    token
}

/// Comparaison en temps constant (le jeton ne doit pas se deviner octet par octet).
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

impl Server {
    /// Écoute sur `127.0.0.1`. Le port 0 laisse le système choisir un port libre.
    pub fn bind(port: u16, session: Session) -> io::Result<Server> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        Ok(Server {
            listener,
            shared: Arc::new(Shared {
                token: random_token(),
                port,
                session: Mutex::new(session),
                active: AtomicUsize::new(0),
            }),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.shared.port))
    }

    pub fn token(&self) -> &str {
        &self.shared.token
    }

    /// Adresse à ouvrir dans le navigateur (elle contient le jeton).
    pub fn url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/?t={}",
            self.shared.port, self.shared.token
        )
    }

    /// Sert les connexions, indéfiniment.
    pub fn serve(self) -> io::Result<()> {
        for stream in self.listener.incoming() {
            let Ok(stream) = stream else { continue };
            let shared = Arc::clone(&self.shared);
            if shared.active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                shared.active.fetch_sub(1, Ordering::SeqCst);
                let _ = reply(&stream, &Response::text(503, "Trop de connexions."));
                continue;
            }
            let spawned = thread::Builder::new().spawn(move || {
                handle(&stream, &shared);
                shared.active.fetch_sub(1, Ordering::SeqCst);
            });
            if spawned.is_err() {
                self.shared.active.fetch_sub(1, Ordering::SeqCst);
            }
        }
        Ok(())
    }
}

fn reply(mut stream: &TcpStream, response: &Response) -> io::Result<()> {
    http::write_response(&mut stream, response)
}

fn handle(stream: &TcpStream, shared: &Shared) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let mut reader = BufReader::new(stream);
    let response = match http::read_request(&mut reader) {
        Ok(request) => route(&request, shared),
        Err(error) => match error.response() {
            Some(response) => response,
            None => return,
        },
    };
    let _ = reply(stream, &response);
}

fn host_allowed(request: &Request, port: u16) -> bool {
    matches!(
        request.header("Host"),
        Some(host) if host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
    )
}

fn forbidden() -> Response {
    Response::text(
        403,
        "Accès refusé. Ouvrez l'adresse complète affichée par Open-ExAO au démarrage.",
    )
}

fn route(request: &Request, shared: &Shared) -> Response {
    if !host_allowed(request, shared.port) {
        return forbidden();
    }
    let query_ok = request
        .query("t")
        .is_some_and(|t| same_token(t, &shared.token));
    let header_ok = request
        .header("X-Exao-Token")
        .is_some_and(|t| same_token(t, &shared.token));

    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") if query_ok => {
            Response::html(INDEX_HTML.replace(TOKEN_PLACEHOLDER, &shared.token))
        }
        ("GET", "/api/state") if header_ok => state_response(&shared.session(), &[]),
        ("POST", "/api/command") if header_ok => command_response(request, shared),
        ("GET", "/api/export") if query_ok => export_response(request, shared),
        // bonne méthode, mais jeton absent ou faux
        ("GET", "/" | "/api/state" | "/api/export") | ("POST", "/api/command") => forbidden(),
        ("GET", "/favicon.ico") => Response::new(204, "image/x-icon", Vec::new()),
        // route connue, mauvaise méthode
        (_, "/" | "/api/state" | "/api/command" | "/api/export") => {
            Response::text(405, "Méthode non autorisée.")
        }
        _ => Response::text(404, "Introuvable."),
    }
}

fn state_response(session: &Session, messages: &[String]) -> Response {
    let body = Json::Obj(vec![
        ("ok".into(), Json::Bool(true)),
        ("state".into(), session.snapshot()),
        (
            "messages".into(),
            Json::Arr(messages.iter().map(|m| Json::Str(m.clone())).collect()),
        ),
    ]);
    Response::json(200, &body.to_string_pretty())
}

fn command_response(request: &Request, shared: &Shared) -> Response {
    let Ok(body) = std::str::from_utf8(&request.body) else {
        return Response::text(400, "Le corps de la requête n'est pas de l'UTF-8.");
    };
    let Ok(command) = Json::parse(body) else {
        return Response::text(400, "Le corps de la requête n'est pas du JSON valide.");
    };
    let mut session = shared.session();
    match session.apply(&command) {
        Ok(messages) => state_response(&session, &messages),
        Err(error) => {
            let body = Json::Obj(vec![
                ("ok".into(), Json::Bool(false)),
                ("error".into(), Json::Str(error)),
            ]);
            Response::json(200, &body.to_string_pretty())
        }
    }
}

fn export_response(request: &Request, shared: &Shared) -> Response {
    let Some(kind) = request.query("kind").and_then(ExportKind::parse) else {
        return Response::text(400, "Format d'export inconnu.");
    };
    let download = shared.session().export(kind);
    Response::new(200, download.mime, download.content).with_header(
        "Content-Disposition",
        format!("attachment; filename=\"{}\"", download.filename),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_hex_and_compared_exactly() {
        let a = random_token();
        let b = random_token();
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert!(same_token(&a, &a));
        assert!(!same_token(&a, &b));
        assert!(!same_token(&a, &a[..31]));
        assert!(!same_token("", &a));
    }
}

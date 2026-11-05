//! Le serveur sur une vraie connexion TCP : page, API, téléchargements et refus.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread;

use openexao_app::{Server, Session};
use openexao_core::Project;
use openexao_core::json::Json;

struct Reply {
    status: u16,
    headers: String,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.lines().find_map(|line| {
            let (k, v) = line.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    fn json(&self) -> Json {
        Json::parse(&self.body).unwrap_or_else(|e| panic!("{e}: {}", self.body))
    }
}

struct Fixture {
    addr: SocketAddr,
    token: String,
}

fn start(session: Session) -> Fixture {
    let server = Server::bind(0, session).unwrap();
    let fixture = Fixture {
        addr: server.addr(),
        token: server.token().to_string(),
    };
    thread::spawn(move || server.serve());
    fixture
}

impl Fixture {
    fn raw(&self, request: &[u8]) -> Reply {
        let mut stream = TcpStream::connect(self.addr).unwrap();
        stream.write_all(request).unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").expect("réponse HTTP");
        let (status_line, headers) = head.split_once("\r\n").unwrap_or((head, ""));
        Reply {
            status: status_line.split(' ').nth(1).unwrap().parse().unwrap(),
            headers: headers.to_string(),
            body: body.to_string(),
        }
    }

    fn send(&self, method: &str, path: &str, extra: &[(&str, &str)], body: &str) -> Reply {
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: {}\r\n",
            self.addr.port(),
            body.len()
        );
        for (k, v) in extra {
            request.push_str(&format!("{k}: {v}\r\n"));
        }
        request.push_str("\r\n");
        request.push_str(body);
        self.raw(request.as_bytes())
    }

    fn get_api(&self, path: &str) -> Reply {
        self.send("GET", path, &[("X-Exao-Token", &self.token)], "")
    }

    fn command(&self, json: &str) -> Reply {
        self.send(
            "POST",
            "/api/command",
            &[("X-Exao-Token", &self.token)],
            json,
        )
    }
}

fn display(state: &Json, col: usize, row: usize) -> String {
    let cells = state.get("cells").unwrap().as_array().unwrap();
    let cell = &cells[col].as_array().unwrap()[row];
    cell.get("d").unwrap().as_str().unwrap().to_string()
}

#[test]
fn serves_the_page_only_with_the_token() {
    let f = start(Session::demo());
    let ok = f.send("GET", &format!("/?t={}", f.token), &[], "");
    assert_eq!(ok.status, 200);
    assert!(ok.header("content-type").unwrap().starts_with("text/html"));
    assert!(
        ok.body.contains(&f.token),
        "le jeton est injecté dans la page"
    );
    assert!(!ok.body.contains("__EXAO_TOKEN__"));
    let csp = ok.header("content-security-policy").unwrap();
    assert!(csp.contains("frame-ancestors 'none'") && csp.contains("default-src 'none'"));

    assert_eq!(f.send("GET", "/", &[], "").status, 403);
    assert_eq!(f.send("GET", "/?t=faux", &[], "").status, 403);
    assert_eq!(
        f.send("GET", &format!("/?t={}x", f.token), &[], "").status,
        403
    );
}

#[test]
fn refuses_foreign_hosts_and_missing_token_on_the_api() {
    let f = start(Session::demo());
    // adresse correcte, mais un autre nom d'hôte (DNS rebinding)
    let rebind = format!(
        "GET /api/state HTTP/1.1\r\nHost: evil.example:{}\r\nX-Exao-Token: {}\r\n\r\n",
        f.addr.port(),
        f.token
    );
    assert_eq!(f.raw(rebind.as_bytes()).status, 403);
    let no_host = format!("GET /?t={} HTTP/1.1\r\n\r\n", f.token);
    assert_eq!(f.raw(no_host.as_bytes()).status, 403);
    // « localhost » est accepté
    let localhost = format!(
        "GET /api/state HTTP/1.1\r\nHost: localhost:{}\r\nX-Exao-Token: {}\r\n\r\n",
        f.addr.port(),
        f.token
    );
    assert_eq!(f.raw(localhost.as_bytes()).status, 200);

    assert_eq!(f.send("GET", "/api/state", &[], "").status, 403);
    assert_eq!(
        f.send("GET", "/api/state", &[("X-Exao-Token", "faux")], "")
            .status,
        403
    );
    assert_eq!(
        f.send("POST", "/api/command", &[], r#"{"op":"new"}"#)
            .status,
        403
    );
    // le jeton dans l'adresse ne suffit pas pour l'API (il faut l'en-tête)
    assert_eq!(
        f.send("GET", &format!("/api/state?t={}", f.token), &[], "")
            .status,
        403
    );
    // et l'en-tête ne suffit pas pour un téléchargement (il faut l'adresse)
    assert_eq!(f.get_api("/api/export?kind=csv").status, 403);
}

#[test]
fn unknown_routes_and_wrong_methods() {
    let f = start(Session::demo());
    assert_eq!(f.get_api("/nope").status, 404);
    // route réservée à POST, appelée en GET avec un jeton valide : mauvaise méthode
    assert_eq!(f.get_api("/api/command").status, 405);
    assert_eq!(f.send("PUT", "/api/state", &[], "").status, 405);
    assert_eq!(f.send("DELETE", "/", &[], "").status, 405);
    assert_eq!(f.send("GET", "/favicon.ico", &[], "").status, 204);
}

#[test]
fn edit_flow_over_http() {
    let f = start(Session::demo());
    let state = f.get_api("/api/state").json();
    assert_eq!(state.get("ok"), Some(&Json::Bool(true)));
    let state = state.get("state").unwrap().clone();
    assert_eq!(
        state.get("title").unwrap().as_str(),
        Some("Chute libre (exemple)")
    );
    assert_eq!(display(&state, 2, 10), "4,905");

    // modification acceptée : tout est recalculé par le noyau
    let reply = f.command(r#"{"op":"set_cell","col":0,"row":10,"text":"2"}"#);
    assert_eq!(reply.status, 200);
    assert_eq!(display(reply.json().get("state").unwrap(), 2, 10), "19,62");

    // modification refusée : réponse 200 avec ok:false, et rien n'a changé
    let reply = f
        .command(r#"{"op":"set_cell","col":0,"row":10,"text":"abc"}"#)
        .json();
    assert_eq!(reply.get("ok"), Some(&Json::Bool(false)));
    assert!(
        reply
            .get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("pas un nombre")
    );
    let state = f.get_api("/api/state").json();
    assert_eq!(display(state.get("state").unwrap(), 0, 10), "2");

    // collage d'un bloc, avec création de lignes
    let reply = f
        .command(r#"{"op":"paste","col":0,"row":11,"block":[["5","5"],["6","6"]]}"#)
        .json();
    let state = reply.get("state").unwrap();
    assert_eq!(state.get("rows").unwrap().as_f64(), Some(13.0));
    // la colonne calculée y_th = 0,5 * 9,81 * t² est recalculée pour les lignes créées :
    // à t = 6 s, 0,5 * 9,81 * 36 = 176,58 m
    assert_eq!(display(state, 2, 12), "176,58");
}

#[test]
fn bad_requests_are_refused_cleanly() {
    let f = start(Session::demo());
    assert_eq!(f.command("pas du json").status, 400);
    assert_eq!(f.command("").status, 400);
    let not_utf8 = f.send_bytes_command(&[0xff, 0xfe, 0xfd]);
    assert_eq!(not_utf8.status, 400);
    let unknown = f.command(r#"{"op":"supprimer_tout"}"#).json();
    assert_eq!(unknown.get("ok"), Some(&Json::Bool(false)));

    // corps trop gros : refusé avant d'être lu
    let huge = format!(
        "POST /api/command HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Exao-Token: {}\r\nContent-Length: 99999999\r\n\r\n",
        f.addr.port(),
        f.token
    );
    assert_eq!(f.raw(huge.as_bytes()).status, 413);
    // requête sans sens
    assert_eq!(f.raw(b"n'importe quoi\r\n\r\n").status, 400);
    let chunked = format!(
        "POST /api/command HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nTransfer-Encoding: chunked\r\n\r\n",
        f.addr.port()
    );
    assert_eq!(f.raw(chunked.as_bytes()).status, 501);

    // le serveur répond toujours après tous ces refus
    assert_eq!(f.get_api("/api/state").status, 200);
}

impl Fixture {
    fn send_bytes_command(&self, body: &[u8]) -> Reply {
        let mut request = format!(
            "POST /api/command HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Exao-Token: {}\r\nContent-Length: {}\r\n\r\n",
            self.addr.port(),
            self.token,
            body.len()
        )
        .into_bytes();
        request.extend_from_slice(body);
        self.raw(&request)
    }
}

#[test]
fn downloads_and_reopening_a_downloaded_project() {
    let f = start(Session::demo());
    let t = &f.token;

    let csv = f.send("GET", &format!("/api/export?kind=csv&t={t}"), &[], "");
    assert_eq!(csv.status, 200);
    assert!(csv.header("content-type").unwrap().starts_with("text/csv"));
    assert_eq!(
        csv.header("content-disposition"),
        Some("attachment; filename=\"Chute_libre_exemple.csv\"")
    );
    assert!(csv.body.starts_with("t (s);y (m);y_th (m);ecart (m)\n"));

    let intl = f.send("GET", &format!("/api/export?kind=csv-intl&t={t}"), &[], "");
    assert!(intl.body.contains("\n0.1,0.05,"));

    let exao = f.send("GET", &format!("/api/export?kind=exao&t={t}"), &[], "");
    assert_eq!(
        exao.header("content-disposition"),
        Some("attachment; filename=\"Chute_libre_exemple.exao\"")
    );
    let project = Project::from_json_str(&exao.body).unwrap();
    assert_eq!(project.table.rows(), 11);

    let unknown = f.send("GET", &format!("/api/export?kind=zip&t={t}"), &[], "");
    assert_eq!(unknown.status, 400);

    // rouvre le projet téléchargé dans une autre application : même contenu
    let other = start(Session::new());
    let open = Json::Obj(vec![
        ("op".into(), Json::Str("open".into())),
        ("name".into(), Json::Str("chute.exao".into())),
        ("content".into(), Json::Str(exao.body.clone())),
    ]);
    let reply = other.command(&open.to_string_pretty()).json();
    assert_eq!(reply.get("ok"), Some(&Json::Bool(true)));
    assert_eq!(display(reply.get("state").unwrap(), 2, 10), "4,905");
}

#[test]
fn several_clients_share_one_session_without_corruption() {
    let f = start(Session::new());
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let addr = f.addr;
            let token = f.token.clone();
            thread::spawn(move || {
                let client = Fixture { addr, token };
                for _ in 0..10 {
                    client.command(r#"{"op":"add_row"}"#);
                }
                let _ = i;
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let state = f.get_api("/api/state").json();
    // 10 lignes de départ + 8 × 10 ajoutées
    assert_eq!(
        state.get("state").unwrap().get("rows").unwrap().as_f64(),
        Some(90.0)
    );
}

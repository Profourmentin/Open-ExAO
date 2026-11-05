//! HTTP/1.1 minimal, juste ce qu'il faut pour servir une page et une petite API
//! sur la machine locale. Une requête par connexion (`Connection: close`).
//!
//! Tout est borné : taille de la ligne de requête, des en-têtes et du corps, pour
//! qu'un client défaillant ne puisse pas épuiser la mémoire.

use std::fmt;
use std::io::{self, BufRead, Read, Write};

const MAX_LINE: u64 = 8 * 1024;
const MAX_HEADERS: usize = 64;
/// Corps maximal accepté (un fichier CSV ou projet ouvert depuis le navigateur).
pub const MAX_BODY: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub enum HttpError {
    Io(io::Error),
    Malformed(&'static str),
    TooLarge,
    Unsupported(&'static str),
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpError::Io(e) => write!(f, "{e}"),
            HttpError::Malformed(m) => write!(f, "requête mal formée : {m}"),
            HttpError::TooLarge => write!(f, "requête trop volumineuse"),
            HttpError::Unsupported(m) => write!(f, "non pris en charge : {m}"),
        }
    }
}

impl std::error::Error for HttpError {}

impl From<io::Error> for HttpError {
    fn from(e: io::Error) -> Self {
        HttpError::Io(e)
    }
}

impl HttpError {
    /// Réponse à renvoyer au client, sauf pour une erreur d'entrée-sortie (connexion
    /// perdue : il n'y a plus personne à qui répondre).
    pub fn response(&self) -> Option<Response> {
        let (status, message) = match self {
            HttpError::Io(_) => return None,
            HttpError::Malformed(_) => (400, "Requête mal formée."),
            HttpError::TooLarge => (413, "Requête trop volumineuse."),
            HttpError::Unsupported(_) => (501, "Fonction non prise en charge."),
        };
        Some(Response::text(status, message))
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// Valeur d'un en-tête (nom insensible à la casse).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Valeur d'un paramètre de l'adresse (`?t=...`).
    pub fn query(&self, key: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Lit une ligne terminée par `\n`, au plus `MAX_LINE` octets.
fn read_line(reader: &mut impl BufRead) -> Result<String, HttpError> {
    let mut raw = Vec::new();
    reader.by_ref().take(MAX_LINE).read_until(b'\n', &mut raw)?;
    if raw.last() != Some(&b'\n') {
        return Err(if raw.len() as u64 >= MAX_LINE {
            HttpError::TooLarge
        } else {
            HttpError::Malformed("ligne incomplète")
        });
    }
    let line = String::from_utf8(raw).map_err(|_| HttpError::Malformed("texte non UTF-8"))?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if bytes.get(i + 1..i + 3).is_some() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(pair), String::new()),
        })
        .collect()
}

pub fn read_request(reader: &mut impl BufRead) -> Result<Request, HttpError> {
    let request_line = read_line(reader)?;
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(HttpError::Malformed("ligne de requête"));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(HttpError::Malformed("version HTTP"));
    }
    if !target.starts_with('/') {
        return Err(HttpError::Malformed("adresse"));
    }
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), parse_query(q)),
        None => (target.to_string(), Vec::new()),
    };

    let mut headers = Vec::new();
    loop {
        let line = read_line(reader)?;
        if line.is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADERS {
            return Err(HttpError::TooLarge);
        }
        let (name, value) = line
            .split_once(':')
            .ok_or(HttpError::Malformed("en-tête"))?;
        headers.push((name.trim().to_string(), value.trim().to_string()));
    }

    let mut request = Request {
        method: method.to_string(),
        path,
        query,
        headers,
        body: Vec::new(),
    };
    if request.header("Transfer-Encoding").is_some() {
        return Err(HttpError::Unsupported("Transfer-Encoding"));
    }
    if let Some(length) = request.header("Content-Length") {
        let length: usize = length
            .parse()
            .map_err(|_| HttpError::Malformed("Content-Length"))?;
        if length > MAX_BODY {
            return Err(HttpError::TooLarge);
        }
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body)?;
        request.body = body;
    }
    Ok(request)
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Response {
            status,
            content_type,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn text(status: u16, message: &str) -> Self {
        Response::new(status, "text/plain; charset=utf-8", message)
    }

    pub fn html(body: impl Into<Vec<u8>>) -> Self {
        Response::new(200, "text/html; charset=utf-8", body)
    }

    pub fn json(status: u16, body: &str) -> Self {
        Response::new(status, "application/json; charset=utf-8", body)
    }

    pub fn with_header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

pub fn write_response(out: &mut impl Write, response: &Response) -> io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\
         Content-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
         connect-src 'self'; img-src data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n",
        response.status,
        reason(response.status),
        response.content_type,
        response.body.len()
    );
    for (name, value) in &response.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    out.write_all(head.as_bytes())?;
    out.write_all(&response.body)?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(raw: &str) -> Result<Request, HttpError> {
        read_request(&mut Cursor::new(raw.as_bytes().to_vec()))
    }

    #[test]
    fn parses_a_get_with_query_and_headers() {
        let r = parse(
            "GET /api/export?kind=csv&t=ab%20c HTTP/1.1\r\nHost: localhost:1\r\nX-Test: 1\r\n\r\n",
        )
        .unwrap();
        assert_eq!(r.method, "GET");
        assert_eq!(r.path, "/api/export");
        assert_eq!(r.query("kind"), Some("csv"));
        assert_eq!(r.query("t"), Some("ab c"));
        assert_eq!(r.header("host"), Some("localhost:1"));
        assert_eq!(r.header("x-test"), Some("1"));
        assert!(r.body.is_empty());
    }

    #[test]
    fn reads_exactly_content_length_bytes() {
        let r = parse("POST /x HTTP/1.1\r\nContent-Length: 5\r\n\r\nhelloEXTRA").unwrap();
        assert_eq!(r.body, b"hello");
    }

    #[test]
    fn rejects_bad_requests() {
        assert!(matches!(
            parse("GARBAGE\r\n\r\n"),
            Err(HttpError::Malformed(_))
        ));
        assert!(matches!(
            parse("GET http://x/ HTTP/1.1\r\n\r\n"),
            Err(HttpError::Malformed(_))
        ));
        assert!(matches!(
            parse("GET / HTTP/2\r\n\r\n"),
            Err(HttpError::Malformed(_))
        ));
        assert!(matches!(
            parse("GET / HTTP/1.1\r\nno-colon\r\n\r\n"),
            Err(HttpError::Malformed(_))
        ));
        assert!(matches!(
            parse("POST / HTTP/1.1\r\nContent-Length: abc\r\n\r\n"),
            Err(HttpError::Malformed(_))
        ));
        assert!(matches!(
            parse("POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n"),
            Err(HttpError::Unsupported(_))
        ));
        // corps annoncé plus long que ce qui arrive : erreur d'entrée-sortie, pas de blocage
        assert!(matches!(
            parse("POST / HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc"),
            Err(HttpError::Io(_))
        ));
        // ligne de requête sans fin
        assert!(matches!(
            parse("GET / HTTP/1.1"),
            Err(HttpError::Malformed(_))
        ));
    }

    #[test]
    fn enforces_size_limits() {
        let big = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(9000));
        assert!(matches!(parse(&big), Err(HttpError::TooLarge)));
        let huge = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert!(matches!(parse(&huge), Err(HttpError::TooLarge)));
        let many: String = (0..70).map(|i| format!("H{i}: v\r\n")).collect();
        assert!(matches!(
            parse(&format!("GET / HTTP/1.1\r\n{many}\r\n")),
            Err(HttpError::TooLarge)
        ));
    }

    #[test]
    fn response_has_length_and_security_headers() {
        let mut out = Vec::new();
        let r = Response::text(403, "non").with_header("X-Extra", "1");
        write_response(&mut out, &r).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("HTTP/1.1 403 Forbidden\r\n"));
        assert!(text.contains("Content-Length: 3\r\n"));
        assert!(text.contains("X-Content-Type-Options: nosniff\r\n"));
        assert!(text.contains("frame-ancestors 'none'"));
        assert!(text.contains("X-Extra: 1\r\n"));
        assert!(text.ends_with("\r\n\r\nnon"));
    }

    #[test]
    fn percent_decoding_is_lenient_and_safe() {
        assert_eq!(percent_decode("a%2Fb+c"), "a/b c");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("%e9"), "\u{fffd}"); // octet isolé non UTF-8
    }
}

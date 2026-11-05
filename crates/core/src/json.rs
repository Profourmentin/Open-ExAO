//! JSON minimal (lecture et écriture), sans dépendance externe.
//!
//! Suffisant pour le fichier de projet. Quand le projet gagnera des dépendances
//! (vidéo, archive zip), ce module pourra être remplacé par `serde_json` sans
//! changer le format du fichier.

use std::fmt;

const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    /// Objet : liste de paires pour conserver l'ordre d'écriture.
    Obj(Vec<(String, Json)>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    pub pos: usize,
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "JSON invalide au caractère {} : {}",
            self.pos + 1,
            self.message
        )
    }
}

impl std::error::Error for JsonError {}

impl Json {
    pub fn parse(text: &str) -> Result<Json, JsonError> {
        let mut p = Parser {
            chars: text.chars().collect(),
            i: 0,
        };
        p.skip_ws();
        let value = p.value(0)?;
        p.skip_ws();
        if p.i < p.chars.len() {
            return Err(p.err("texte inattendu après la valeur"));
        }
        Ok(value)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(x) => Some(*x),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    /// Écrit le JSON indenté ; les tableaux de valeurs simples restent sur une ligne.
    pub fn to_string_pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out.push('\n');
        out
    }

    fn is_scalar(&self) -> bool {
        !matches!(self, Json::Arr(_) | Json::Obj(_))
    }

    fn write(&self, out: &mut String, level: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(x) => {
                if x.is_finite() {
                    out.push_str(&format!("{x}"));
                } else {
                    out.push_str("null"); // le JSON ne sait pas écrire NaN ni l'infini
                }
            }
            Json::Str(s) => write_string(out, s),
            Json::Arr(items) if items.iter().all(Json::is_scalar) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write(out, level);
                }
                out.push(']');
            }
            Json::Arr(items) => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    indent(out, level + 1);
                    item.write(out, level + 1);
                    out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
                }
                indent(out, level);
                out.push(']');
            }
            Json::Obj(pairs) if pairs.is_empty() => out.push_str("{}"),
            Json::Obj(pairs) => {
                out.push_str("{\n");
                for (i, (key, value)) in pairs.iter().enumerate() {
                    indent(out, level + 1);
                    write_string(out, key);
                    out.push_str(": ");
                    value.write(out, level + 1);
                    out.push_str(if i + 1 < pairs.len() { ",\n" } else { "\n" });
                }
                indent(out, level);
                out.push('}');
            }
        }
    }
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser {
    chars: Vec<char>,
    i: usize,
}

impl Parser {
    fn err(&self, message: &str) -> JsonError {
        JsonError {
            pos: self.i,
            message: message.to_string(),
        }
    }

    fn skip_ws(&mut self) {
        while self.i < self.chars.len() && matches!(self.chars[self.i], ' ' | '\t' | '\n' | '\r') {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    fn expect_word(&mut self, word: &str, value: Json) -> Result<Json, JsonError> {
        for expected in word.chars() {
            if self.peek() != Some(expected) {
                return Err(self.err(&format!("« {word} » attendu")));
            }
            self.i += 1;
        }
        Ok(value)
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_DEPTH {
            return Err(self.err("imbrication trop profonde"));
        }
        match self.peek() {
            None => Err(self.err("valeur attendue")),
            Some('n') => self.expect_word("null", Json::Null),
            Some('t') => self.expect_word("true", Json::Bool(true)),
            Some('f') => self.expect_word("false", Json::Bool(false)),
            Some('"') => Ok(Json::Str(self.string()?)),
            Some('[') => self.array(depth),
            Some('{') => self.object(depth),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            Some(_) => Err(self.err("valeur attendue")),
        }
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.i;
        while self.i < self.chars.len()
            && matches!(self.chars[self.i], '0'..='9' | '-' | '+' | '.' | 'e' | 'E')
        {
            self.i += 1;
        }
        let text: String = self.chars[start..self.i].iter().collect();
        match text.parse::<f64>() {
            Ok(x) if x.is_finite() => Ok(Json::Num(x)),
            _ => {
                self.i = start;
                Err(self.err("nombre invalide"))
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut value = 0u32;
        for _ in 0..4 {
            let digit = self
                .peek()
                .and_then(|c| c.to_digit(16))
                .ok_or_else(|| self.err("séquence \\u invalide"))?;
            value = value * 16 + digit;
            self.i += 1;
        }
        Ok(value)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.i += 1; // guillemet ouvrant
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err("chaîne non terminée"));
            };
            self.i += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(e) = self.peek() else {
                        return Err(self.err("chaîne non terminée"));
                    };
                    self.i += 1;
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                // paire de substitution : \uD83D\uDE00
                                if self.peek() == Some('\\')
                                    && self.chars.get(self.i + 1) == Some(&'u')
                                {
                                    self.i += 2;
                                    let second = self.hex4()?;
                                    if !(0xDC00..0xE000).contains(&second) {
                                        return Err(self.err("séquence \\u invalide"));
                                    }
                                    0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                                } else {
                                    return Err(self.err("séquence \\u invalide"));
                                }
                            } else {
                                first
                            };
                            out.push(
                                char::from_u32(code)
                                    .ok_or_else(|| self.err("séquence \\u invalide"))?,
                            );
                        }
                        _ => return Err(self.err("échappement inconnu")),
                    }
                }
                c if (c as u32) < 0x20 => {
                    return Err(self.err("caractère de contrôle dans une chaîne"));
                }
                c => out.push(c),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.i += 1; // [
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.i += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.peek() {
                Some(',') => self.i += 1,
                Some(']') => {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(self.err("« , » ou « ] » attendu")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.i += 1; // {
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.i += 1;
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some('"') {
                return Err(self.err("nom de champ entre guillemets attendu"));
            }
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(':') {
                return Err(self.err("« : » attendu"));
            }
            self.i += 1;
            self.skip_ws();
            let value = self.value(depth + 1)?;
            pairs.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(',') => self.i += 1,
                Some('}') => {
                    self.i += 1;
                    return Ok(Json::Obj(pairs));
                }
                _ => return Err(self.err("« , » ou « } » attendu")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_value_kinds() {
        let j = Json::parse(r#" {"a": [1, 2.5, -3e2, null, true, false], "b": "x\ny", "c": {}} "#)
            .unwrap();
        let a = j.get("a").unwrap().as_array().unwrap();
        assert_eq!(a[0].as_f64(), Some(1.0));
        assert_eq!(a[2].as_f64(), Some(-300.0));
        assert!(a[3].is_null());
        assert_eq!(a[4], Json::Bool(true));
        assert_eq!(j.get("b").unwrap().as_str(), Some("x\ny"));
        assert_eq!(j.get("c"), Some(&Json::Obj(vec![])));
        assert_eq!(j.get("zz"), None);
    }

    #[test]
    fn string_escapes_and_unicode() {
        let j = Json::parse(r#""é\u00e9\"\\\/\t\ud83d\ude00""#).unwrap();
        assert_eq!(j.as_str(), Some("éé\"\\/\t😀"));
    }

    #[test]
    fn write_then_parse_is_identity() {
        let original = Json::Obj(vec![
            ("titre".into(), Json::Str("Chute \"libre\"\n\tΔt".into())),
            (
                "valeurs".into(),
                Json::Arr(vec![Json::Num(0.1), Json::Null, Json::Num(-2.5e-8)]),
            ),
            (
                "objets".into(),
                Json::Arr(vec![
                    Json::Obj(vec![("k".into(), Json::Bool(false))]),
                    Json::Obj(vec![]),
                ]),
            ),
            ("ctrl".into(), Json::Str("\u{1}".into())),
        ]);
        let text = original.to_string_pretty();
        assert_eq!(Json::parse(&text).unwrap(), original, "{text}");
        assert!(text.contains("[0.1, null, -0.000000025]"), "{text}");
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\":}",
            "{a:1}",
            "[1 2]",
            "\"abc",
            "01x",
            "nul",
            "tru",
            "{\"a\":1,}",
            "[1] x",
            "\"\\q\"",
            "\"\\ud83d\"",
            "1e999",
            "--1",
            "\"a\nb\"",
        ] {
            assert!(Json::parse(bad).is_err(), "devrait échouer : {bad:?}");
        }
    }

    #[test]
    fn deep_nesting_is_rejected_not_crashed() {
        let deep = "[".repeat(10_000);
        assert!(Json::parse(&deep).is_err());
    }

    #[test]
    fn non_finite_numbers_are_written_as_null() {
        assert_eq!(Json::Num(f64::NAN).to_string_pretty().trim(), "null");
    }
}

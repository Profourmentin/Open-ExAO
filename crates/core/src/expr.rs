//! Formules : analyse (parser) et évaluation d'expressions arithmétiques.
//!
//! Syntaxe : nombres (`3`, `0.5`, `1e-3`), noms de colonnes, `+ - * / ^`,
//! parenthèses, fonctions (`sqrt`, `abs`, `sin`, `cos`, `tan`, `asin`, `acos`,
//! `atan`, `atan2`, `exp`, `ln`, `log10` (alias `log`), `pow`, `min`, `max`)
//! et la constante `pi`. Le séparateur décimal des formules est le point.
//!
//! Précédences : `^` (associatif à droite) avant `*` `/` avant `+` `-`,
//! et `-2^2` vaut `-4`.

use std::fmt;

/// Profondeur maximale d'imbrication, pour éviter un dépassement de pile.
const MAX_DEPTH: usize = 100;

/// Erreur d'une cellule calculée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellError {
    DivByZero,
    Domain,
    Overflow,
    UnknownName,
    Cycle,
    Syntax,
}

impl CellError {
    /// Étiquette courte affichée dans la cellule.
    pub fn label(self) -> &'static str {
        match self {
            CellError::DivByZero => "#DIV/0",
            CellError::Domain => "#DOMAINE",
            CellError::Overflow => "#DEBORD",
            CellError::UnknownName => "#NOM",
            CellError::Cycle => "#CYCLE",
            CellError::Syntax => "#SYNTAXE",
        }
    }

    /// Explication en français.
    pub fn description(self) -> &'static str {
        match self {
            CellError::DivByZero => "division par zéro",
            CellError::Domain => {
                "valeur hors du domaine de la fonction (racine ou logarithme d'un nombre négatif, par exemple)"
            }
            CellError::Overflow => "résultat trop grand",
            CellError::UnknownName => "nom de colonne inconnu",
            CellError::Cycle => "dépendance circulaire entre colonnes",
            CellError::Syntax => "formule mal écrite",
        }
    }
}

/// Échec d'évaluation : une entrée vide (propagée silencieusement) ou une vraie erreur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalFail {
    Empty,
    Error(CellError),
}

/// Erreur de syntaxe, avec la position (en caractères, à partir de 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub pos: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "caractère {} : {}", self.pos + 1, self.message)
    }
}

impl std::error::Error for ParseError {}

fn parse_err(pos: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        pos,
        message: message.into(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    Sqrt,
    Abs,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Exp,
    Ln,
    Log10,
    Pow,
    Min,
    Max,
}

impl Func {
    fn from_name(name: &str) -> Option<Func> {
        Some(match name {
            "sqrt" => Func::Sqrt,
            "abs" => Func::Abs,
            "sin" => Func::Sin,
            "cos" => Func::Cos,
            "tan" => Func::Tan,
            "asin" => Func::Asin,
            "acos" => Func::Acos,
            "atan" => Func::Atan,
            "atan2" => Func::Atan2,
            "exp" => Func::Exp,
            "ln" => Func::Ln,
            "log10" | "log" => Func::Log10,
            "pow" => Func::Pow,
            "min" => Func::Min,
            "max" => Func::Max,
            _ => return None,
        })
    }

    fn arity(self) -> usize {
        match self {
            Func::Atan2 | Func::Pow | Func::Min | Func::Max => 2,
            _ => 1,
        }
    }
}

/// Arbre d'une formule.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Var(String),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
}

// ---------------------------------------------------------------- lexique

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
    Comma,
    End,
}

fn tokenize(src: &str) -> Result<Vec<(Tok, usize)>, ParseError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let starts_number = c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()));
        if starts_number {
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '.' {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            // Exposant : seulement si des chiffres suivent (« 2e » reste « 2 » puis « e »).
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                    i = j;
                }
            }
            let text: String = chars[start..i].iter().collect();
            let value: f64 = text
                .parse()
                .map_err(|_| parse_err(start, "nombre invalide"))?;
            if !value.is_finite() {
                return Err(parse_err(start, "nombre trop grand"));
            }
            out.push((Tok::Num(value), start));
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            out.push((Tok::Ident(text), start));
            continue;
        }
        let tok = match c {
            '+' => Tok::Plus,
            '-' => Tok::Minus,
            '*' => Tok::Star,
            '/' => Tok::Slash,
            '^' => Tok::Caret,
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            ',' => Tok::Comma,
            _ => return Err(parse_err(start, format!("caractère inattendu « {c} »"))),
        };
        out.push((tok, start));
        i += 1;
    }
    out.push((Tok::End, chars.len()));
    Ok(out)
}

// ------------------------------------------------------------- grammaire

struct Parser {
    toks: Vec<(Tok, usize)>,
    i: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.i].0
    }

    fn pos(&self) -> usize {
        self.toks[self.i].1
    }

    fn bump(&mut self) -> Tok {
        let tok = self.toks[self.i].0.clone();
        if self.i + 1 < self.toks.len() {
            self.i += 1;
        }
        tok
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(parse_err(self.pos(), message))
    }

    fn parse_sum(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_product()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => Op::Add,
                Tok::Minus => Op::Sub,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.parse_product()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn parse_product(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => Op::Mul,
                Tok::Slash => Op::Div,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.parse_unary()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.depth >= MAX_DEPTH {
            return self.err("expression trop imbriquée");
        }
        self.depth += 1;
        let result = self.parse_unary_inner();
        self.depth -= 1;
        result
    }

    fn parse_unary_inner(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Tok::Minus => {
                self.bump();
                Ok(Expr::Neg(Box::new(self.parse_unary()?)))
            }
            Tok::Plus => {
                self.bump();
                self.parse_unary()
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Result<Expr, ParseError> {
        let base = self.parse_atom()?;
        if *self.peek() == Tok::Caret {
            self.bump();
            // L'exposant passe par parse_unary : « 2^-3 » est permis et « 2^3^2 » vaut 2^(3^2).
            let exponent = self.parse_unary()?;
            return Ok(Expr::Bin(Op::Pow, Box::new(base), Box::new(exponent)));
        }
        Ok(base)
    }

    fn parse_atom(&mut self) -> Result<Expr, ParseError> {
        let pos = self.pos();
        match self.bump() {
            Tok::Num(v) => Ok(Expr::Num(v)),
            Tok::Ident(name) => {
                if *self.peek() == Tok::LParen {
                    self.bump();
                    self.parse_call(&name, pos)
                } else {
                    Ok(Expr::Var(name))
                }
            }
            Tok::LParen => {
                let inner = self.parse_sum()?;
                if *self.peek() != Tok::RParen {
                    return self.err("parenthèse fermante « ) » attendue");
                }
                self.bump();
                Ok(inner)
            }
            Tok::End => Err(parse_err(pos, "expression incomplète")),
            Tok::RParen => Err(parse_err(pos, "parenthèse fermante « ) » inattendue")),
            Tok::Comma => Err(parse_err(
                pos,
                "virgule inattendue (le séparateur décimal des formules est le point)",
            )),
            _ => Err(parse_err(pos, "nombre, nom ou parenthèse attendu")),
        }
    }

    fn parse_call(&mut self, name: &str, name_pos: usize) -> Result<Expr, ParseError> {
        let Some(func) = Func::from_name(name) else {
            return Err(parse_err(name_pos, format!("fonction inconnue « {name} »")));
        };
        let mut args = Vec::new();
        if *self.peek() != Tok::RParen {
            loop {
                args.push(self.parse_sum()?);
                match self.peek() {
                    Tok::Comma => {
                        self.bump();
                    }
                    Tok::RParen => break,
                    _ => return self.err("« , » ou « ) » attendu"),
                }
            }
        }
        self.bump(); // la parenthèse fermante
        if args.len() != func.arity() {
            return Err(parse_err(
                name_pos,
                format!(
                    "« {name} » attend {} argument(s), {} donné(s)",
                    func.arity(),
                    args.len()
                ),
            ));
        }
        Ok(Expr::Call(func, args))
    }
}

// ------------------------------------------------------------ évaluation

fn fail(error: CellError) -> EvalFail {
    EvalFail::Error(error)
}

/// Un résultat doit être fini : NaN devient une erreur de domaine, l'infini un débordement.
fn finite(x: f64) -> Result<f64, EvalFail> {
    if x.is_nan() {
        Err(fail(CellError::Domain))
    } else if x.is_infinite() {
        Err(fail(CellError::Overflow))
    } else {
        Ok(x)
    }
}

impl Expr {
    /// Analyse une formule.
    pub fn parse(src: &str) -> Result<Expr, ParseError> {
        let toks = tokenize(src)?;
        let mut parser = Parser {
            toks,
            i: 0,
            depth: 0,
        };
        let expr = parser.parse_sum()?;
        if *parser.peek() != Tok::End {
            return match parser.peek() {
                Tok::RParen => parser.err("parenthèse fermante « ) » inattendue"),
                Tok::Comma => parser
                    .err("virgule inattendue (le séparateur décimal des formules est le point)"),
                _ => parser.err("opérateur attendu (+ - * / ^)"),
            };
        }
        Ok(expr)
    }

    /// Noms de variables utilisés, sans doublon, dans l'ordre d'apparition.
    pub fn variables(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect_variables(&mut out);
        out
    }

    fn collect_variables<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Expr::Num(_) => {}
            Expr::Var(name) => {
                if !out.contains(&name.as_str()) {
                    out.push(name);
                }
            }
            Expr::Neg(e) => e.collect_variables(out),
            Expr::Bin(_, a, b) => {
                a.collect_variables(out);
                b.collect_variables(out);
            }
            Expr::Call(_, args) => {
                for a in args {
                    a.collect_variables(out);
                }
            }
        }
    }

    /// Évalue la formule. `lookup` donne la valeur d'un nom, ou `None` s'il est inconnu
    /// (la constante `pi` est alors utilisée si le nom est `pi`).
    pub fn eval(
        &self,
        lookup: &dyn Fn(&str) -> Option<Result<f64, EvalFail>>,
    ) -> Result<f64, EvalFail> {
        match self {
            Expr::Num(v) => Ok(*v),
            Expr::Var(name) => match lookup(name) {
                Some(result) => result,
                None if name == "pi" => Ok(std::f64::consts::PI),
                None => Err(fail(CellError::UnknownName)),
            },
            Expr::Neg(e) => Ok(-e.eval(lookup)?),
            Expr::Bin(op, a, b) => {
                let x = a.eval(lookup)?;
                let y = b.eval(lookup)?;
                match op {
                    Op::Add => finite(x + y),
                    Op::Sub => finite(x - y),
                    Op::Mul => finite(x * y),
                    Op::Div => {
                        if y == 0.0 {
                            Err(fail(CellError::DivByZero))
                        } else {
                            finite(x / y)
                        }
                    }
                    Op::Pow => power(x, y),
                }
            }
            Expr::Call(func, args) => {
                let x = args[0].eval(lookup)?;
                let y = if args.len() > 1 {
                    args[1].eval(lookup)?
                } else {
                    0.0
                };
                call(*func, x, y)
            }
        }
    }
}

fn power(base: f64, exponent: f64) -> Result<f64, EvalFail> {
    if base == 0.0 && exponent < 0.0 {
        return Err(fail(CellError::DivByZero));
    }
    finite(base.powf(exponent))
}

fn call(func: Func, x: f64, y: f64) -> Result<f64, EvalFail> {
    match func {
        Func::Sqrt if x < 0.0 => Err(fail(CellError::Domain)),
        Func::Sqrt => finite(x.sqrt()),
        Func::Abs => finite(x.abs()),
        Func::Sin => finite(x.sin()),
        Func::Cos => finite(x.cos()),
        Func::Tan => finite(x.tan()),
        Func::Asin | Func::Acos if x.abs() > 1.0 => Err(fail(CellError::Domain)),
        Func::Asin => finite(x.asin()),
        Func::Acos => finite(x.acos()),
        Func::Atan => finite(x.atan()),
        Func::Atan2 => finite(x.atan2(y)),
        Func::Exp => finite(x.exp()),
        Func::Ln | Func::Log10 if x <= 0.0 => Err(fail(CellError::Domain)),
        Func::Ln => finite(x.ln()),
        Func::Log10 => finite(x.log10()),
        Func::Pow => power(x, y),
        Func::Min => Ok(x.min(y)),
        Func::Max => Ok(x.max(y)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &str) -> Result<f64, EvalFail> {
        Expr::parse(src).unwrap().eval(&|_| None)
    }

    fn eval_with(src: &str, vars: &[(&str, f64)]) -> Result<f64, EvalFail> {
        let vars: Vec<(String, f64)> = vars.iter().map(|(n, v)| (n.to_string(), *v)).collect();
        Expr::parse(src).unwrap().eval(&move |name| {
            vars.iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| Ok::<f64, EvalFail>(*v))
        })
    }

    #[test]
    fn precedences_and_associativity() {
        assert_eq!(eval("1+2*3"), Ok(7.0));
        assert_eq!(eval("(1+2)*3"), Ok(9.0));
        assert_eq!(eval("10-4-3"), Ok(3.0));
        assert_eq!(eval("100/10/5"), Ok(2.0));
        assert_eq!(eval("2^3^2"), Ok(512.0));
        assert_eq!(eval("-2^2"), Ok(-4.0));
        assert_eq!(eval("2^-1"), Ok(0.5));
        assert_eq!(eval("--3"), Ok(3.0));
        assert_eq!(eval("+3"), Ok(3.0));
    }

    #[test]
    fn numbers() {
        assert_eq!(eval(".5+0.5"), Ok(1.0));
        assert_eq!(eval("1e3"), Ok(1000.0));
        assert_eq!(eval("2.5E-1"), Ok(0.25));
        assert_eq!(eval("3."), Ok(3.0));
    }

    #[test]
    fn functions_and_constants() {
        assert_eq!(eval("sqrt(16)"), Ok(4.0));
        assert_eq!(eval("max(2,5)"), Ok(5.0));
        assert_eq!(eval("min(2, 5)"), Ok(2.0));
        assert_eq!(eval("pow(2,10)"), Ok(1024.0));
        assert_eq!(eval("log(1000)"), Ok(3.0));
        assert!((eval("sin(pi/2)").unwrap() - 1.0).abs() < 1e-12);
        assert!((eval("ln(exp(2))").unwrap() - 2.0).abs() < 1e-12);
        assert!((eval("atan2(1,1)").unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    }

    #[test]
    fn variables_and_shadowing_of_pi() {
        assert_eq!(eval_with("0.5*g*t^2", &[("g", 10.0), ("t", 2.0)]), Ok(20.0));
        // une colonne nommée « pi » prend le pas sur la constante
        assert_eq!(eval_with("pi", &[("pi", 3.0)]), Ok(3.0));
        assert_eq!(eval("x+1"), Err(fail(CellError::UnknownName)));
    }

    #[test]
    fn variables_list_is_unique_and_ordered() {
        let e = Expr::parse("a*b + a - sqrt(c)").unwrap();
        assert_eq!(e.variables(), vec!["a", "b", "c"]);
    }

    #[test]
    fn evaluation_errors() {
        assert_eq!(eval("1/0"), Err(fail(CellError::DivByZero)));
        assert_eq!(eval("0/0"), Err(fail(CellError::DivByZero)));
        assert_eq!(eval("0^-1"), Err(fail(CellError::DivByZero)));
        assert_eq!(eval("sqrt(-1)"), Err(fail(CellError::Domain)));
        assert_eq!(eval("ln(0)"), Err(fail(CellError::Domain)));
        assert_eq!(eval("log10(-5)"), Err(fail(CellError::Domain)));
        assert_eq!(eval("asin(2)"), Err(fail(CellError::Domain)));
        assert_eq!(eval("(-8)^0.5"), Err(fail(CellError::Domain)));
        assert_eq!(eval("10^400"), Err(fail(CellError::Overflow)));
        assert_eq!(eval("exp(1000)"), Err(fail(CellError::Overflow)));
    }

    #[test]
    fn empty_input_propagates() {
        let e = Expr::parse("x*2").unwrap();
        let r = e.eval(&|_| Some(Err(EvalFail::Empty)));
        assert_eq!(r, Err(EvalFail::Empty));
    }

    #[test]
    fn syntax_errors_have_positions_and_messages() {
        let err = Expr::parse("2 +").unwrap_err();
        assert_eq!(err.message, "expression incomplète");
        assert_eq!(err.pos, 3);

        let err = Expr::parse("(1+2").unwrap_err();
        assert!(err.message.contains("parenthèse fermante"));

        let err = Expr::parse("1 2").unwrap_err();
        assert!(err.message.contains("opérateur attendu"));

        let err = Expr::parse("0,5*2").unwrap_err();
        assert!(err.message.contains("point"), "{}", err.message);

        let err = Expr::parse("foo(1)").unwrap_err();
        assert!(err.message.contains("fonction inconnue"));

        let err = Expr::parse("sqrt(1,2)").unwrap_err();
        assert!(err.message.contains("attend 1 argument"));

        let err = Expr::parse("2 $ 3").unwrap_err();
        assert!(err.message.contains("caractère inattendu"));

        assert!(Expr::parse("").is_err());
        assert!(Expr::parse(")").is_err());
        assert!(Expr::parse("2e").is_err()); // « 2 » puis le nom « e » : opérateur attendu
    }

    #[test]
    fn deep_nesting_is_rejected_not_crashed() {
        let src = format!("{}1{}", "(".repeat(5000), ")".repeat(5000));
        let err = Expr::parse(&src).unwrap_err();
        assert!(err.message.contains("trop imbriquée"));
        let minus = format!("{}1", "-".repeat(5000));
        assert!(Expr::parse(&minus).is_err());
    }

    #[test]
    fn unicode_names() {
        assert_eq!(eval_with("Δt*2", &[("Δt", 4.0)]), Ok(8.0));
    }
}

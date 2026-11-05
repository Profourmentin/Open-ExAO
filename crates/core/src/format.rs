//! Lecture et affichage des nombres, à la française par défaut (virgule décimale).

/// Lit un nombre saisi par l'utilisateur : `1,5`, `1.5`, `1 000,5`, `2e-3`.
/// Renvoie `None` pour une chaîne vide, un texte non numérique ou un nombre non fini.
pub fn parse_number(text: &str) -> Option<f64> {
    let cleaned: String = text
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace()) // espaces, insécables et fines (milliers à la française)
        .map(|c| if c == ',' { '.' } else { c })
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    let value: f64 = cleaned.parse().ok()?;
    value.is_finite().then_some(value)
}

/// Écrit un nombre avec le plus court affichage qui permet de le relire à l'identique.
pub fn format_exact(x: f64, decimal_comma: bool) -> String {
    let text = format!("{x}");
    if decimal_comma {
        text.replace('.', ",")
    } else {
        text
    }
}

/// Affiche un nombre avec `sig` chiffres significatifs (1 à 15), sans zéros inutiles.
/// Notation fixe pour les exposants de -4 à 6, scientifique au-delà (`1,5e-7`).
pub fn format_number(x: f64, sig: usize, decimal_comma: bool) -> String {
    let sig = sig.clamp(1, 15);
    if x == 0.0 {
        return "0".to_string();
    }
    if !x.is_finite() {
        return "—".to_string();
    }
    let scientific = format!("{:.*e}", sig - 1, x); // ex. « -1.23457e2 »
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("le format scientifique contient toujours « e »");
    let exponent: i32 = exponent.parse().expect("exposant entier");
    let text = if (-4..7).contains(&exponent) {
        let decimals = (sig as i32 - 1 - exponent).max(0) as usize;
        trim_zeros(&format!("{x:.decimals$}"))
    } else {
        format!("{}e{exponent}", trim_zeros(mantissa))
    };
    if decimal_comma {
        text.replace('.', ",")
    } else {
        text
    }
}

fn trim_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_french_and_english_notations() {
        assert_eq!(parse_number("1,5"), Some(1.5));
        assert_eq!(parse_number("1.5"), Some(1.5));
        assert_eq!(parse_number("  -2,25 "), Some(-2.25));
        assert_eq!(parse_number("1 000,5"), Some(1000.5));
        assert_eq!(parse_number("1\u{a0}000,5"), Some(1000.5));
        assert_eq!(parse_number("2e-3"), Some(0.002));
        assert_eq!(parse_number("+4"), Some(4.0));
    }

    #[test]
    fn parse_rejects_non_numbers_and_non_finite() {
        for bad in [
            "", "  ", "abc", "1,2,3", "1.2.3", "inf", "-inf", "NaN", "infinity", "1e999", "0x10",
        ] {
            assert_eq!(parse_number(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn exact_format_round_trips() {
        for x in [
            0.1 + 0.2,
            1.0 / 3.0,
            1e-10,
            123456789.125,
            -0.5,
            6.02214076e23,
        ] {
            let text = format_exact(x, true);
            assert_eq!(parse_number(&text), Some(x), "{text}");
        }
        assert_eq!(format_exact(1.5, true), "1,5");
        assert_eq!(format_exact(1.5, false), "1.5");
    }

    #[test]
    fn display_format() {
        assert_eq!(format_number(0.0, 6, true), "0");
        assert_eq!(format_number(1.0, 6, true), "1");
        assert_eq!(format_number(4.905, 6, true), "4,905");
        assert_eq!(format_number(-4.905, 6, true), "-4,905");
        assert_eq!(format_number(1.0 / 3.0, 6, true), "0,333333");
        assert_eq!(format_number(1.0 / 3.0, 3, false), "0.333");
        assert_eq!(format_number(1234.5678, 6, true), "1234,57");
        assert_eq!(format_number(123456.0, 6, true), "123456");
        assert_eq!(format_number(0.0001234, 3, true), "0,000123");
        assert_eq!(format_number(0.00001234, 3, true), "1,23e-5");
        assert_eq!(format_number(1.5e10, 6, true), "1,5e10");
        assert_eq!(format_number(9.9999996, 6, true), "10");
        assert_eq!(format_number(f64::NAN, 6, true), "—");
    }
}

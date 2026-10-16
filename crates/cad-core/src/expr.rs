//! Parameter expression engine.
//!
//! Fusion stores every dimension and feature value as a named parameter whose value is an
//! expression with units ("50 mm", "d2 / 2", "2 * width + 3 mm"). Observed in Fusion 2705.1.30:
//! `d2 / 2` was accepted for an extrude distance, while `foo * 2` and `15 deg` (for a length)
//! were rejected with "Expression is invalid." This module parses and evaluates such
//! expressions with light dimensional analysis: lengths normalise to millimetres, angles to
//! degrees. A bare number adopts the unit of its context, as in Fusion.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quantity {
    Length,
    Angle,
    Unitless,
}

impl Quantity {
    pub fn unit_suffix(self) -> &'static str {
        match self {
            Quantity::Length => "mm",
            Quantity::Angle => "deg",
            Quantity::Unitless => "",
        }
    }
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct ExprError(pub String);

fn err<T>(msg: impl Into<String>) -> Result<T, ExprError> {
    Err(ExprError(msg.into()))
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Dim {
    len: f64,
    ang: f64,
}
const NONE: Dim = Dim { len: 0.0, ang: 0.0 };
const LEN: Dim = Dim { len: 1.0, ang: 0.0 };
const ANG: Dim = Dim { len: 0.0, ang: 1.0 };

#[derive(Clone, Copy, Debug)]
struct Val {
    v: f64,
    d: Dim,
    /// A number written without a unit; it adopts the unit of its context.
    bare: bool,
}

fn unit(name: &str) -> Option<(f64, Dim)> {
    Some(match name {
        "mm" => (1.0, LEN),
        "cm" => (10.0, LEN),
        "m" => (1000.0, LEN),
        "in" => (25.4, LEN),
        "ft" => (304.8, LEN),
        "deg" => (1.0, ANG),
        "rad" => (180.0 / std::f64::consts::PI, ANG),
        _ => return None,
    })
}

const FUNCS: &[&str] = &["sqrt", "abs", "min", "max", "round", "floor", "ceil", "sin", "cos", "tan"];

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Id(String),
    Op(char),
    LParen,
    RParen,
    Comma,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, ExprError> {
    let mut out = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let s: String = chars[start..i].iter().collect();
            let v: f64 = s.parse().map_err(|_| ExprError(format!("Invalid number \"{s}\"")))?;
            out.push(Tok::Num(v));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Id(chars[start..i].iter().collect()));
        } else if "+-*/^".contains(c) {
            out.push(Tok::Op(c));
            i += 1;
        } else if c == '(' {
            out.push(Tok::LParen);
            i += 1;
        } else if c == ')' {
            out.push(Tok::RParen);
            i += 1;
        } else if c == ',' {
            out.push(Tok::Comma);
            i += 1;
        } else {
            return err(format!("Unexpected character \"{c}\""));
        }
    }
    Ok(out)
}

/// Looks up a referenced parameter: its value (base units) and quantity.
pub trait Resolve {
    fn resolve(&mut self, name: &str) -> Result<Option<(f64, Quantity)>, ExprError>;
}

impl<F: FnMut(&str) -> Result<Option<(f64, Quantity)>, ExprError>> Resolve for F {
    fn resolve(&mut self, name: &str) -> Result<Option<(f64, Quantity)>, ExprError> {
        self(name)
    }
}

struct Parser<'a, R: Resolve> {
    toks: Vec<Tok>,
    i: usize,
    resolver: &'a mut R,
    refs: Vec<String>,
}

impl<R: Resolve> Parser<'_, R> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.i).cloned();
        self.i += 1;
        t
    }
    fn sum(&mut self) -> Result<Val, ExprError> {
        let mut a = self.product()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.i += 1;
            let b = self.product()?;
            a = add_sub(a, b, if op == '+' { 1.0 } else { -1.0 })?;
        }
        Ok(a)
    }
    fn product(&mut self) -> Result<Val, ExprError> {
        let mut a = self.unary()?;
        while let Some(Tok::Op(op @ ('*' | '/'))) = self.peek().cloned() {
            self.i += 1;
            let b = self.unary()?;
            a = if op == '*' {
                Val { v: a.v * b.v, d: Dim { len: a.d.len + b.d.len, ang: a.d.ang + b.d.ang }, bare: a.bare && b.bare }
            } else {
                if b.v == 0.0 {
                    return err("Division by zero");
                }
                Val { v: a.v / b.v, d: Dim { len: a.d.len - b.d.len, ang: a.d.ang - b.d.ang }, bare: a.bare && b.bare }
            };
        }
        Ok(a)
    }
    fn unary(&mut self) -> Result<Val, ExprError> {
        if let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.i += 1;
            let v = self.unary()?;
            return Ok(if op == '-' { Val { v: -v.v, ..v } } else { v });
        }
        self.power()
    }
    fn power(&mut self) -> Result<Val, ExprError> {
        let base = self.atom()?;
        let base = self.postfix_unit(base);
        if let Some(Tok::Op('^')) = self.peek() {
            self.i += 1;
            let e = self.unary()?;
            if e.d != NONE {
                return err("Exponent must be unitless");
            }
            return Ok(Val { v: base.v.powf(e.v), d: Dim { len: base.d.len * e.v, ang: base.d.ang * e.v }, bare: base.bare });
        }
        Ok(base)
    }
    /// A number immediately followed by a unit name ("50 mm", "2.5in").
    fn postfix_unit(&mut self, v: Val) -> Val {
        if v.bare && matches!(self.toks.get(self.i.wrapping_sub(1)), Some(Tok::Num(_))) {
            if let Some(Tok::Id(name)) = self.peek() {
                if let Some((f, d)) = unit(name) {
                    self.i += 1;
                    return Val { v: v.v * f, d, bare: false };
                }
            }
        }
        v
    }
    fn atom(&mut self) -> Result<Val, ExprError> {
        match self.next() {
            None => err("Unexpected end of expression"),
            Some(Tok::Num(v)) => Ok(Val { v, d: NONE, bare: true }),
            Some(Tok::LParen) => {
                let v = self.sum()?;
                if self.next() != Some(Tok::RParen) {
                    return err("Missing closing parenthesis");
                }
                Ok(v)
            }
            Some(Tok::Id(name)) => {
                if self.peek() == Some(&Tok::LParen) {
                    return self.call(&name);
                }
                if name == "PI" || name == "pi" {
                    return Ok(Val { v: std::f64::consts::PI, d: NONE, bare: true });
                }
                if unit(&name).is_some() {
                    return err(format!("Unit \"{name}\" must follow a number"));
                }
                match self.resolver.resolve(&name)? {
                    None => err(format!("Unknown parameter \"{name}\"")),
                    Some((v, q)) => {
                        if !self.refs.contains(&name) {
                            self.refs.push(name);
                        }
                        let d = match q {
                            Quantity::Length => LEN,
                            Quantity::Angle => ANG,
                            Quantity::Unitless => NONE,
                        };
                        Ok(Val { v, d, bare: false })
                    }
                }
            }
            Some(_) => err("Unexpected token"),
        }
    }
    fn call(&mut self, name: &str) -> Result<Val, ExprError> {
        self.i += 1; // (
        let mut args = Vec::new();
        if self.peek() != Some(&Tok::RParen) {
            loop {
                args.push(self.sum()?);
                if self.peek() == Some(&Tok::Comma) {
                    self.i += 1;
                    continue;
                }
                break;
            }
        }
        if self.next() != Some(Tok::RParen) {
            return err(format!("Missing \")\" after {name}("));
        }
        if args.is_empty() {
            return err(format!("{name}() needs arguments"));
        }
        let a = args[0];
        Ok(match name {
            "sin" | "cos" | "tan" => {
                if args.len() != 1 || a.d.len != 0.0 {
                    return err(format!("{name}() needs one angle argument"));
                }
                // bare numbers are degrees, matching the document angle unit
                let r = a.v.to_radians();
                let v = match name {
                    "sin" => r.sin(),
                    "cos" => r.cos(),
                    _ => r.tan(),
                };
                Val { v, d: NONE, bare: true }
            }
            "sqrt" => Val { v: a.v.sqrt(), d: Dim { len: a.d.len / 2.0, ang: a.d.ang / 2.0 }, bare: a.bare },
            "abs" => Val { v: a.v.abs(), ..a },
            "round" => Val { v: a.v.round(), ..a },
            "floor" => Val { v: a.v.floor(), ..a },
            "ceil" => Val { v: a.v.ceil(), ..a },
            "min" | "max" => {
                let mut acc = a;
                for b in &args[1..] {
                    let b = add_sub(*b, Val { v: 0.0, ..acc }, 1.0)?; // unit check
                    acc.v = if name == "min" { acc.v.min(b.v) } else { acc.v.max(b.v) };
                    acc.bare &= b.bare;
                }
                acc
            }
            _ => return err(format!("Unknown function \"{name}\"")),
        })
    }
}

fn add_sub(mut a: Val, mut b: Val, sign: f64) -> Result<Val, ExprError> {
    if a.bare && !b.bare {
        a = Val { d: b.d, bare: false, ..a };
    }
    if b.bare && !a.bare {
        b = Val { d: a.d, bare: false, ..b };
    }
    if a.d != b.d {
        return err("Cannot add values with different units");
    }
    Ok(Val { v: a.v + sign * b.v, d: a.d, bare: a.bare && b.bare })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Evaluated {
    /// Value in base units (mm, deg).
    pub value: f64,
    pub refs: Vec<String>,
}

/// Evaluates an expression that must produce `expect`.
pub fn evaluate(src: &str, expect: Quantity, resolver: &mut impl Resolve) -> Result<Evaluated, ExprError> {
    let toks = tokenize(src)?;
    if toks.is_empty() {
        return err("Expression is empty");
    }
    let mut p = Parser { toks, i: 0, resolver, refs: Vec::new() };
    let v = p.sum()?;
    if p.i < p.toks.len() {
        return err("Unexpected input after expression");
    }
    if !v.v.is_finite() {
        return err("Result is not a finite number");
    }
    let want = match expect {
        Quantity::Length => LEN,
        Quantity::Angle => ANG,
        Quantity::Unitless => NONE,
    };
    if !v.bare && v.d != want {
        let got = if v.d == LEN {
            "length"
        } else if v.d == ANG {
            "angle"
        } else if v.d == NONE {
            "unitless value"
        } else {
            "compound unit"
        };
        let want_s = match expect {
            Quantity::Length => "length",
            Quantity::Angle => "angle",
            Quantity::Unitless => "unitless value",
        };
        return err(format!("Expected a {want_s} but the expression is a {got}"));
    }
    Ok(Evaluated { value: v.v, refs: p.refs })
}

/// Evaluates an expression that references no parameters.
pub fn evaluate_constant(src: &str, expect: Quantity) -> Result<f64, ExprError> {
    evaluate(src, expect, &mut |_: &str| Ok(None)).map(|e| e.value)
}

/// Parameter names referenced by an expression (without evaluating it).
pub fn referenced_names(src: &str) -> Vec<String> {
    let Ok(toks) = tokenize(src) else { return Vec::new() };
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if let Tok::Id(name) = t {
            let is_call = toks.get(i + 1) == Some(&Tok::LParen);
            let after_num = i > 0 && matches!(toks[i - 1], Tok::Num(_));
            if !is_call && !(after_num && unit(name).is_some()) && unit(name).is_none() && name != "PI" && name != "pi" && !FUNCS.contains(&name.as_str()) && !out.contains(name) {
                out.push(name.clone());
            }
        }
    }
    out
}

/// Formats a value for display with the quantity's unit ("50 mm", "12.5 deg").
pub struct Display(pub f64, pub Quantity);
impl fmt::Display for Display {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = format!("{:.3}", self.0);
        while s.contains('.') && (s.ends_with('0') || s.ends_with('.')) {
            s.pop();
        }
        if s == "-0" {
            s = "0".into();
        }
        match self.1 {
            Quantity::Unitless => write!(f, "{s}"),
            q => write!(f, "{s} {}", q.unit_suffix()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(name: &str) -> Result<Option<(f64, Quantity)>, ExprError> {
        Ok(match name {
            "d1" => Some((50.0, Quantity::Length)),
            "d2" => Some((30.0, Quantity::Length)),
            "a1" => Some((90.0, Quantity::Angle)),
            "n" => Some((4.0, Quantity::Unitless)),
            _ => None,
        })
    }
    fn len(s: &str) -> Result<f64, ExprError> {
        evaluate(s, Quantity::Length, &mut params).map(|e| e.value)
    }

    #[test]
    fn units_and_bare_numbers() {
        assert_eq!(len("50 mm").unwrap(), 50.0);
        assert_eq!(len("5 cm").unwrap(), 50.0);
        assert_eq!(len("1 in").unwrap(), 25.4);
        assert_eq!(len("12").unwrap(), 12.0);
        assert_eq!(len("2.5e1 mm").unwrap(), 25.0);
    }

    #[test]
    fn parameter_references_like_fusion() {
        // Observed: Fusion accepted "d2 / 2" (30 mm / 2 = 15 mm) for an extrude distance.
        let e = evaluate("d2 / 2", Quantity::Length, &mut params).unwrap();
        assert_eq!(e.value, 15.0);
        assert_eq!(e.refs, vec!["d2".to_string()]);
        assert_eq!(len("d1 + 5").unwrap(), 55.0);
        assert_eq!(len("d1 * n - 10 mm").unwrap(), 190.0);
        assert_eq!(len("sqrt(d1 * d1)").unwrap(), 50.0);
        assert!((len("d1 * cos(60 deg)").unwrap() - 25.0).abs() < 1e-9);
        assert_eq!(len("max(d1, d2)").unwrap(), 50.0);
        assert_eq!(len("-d2").unwrap(), -30.0);
    }

    #[test]
    fn rejects_invalid_expressions_like_fusion() {
        // Observed: "foo * 2" and "15 deg" (for a length) -> "Expression is invalid."
        assert!(len("foo * 2").unwrap_err().0.contains("Unknown parameter"));
        assert!(len("15 deg").unwrap_err().0.contains("Expected a length"));
        assert!(len("d1 + a1").is_err());
        assert!(len("").is_err());
        assert!(len("(d1").is_err());
        assert!(len("d1 / 0").is_err());
        assert!(len("d1 * d2").is_err()); // area, not a length
    }

    #[test]
    fn angles() {
        assert_eq!(evaluate("360 deg", Quantity::Angle, &mut params).unwrap().value, 360.0);
        let pi = evaluate("PI rad", Quantity::Angle, &mut params);
        assert!(pi.is_err()); // PI is a bare constant: "rad" must follow a number
        assert!((evaluate("a1 / 2", Quantity::Angle, &mut params).unwrap().value - 45.0).abs() < 1e-12);
    }

    #[test]
    fn referenced_names_skips_units_and_functions() {
        assert_eq!(referenced_names("2 * d1 + sqrt(width) - 3 mm"), vec!["d1", "width"]);
    }

    #[test]
    fn display() {
        assert_eq!(Display(50.0, Quantity::Length).to_string(), "50 mm");
        assert_eq!(Display(12.3456, Quantity::Angle).to_string(), "12.346 deg");
        assert_eq!(Display(-0.0001, Quantity::Length).to_string(), "0 mm");
    }
}

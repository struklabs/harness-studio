use serde_json::{json, Value};

use super::simple_def;
use crate::tools::{PermissionClass, ToolDef, ToolError, ToolOutput};

pub fn def() -> ToolDef {
    simple_def(
        "calculator",
        "Evaluate a safe arithmetic expression without executing code. Supports +, -, *, /, %, ^, parentheses, pi, e, and sqrt/abs/round/min/max/log/ln/sin/cos/tan.",
        PermissionClass::Safe,
        json!({
            "type": "object",
            "properties": {"expression": {"type": "string", "description": "Arithmetic expression to evaluate."}},
            "required": ["expression"],
            "additionalProperties": false
        }),
    )
}

pub fn run(args: Value) -> Result<ToolOutput, ToolError> {
    let expression = args["expression"].as_str().unwrap_or_default();
    if expression.trim().is_empty() {
        return Err(ToolError::invalid("expression must not be empty"));
    }
    if expression.len() > MAX_EXPRESSION_BYTES {
        return Err(ToolError::invalid(format!(
            "expression is longer than {MAX_EXPRESSION_BYTES} bytes"
        )));
    }
    let mut parser = Parser::new(expression);
    let value = parser.expression()?;
    parser.space();
    if parser.peek().is_some() {
        return Err(parser.error("unexpected character"));
    }
    if !value.is_finite() {
        return Err(ToolError::failed("the result is not a finite number"));
    }
    Ok(ToolOutput::text(
        json!({"expression": expression, "result": value}).to_string(),
    ))
}

/// Bounds the input and the recursion depth so hostile input cannot overflow the stack.
const MAX_EXPRESSION_BYTES: usize = 2000;
const MAX_DEPTH: usize = 64;

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            pos: 0,
            depth: 0,
        }
    }
    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }
    fn space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }
    fn take(&mut self, c: char) -> bool {
        self.space();
        if self.peek() == Some(c) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn error(&self, message: &str) -> ToolError {
        ToolError::invalid(format!("{message} at byte {}", self.pos + 1))
    }

    fn expression(&mut self) -> Result<f64, ToolError> {
        let mut v = self.term()?;
        loop {
            if self.take('+') {
                v += self.term()?;
            } else if self.take('-') {
                v -= self.term()?;
            } else {
                return Ok(v);
            }
        }
    }
    fn term(&mut self) -> Result<f64, ToolError> {
        let mut v = self.unary()?;
        loop {
            if self.take('*') {
                v *= self.unary()?;
            } else if self.take('/') {
                let r = self.unary()?;
                if r == 0.0 {
                    return Err(self.error("division by zero"));
                }
                v /= r;
            } else if self.take('%') {
                let r = self.unary()?;
                if r == 0.0 {
                    return Err(self.error("remainder by zero"));
                }
                v %= r;
            } else {
                return Ok(v);
            }
        }
    }
    fn unary(&mut self) -> Result<f64, ToolError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.error("expression is nested too deeply"));
        }
        let result = if self.take('+') {
            self.unary()
        } else if self.take('-') {
            self.unary().map(|v| -v)
        } else {
            self.power()
        };
        self.depth -= 1;
        result
    }
    fn power(&mut self) -> Result<f64, ToolError> {
        let base = self.primary()?;
        if self.take('^') {
            Ok(base.powf(self.unary()?))
        } else {
            Ok(base)
        }
    }
    fn primary(&mut self) -> Result<f64, ToolError> {
        self.space();
        if self.take('(') {
            let v = self.expression()?;
            if !self.take(')') {
                return Err(self.error("expected ')'"));
            }
            return Ok(v);
        }
        if self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
            return self.number();
        }
        if self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            let name = self.ident();
            if name == "pi" {
                return Ok(std::f64::consts::PI);
            }
            if name == "e" {
                return Ok(std::f64::consts::E);
            }
            if !self.take('(') {
                return Err(self.error(&format!("unknown constant {name:?}")));
            }
            let mut values = vec![self.expression()?];
            while self.take(',') {
                values.push(self.expression()?);
            }
            if !self.take(')') {
                return Err(self.error("expected ')' after function arguments"));
            }
            return self.function(&name, &values);
        }
        Err(self.error("expected a number, constant, function, or '('"))
    }
    fn number(&mut self) -> Result<f64, ToolError> {
        self.space();
        let start = self.pos;
        let mut dot = false;
        let mut exponent = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.bump();
            } else if c == '.' && !dot && !exponent {
                dot = true;
                self.bump();
            } else if matches!(c, 'e' | 'E') && !exponent {
                exponent = true;
                self.bump();
                if matches!(self.peek(), Some('+' | '-')) {
                    self.bump();
                }
            } else {
                break;
            }
        }
        self.src[start..self.pos]
            .parse()
            .map_err(|_| self.error("invalid number"))
    }
    fn ident(&mut self) -> String {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.bump();
        }
        self.src[start..self.pos].to_ascii_lowercase()
    }
    fn function(&self, name: &str, a: &[f64]) -> Result<f64, ToolError> {
        let one = || {
            if a.len() == 1 {
                Ok(a[0])
            } else {
                Err(self.error(&format!("{name} expects one argument")))
            }
        };
        match name {
            "sqrt" => {
                let x = one()?;
                if x < 0.0 {
                    Err(self.error("sqrt domain error: argument must be non-negative"))
                } else {
                    Ok(x.sqrt())
                }
            }
            "abs" => Ok(one()?.abs()),
            "log" => {
                let x = one()?;
                if x <= 0.0 {
                    Err(self.error("log domain error: argument must be positive"))
                } else {
                    Ok(x.log10())
                }
            }
            "ln" => {
                let x = one()?;
                if x <= 0.0 {
                    Err(self.error("ln domain error: argument must be positive"))
                } else {
                    Ok(x.ln())
                }
            }
            "sin" => Ok(one()?.sin()),
            "cos" => Ok(one()?.cos()),
            "tan" => Ok(one()?.tan()),
            "round" => match a {
                [x] => Ok(x.round()),
                [x, digits] if digits.fract() == 0.0 && (-15.0..=15.0).contains(digits) => {
                    let p = 10f64.powf(*digits);
                    Ok((x * p).round() / p)
                }
                _ => {
                    Err(self
                        .error("round expects a number and optional integer digits from -15 to 15"))
                }
            },
            "min" if !a.is_empty() => Ok(a.iter().copied().fold(f64::INFINITY, f64::min)),
            "max" if !a.is_empty() => Ok(a.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
            "min" | "max" => Err(self.error(&format!("{name} expects at least one argument"))),
            _ => Err(self.error(&format!("unknown function {name:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn calc(s: &str) -> f64 {
        let out = run(json!({"expression":s})).unwrap();
        serde_json::from_str::<Value>(&out.content).unwrap()["result"]
            .as_f64()
            .unwrap()
    }
    #[test]
    fn precedence_and_power() {
        assert_eq!(calc("2 + 3 * 4"), 14.0);
        assert_eq!(calc("2^3^2"), 512.0);
        assert_eq!(calc("-(2+3)"), -5.0);
    }
    #[test]
    fn constants_and_functions() {
        assert!((calc("sin(pi/2)") - 1.0).abs() < 1e-12);
        assert_eq!(calc("max(2, sqrt(81), abs(-4))"), 9.0);
        assert_eq!(calc("round(2.345, 2)"), 2.35);
    }
    #[test]
    fn deep_nesting_is_an_error_not_a_stack_overflow() {
        let e = run(json!({"expression": "(".repeat(1000)})).unwrap_err();
        assert!(e.message.contains("nested too deeply"), "{}", e.message);
        let e = run(json!({"expression": format!("{}1", "-".repeat(1000))})).unwrap_err();
        assert!(e.message.contains("nested too deeply"), "{}", e.message);
        let e = run(json!({"expression": "1+".repeat(2000)})).unwrap_err();
        assert!(e.message.contains("longer than"), "{}", e.message);
    }
    #[test]
    fn precise_errors() {
        for (s, part) in [
            ("1/0", "division by zero"),
            ("sqrt(-1)", "domain error"),
            ("2 + @", "expected a number"),
            ("wat(1)", "unknown function"),
        ] {
            let e = run(json!({"expression":s})).unwrap_err();
            assert!(e.message.contains(part), "{}", e.message);
            assert!(e.message.contains("byte"));
        }
    }
}

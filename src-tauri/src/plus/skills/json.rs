//! Order-preserving JSON value with a printer that matches Python's
//! `json.dumps(obj, indent=2)` (ensure_ascii, no trailing newline).

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn str(s: impl Into<String>) -> J {
        J::Str(s.into())
    }

    pub fn int(n: i64) -> J {
        J::Num(n.to_string())
    }

    pub fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn dumps(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, depth: usize) {
        match self {
            J::Null => out.push_str("null"),
            J::Bool(true) => out.push_str("true"),
            J::Bool(false) => out.push_str("false"),
            J::Num(n) => out.push_str(n),
            J::Str(s) => write_str(out, s),
            J::Arr(items) if items.is_empty() => out.push_str("[]"),
            J::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, depth + 1);
                    item.write(out, depth + 1);
                }
                newline(out, depth);
                out.push(']');
            }
            J::Obj(items) if items.is_empty() => out.push_str("{}"),
            J::Obj(items) => {
                out.push('{');
                for (i, (k, v)) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, depth + 1);
                    write_str(out, k);
                    out.push_str(": ");
                    v.write(out, depth + 1);
                }
                newline(out, depth);
                out.push('}');
            }
        }
    }
}

fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) >= 0x80 => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn parse(text: &str) -> Result<J, String> {
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
        text,
    };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return Err(format!("trailing data at byte {}", p.i));
    }
    Ok(v)
}

const MAX_DEPTH: usize = 128;

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    text: &'a str,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &str) -> bool {
        if self.text[self.i..].starts_with(lit) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<J, String> {
        if depth > MAX_DEPTH {
            return Err("nesting too deep".into());
        }
        match self.s.get(self.i) {
            None => Err("unexpected end".into()),
            Some(b'{') => {
                self.i += 1;
                let mut items: Vec<(String, J)> = Vec::new();
                self.ws();
                if self.eat("}") {
                    return Ok(J::Obj(items));
                }
                loop {
                    self.ws();
                    let key = self.string()?;
                    self.ws();
                    if !self.eat(":") {
                        return Err(format!("expected ':' at byte {}", self.i));
                    }
                    self.ws();
                    let v = self.value(depth + 1)?;
                    // Python's json keeps the last duplicate key at its first position.
                    match items.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = v,
                        None => items.push((key, v)),
                    }
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("}") {
                        return Ok(J::Obj(items));
                    }
                    return Err(format!("expected ',' or '}}' at byte {}", self.i));
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.eat("]") {
                    return Ok(J::Arr(items));
                }
                loop {
                    self.ws();
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("]") {
                        return Ok(J::Arr(items));
                    }
                    return Err(format!("expected ',' or ']' at byte {}", self.i));
                }
            }
            Some(b'"') => Ok(J::Str(self.string()?)),
            Some(_) => {
                if self.eat("null") {
                    Ok(J::Null)
                } else if self.eat("true") {
                    Ok(J::Bool(true))
                } else if self.eat("false") {
                    Ok(J::Bool(false))
                } else {
                    self.number()
                }
            }
        }
    }

    fn number(&mut self) -> Result<J, String> {
        let start = self.i;
        while self.i < self.s.len()
            && matches!(
                self.s[self.i],
                b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
            )
        {
            self.i += 1;
        }
        let raw = &self.text[start..self.i];
        if raw.is_empty() || raw.parse::<f64>().is_err() {
            return Err(format!("invalid value at byte {start}"));
        }
        Ok(J::Num(raw.to_string()))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let end = self.i + 4;
        let digits = self
            .text
            .get(self.i..end)
            .ok_or_else(|| "bad \\u escape".to_string())?;
        let v = u32::from_str_radix(digits, 16).map_err(|_| "bad \\u escape".to_string())?;
        self.i = end;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, String> {
        if !self.eat("\"") {
            return Err(format!("expected string at byte {}", self.i));
        }
        let mut out = String::new();
        loop {
            let rest = &self.text[self.i..];
            let c = rest.chars().next().ok_or("unterminated string")?;
            self.i += c.len_utf8();
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let e = self.text[self.i..]
                        .chars()
                        .next()
                        .ok_or("unterminated escape")?;
                    self.i += e.len_utf8();
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xd800..0xdc00).contains(&hi) && self.eat("\\u") {
                                let lo = self.hex4()?;
                                0x10000 + ((hi - 0xd800) << 10) + lo.wrapping_sub(0xdc00)
                            } else {
                                hi
                            };
                            out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                        }
                        other => return Err(format!("bad escape \\{other}")),
                    }
                }
                c if (c as u32) < 0x20 => return Err("control character in string".into()),
                c => out.push(c),
            }
        }
    }
}

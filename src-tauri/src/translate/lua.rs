//! A reader for the Warframe wiki's data modules (`Module:…/data`), which are Lua table literals:
//! `local X = { … }` definitions followed by `return X` (or `return { … }`). Only data is read:
//! strings, numbers, booleans, nested tables and references to earlier `local` tables. Anything
//! after the `return` (a module's trailing `for` loop) is ignored.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Lua {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    /// Positional entries in order, and keyed entries in source order.
    Table { list: Vec<Lua>, map: Vec<(String, Lua)> },
}

impl Lua {
    /// A keyed entry of a table.
    pub fn get(&self, key: &str) -> Option<&Lua> {
        match self {
            Lua::Table { map, .. } => map.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// A positional entry of a table (0-based).
    pub fn at(&self, i: usize) -> Option<&Lua> {
        match self {
            Lua::Table { list, .. } => list.get(i),
            _ => None,
        }
    }

    pub fn list(&self) -> &[Lua] {
        match self {
            Lua::Table { list, .. } => list,
            _ => &[],
        }
    }

    pub fn entries(&self) -> &[(String, Lua)] {
        match self {
            Lua::Table { map, .. } => map,
            _ => &[],
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Lua::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Lua::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// `get(key)` as a string.
    pub fn s(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Lua::str)
    }

    /// `get(key)` as a whole number.
    pub fn n(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(Lua::num).map(|n| n as i64)
    }
}

/// Reads a module and returns the value it `return`s, or `None` if it isn't plain data.
pub fn parse_module(src: &str) -> Option<Lua> {
    let mut p = Parser { s: src.as_bytes(), i: 0, locals: HashMap::new() };
    loop {
        p.skip_space();
        if p.i >= p.s.len() {
            return None;
        }
        if p.keyword("local") {
            let name = p.ident()?;
            p.expect(b'=')?;
            let value = p.value()?;
            p.locals.insert(name, value);
        } else if p.keyword("return") {
            return p.value();
        } else {
            // A statement that isn't data: skip its line.
            while p.i < p.s.len() && p.s[p.i] != b'\n' {
                p.i += 1;
            }
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    locals: HashMap<String, Lua>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    /// Skips whitespace and comments (`-- line`, `--[[ block ]]`, `--[==[ block ]==]`).
    fn skip_space(&mut self) {
        loop {
            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.i += 1;
            }
            if !self.s[self.i..].starts_with(b"--") {
                return;
            }
            self.i += 2;
            if let Some(close) = self.long_bracket() {
                self.i = find(self.s, self.i, &close).map_or(self.s.len(), |end| end + close.len());
            } else {
                while self.peek().is_some_and(|c| c != b'\n') {
                    self.i += 1;
                }
            }
        }
    }

    /// At `[[` or `[=*[`: steps past it and returns the matching close (`]]`, `]=]`, …).
    fn long_bracket(&mut self) -> Option<Vec<u8>> {
        let rest = &self.s[self.i..];
        if rest.first() != Some(&b'[') {
            return None;
        }
        let eqs = rest[1..].iter().take_while(|&&c| c == b'=').count();
        if rest.get(1 + eqs) != Some(&b'[') {
            return None;
        }
        self.i += eqs + 2;
        let mut close = vec![b']'];
        close.extend(std::iter::repeat_n(b'=', eqs));
        close.push(b']');
        Some(close)
    }

    fn keyword(&mut self, word: &str) -> bool {
        let w = word.as_bytes();
        let end = self.i + w.len();
        let boundary = self.s.get(end).is_none_or(|c| !(c.is_ascii_alphanumeric() || *c == b'_'));
        if self.s[self.i..].starts_with(w) && boundary {
            self.i = end;
            self.skip_space();
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> Option<String> {
        self.skip_space();
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
            self.i += 1;
        }
        let word = std::str::from_utf8(&self.s[start..self.i]).ok()?;
        (!word.is_empty() && !word.as_bytes()[0].is_ascii_digit()).then(|| word.to_string())
    }

    fn expect(&mut self, c: u8) -> Option<()> {
        self.skip_space();
        (self.peek() == Some(c)).then(|| self.i += 1)
    }

    fn value(&mut self) -> Option<Lua> {
        self.skip_space();
        match self.peek()? {
            b'{' => self.table(),
            b'"' | b'\'' => self.string().map(Lua::Str),
            b'[' => {
                let close = self.long_bracket()?;
                let end = find(self.s, self.i, &close)?;
                let text = String::from_utf8_lossy(&self.s[self.i..end]).into_owned();
                self.i = end + close.len();
                Some(Lua::Str(text))
            }
            c if c == b'-' || c == b'.' || c.is_ascii_digit() => self.number(),
            _ => {
                let word = self.ident()?;
                Some(match word.as_str() {
                    "true" => Lua::Bool(true),
                    "false" => Lua::Bool(false),
                    "nil" => Lua::Nil,
                    _ => self.locals.get(&word).cloned().unwrap_or(Lua::Nil),
                })
            }
        }
    }

    fn number(&mut self) -> Option<Lua> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        if self.s[self.i..].starts_with(b"0x") || self.s[self.i..].starts_with(b"0X") {
            self.i += 2;
            let hex_start = self.i;
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.i += 1;
            }
            let n = i64::from_str_radix(std::str::from_utf8(&self.s[hex_start..self.i]).ok()?, 16).ok()?;
            return Some(Lua::Num(if self.s[start] == b'-' { -n } else { n } as f64));
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E'))
            || (self.peek().is_some_and(|c| c == b'-' || c == b'+') && matches!(self.s[self.i - 1], b'e' | b'E'))
        {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok().map(Lua::Num)
    }

    fn string(&mut self) -> Option<String> {
        let quote = self.peek()?;
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let c = self.peek()?;
            self.i += 1;
            match c {
                c if c == quote => break,
                b'\\' => {
                    let e = self.peek()?;
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'\n' => out.push(b'\n'),
                        d if d.is_ascii_digit() => {
                            // \ddd: a decimal byte value.
                            let mut v = u32::from(d - b'0');
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d) if d.is_ascii_digit() => {
                                        v = v * 10 + u32::from(d - b'0');
                                        self.i += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(v.min(255) as u8);
                        }
                        other => out.push(other),
                    }
                }
                c => out.push(c),
            }
        }
        Some(String::from_utf8_lossy(&out).into_owned())
    }

    fn table(&mut self) -> Option<Lua> {
        self.i += 1; // {
        let (mut list, mut map) = (Vec::new(), Vec::new());
        loop {
            self.skip_space();
            match self.peek()? {
                b'}' => {
                    self.i += 1;
                    return Some(Lua::Table { list, map });
                }
                b',' | b';' => self.i += 1,
                b'[' if !matches!(self.s.get(self.i + 1), Some(b'[' | b'=')) => {
                    // [key] = value
                    self.i += 1;
                    let key = match self.value()? {
                        Lua::Str(s) => s,
                        Lua::Num(n) => n.to_string(),
                        _ => return None,
                    };
                    self.expect(b']')?;
                    self.expect(b'=')?;
                    let value = self.value()?;
                    map.push((key, value));
                }
                c if c.is_ascii_alphabetic() || c == b'_' => {
                    // name = value, or a bare value (true, a local's name)
                    let save = self.i;
                    let name = self.ident()?;
                    self.skip_space();
                    if self.peek() == Some(b'=') && self.s.get(self.i + 1) != Some(&b'=') {
                        self.i += 1;
                        let value = self.value()?;
                        map.push((name, value));
                    } else {
                        self.i = save;
                        list.push(self.value()?);
                    }
                }
                _ => list.push(self.value()?),
            }
        }
    }
}

fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_wiki_shapes() {
        let src = r#"
-- a comment
--[[ a block
comment ]]
local Default = {
	{ {Part = "Chassis Blueprint", Chance = 0.3872} },
}
local Data = {
	Nova = Default,
	["Ack & Brunt"] = {
		BPCost = 15000, Name = 'Ack & Brunt Blueprint', -- trailing
		Parts = { { Count = 5, Name = "Detonite Injector", Type = "Resource" } },
		Offer = { "Kuva", "Resource", 10, 5000, Timer = 604800 },
		Cost = { ["Kuva"] = 100000 },
		Neg = -1.5e2, Flag = true, Hex = 0x1F,
		Text = "say \"hi\"\n",
	},
}
for k,v in pairs(Data) do v.Name = k end
return Data
"#;
        let data = parse_module(src).unwrap();
        let nova = data.get("Nova").unwrap();
        assert_eq!(nova.at(0).and_then(|r| r.at(0)).and_then(|p| p.s("Part")), Some("Chassis Blueprint"));
        let ack = data.get("Ack & Brunt").unwrap();
        assert_eq!((ack.n("BPCost"), ack.s("Name")), (Some(15000), Some("Ack & Brunt Blueprint")));
        assert_eq!(ack.get("Parts").unwrap().at(0).unwrap().n("Count"), Some(5));
        let offer = ack.get("Offer").unwrap();
        assert_eq!((offer.at(0).and_then(Lua::str), offer.at(3).and_then(Lua::num), offer.n("Timer")), (Some("Kuva"), Some(5000.0), Some(604800)));
        assert_eq!(ack.get("Cost").unwrap().n("Kuva"), Some(100000));
        assert_eq!((ack.get("Neg"), ack.get("Flag"), ack.n("Hex")), (Some(&Lua::Num(-150.0)), Some(&Lua::Bool(true)), Some(31)));
        assert_eq!(ack.s("Text"), Some("say \"hi\"\n"));
    }

    #[test]
    fn return_of_a_literal() {
        let data = parse_module("return { Vendors = { Hok = { Name = \"Hok\" } } }").unwrap();
        assert_eq!(data.get("Vendors").and_then(|v| v.get("Hok")).and_then(|h| h.s("Name")), Some("Hok"));
    }
}

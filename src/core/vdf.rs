//! Valve KeyValues files: the binary format of `shortcuts.vdf` and the text
//! format of `config.vdf`. Maps keep their entry order and value types, so a
//! file we only partly change is written back otherwise unchanged. Text values
//! keep their escape sequences verbatim for the same reason.

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Map(Map),
    Str(String),
    Int32(i32),
    Float32(f32),
    Pointer(i32),
    Color(i32),
    UInt64(u64),
    Int64(i64),
}

/// Ordered key/value entries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map(pub Vec<(String, Value)>);

impl Map {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn get_int(&self, key: &str) -> Option<i64> {
        match self.get(key)? {
            Value::Int32(v) | Value::Pointer(v) | Value::Color(v) => Some(*v as i64),
            Value::Int64(v) => Some(*v),
            Value::UInt64(v) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn get_map(&self, key: &str) -> Option<&Map> {
        match self.get(key)? {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Replace the value in place, or append the entry.
    pub fn insert(&mut self, key: &str, value: Value) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value,
            None => self.0.push((key.to_string(), value)),
        }
    }

    /// The map under `key`, matched in any case (as Steam treats KeyValues keys).
    pub fn get_map_ignore_case(&self, key: &str) -> Option<&Map> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .and_then(|(_, v)| match v {
                Value::Map(m) => Some(m),
                _ => None,
            })
    }

    /// The map under `key` (matched in any case, as Steam treats KeyValues
    /// keys), created or replacing a non-map value if needed.
    pub fn map_entry_ignore_case(&mut self, key: &str) -> &mut Map {
        let index = match self.0.iter().position(|(k, _)| k.eq_ignore_ascii_case(key)) {
            Some(i) => i,
            None => {
                self.0.push((key.to_string(), Value::Map(Map::default())));
                self.0.len() - 1
            }
        };
        let slot = &mut self.0[index].1;
        if !matches!(slot, Value::Map(_)) {
            *slot = Value::Map(Map::default());
        }
        match slot {
            Value::Map(m) => m,
            _ => unreachable!("slot was just made a map"),
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.0.iter().map(|(_, v)| v)
    }
}

// ── Binary format ────────────────────────────────────────────────────────

const T_MAP: u8 = 0x00;
const T_STR: u8 = 0x01;
const T_INT32: u8 = 0x02;
const T_FLOAT32: u8 = 0x03;
const T_POINTER: u8 = 0x04;
const T_COLOR: u8 = 0x06;
const T_UINT64: u8 = 0x07;
const T_END: u8 = 0x08;
const T_INT64: u8 = 0x0A;
const T_END_ALT: u8 = 0x0B;

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.data.get(self.pos).ok_or("unexpected end of file")?;
        self.pos += 1;
        Ok(b)
    }

    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let slice = self
            .data
            .get(self.pos..self.pos + N)
            .ok_or("unexpected end of file")?;
        self.pos += N;
        Ok(slice.try_into().unwrap())
    }

    fn cstring(&mut self) -> Result<String, String> {
        let rest = &self.data[self.pos..];
        let end = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or("unterminated string")?;
        self.pos += end + 1;
        Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
    }

    fn map(&mut self) -> Result<Map, String> {
        let mut map = Map::default();
        loop {
            let kind = self.byte()?;
            if kind == T_END || kind == T_END_ALT {
                return Ok(map);
            }
            let key = self.cstring()?;
            let value = match kind {
                T_MAP => Value::Map(self.map()?),
                T_STR => Value::Str(self.cstring()?),
                T_INT32 => Value::Int32(i32::from_le_bytes(self.bytes()?)),
                T_FLOAT32 => Value::Float32(f32::from_le_bytes(self.bytes()?)),
                T_POINTER => Value::Pointer(i32::from_le_bytes(self.bytes()?)),
                T_COLOR => Value::Color(i32::from_le_bytes(self.bytes()?)),
                T_UINT64 => Value::UInt64(u64::from_le_bytes(self.bytes()?)),
                T_INT64 => Value::Int64(i64::from_le_bytes(self.bytes()?)),
                other => {
                    return Err(format!(
                        "unsupported value type 0x{other:02x} at byte {}",
                        self.pos
                    ))
                }
            };
            map.0.push((key, value));
        }
    }
}

pub fn parse_binary(data: &[u8]) -> Result<Map, String> {
    Reader { data, pos: 0 }.map()
}

fn write_binary_map(map: &Map, out: &mut Vec<u8>) {
    for (key, value) in &map.0 {
        let (kind, payload): (u8, Vec<u8>) = match value {
            Value::Map(m) => {
                let mut inner = Vec::new();
                write_binary_map(m, &mut inner);
                (T_MAP, inner)
            }
            Value::Str(s) => (T_STR, [s.as_bytes(), &[0]].concat()),
            Value::Int32(v) => (T_INT32, v.to_le_bytes().to_vec()),
            Value::Float32(v) => (T_FLOAT32, v.to_le_bytes().to_vec()),
            Value::Pointer(v) => (T_POINTER, v.to_le_bytes().to_vec()),
            Value::Color(v) => (T_COLOR, v.to_le_bytes().to_vec()),
            Value::UInt64(v) => (T_UINT64, v.to_le_bytes().to_vec()),
            Value::Int64(v) => (T_INT64, v.to_le_bytes().to_vec()),
        };
        out.push(kind);
        out.extend_from_slice(key.as_bytes());
        out.push(0);
        out.extend_from_slice(&payload);
    }
    out.push(T_END);
}

pub fn to_binary(map: &Map) -> Vec<u8> {
    let mut out = Vec::new();
    write_binary_map(map, &mut out);
    out
}

// ── Text format ──────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum Token {
    Str(String),
    Open,
    Close,
}

fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '/' if chars.peek() == Some(&'/') => {
                chars.by_ref().take_while(|&c| c != '\n').for_each(drop);
            }
            // Platform conditionals such as [$WIN32] are ignored.
            '[' => {
                chars.by_ref().take_while(|&c| c != ']').for_each(drop);
            }
            '"' => {
                let mut s = String::new();
                loop {
                    match chars.next().ok_or("unterminated string")? {
                        '"' => break,
                        '\\' => {
                            s.push('\\');
                            s.push(chars.next().ok_or("unterminated string")?);
                        }
                        other => s.push(other),
                    }
                }
                tokens.push(Token::Str(s));
            }
            first => {
                let mut s = String::from(first);
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || matches!(next, '{' | '}' | '"') {
                        break;
                    }
                    s.push(next);
                    chars.next();
                }
                tokens.push(Token::Str(s));
            }
        }
    }
    Ok(tokens)
}

/// Insert, merging into an existing map with the same key (Steam's semantics).
fn merge_insert(map: &mut Map, key: String, value: Value) {
    match (map.0.iter_mut().find(|(k, _)| *k == key), value) {
        (Some((_, Value::Map(existing))), Value::Map(incoming)) => {
            incoming
                .0
                .into_iter()
                .for_each(|(k, v)| merge_insert(existing, k, v));
        }
        (Some((_, slot)), value) => *slot = value,
        (None, value) => map.0.push((key, value)),
    }
}

fn parse_entries(
    tokens: &mut std::iter::Peekable<std::vec::IntoIter<Token>>,
    nested: bool,
) -> Result<Map, String> {
    let mut map = Map::default();
    loop {
        let key = match tokens.next() {
            Some(Token::Str(k)) => k,
            Some(Token::Close) if nested => return Ok(map),
            None if !nested => return Ok(map),
            Some(other) => return Err(format!("unexpected {other:?}")),
            None => return Err("unexpected end of file".into()),
        };
        let value = match tokens.next() {
            Some(Token::Str(v)) => Value::Str(v),
            Some(Token::Open) => Value::Map(parse_entries(tokens, true)?),
            other => return Err(format!("missing value for {key:?}, got {other:?}")),
        };
        merge_insert(&mut map, key, value);
    }
}

pub fn parse_text(text: &str) -> Result<Map, String> {
    let mut tokens = tokenize(text.trim_start_matches('\u{feff}'))?
        .into_iter()
        .peekable();
    parse_entries(&mut tokens, false)
}

fn write_text_map(map: &Map, level: usize, out: &mut String) {
    let indent = "\t".repeat(level);
    for (key, value) in &map.0 {
        match value {
            Value::Map(m) => {
                out.push_str(&format!("{indent}\"{key}\"\n{indent}{{\n"));
                write_text_map(m, level + 1, out);
                out.push_str(&format!("{indent}}}\n"));
            }
            Value::Str(s) => out.push_str(&format!("{indent}\"{key}\"\t\t\"{s}\"\n")),
            other => out.push_str(&format!(
                "{indent}\"{key}\"\t\t\"{}\"\n",
                scalar_text(other)
            )),
        }
    }
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::Int32(v) | Value::Pointer(v) | Value::Color(v) => v.to_string(),
        Value::Float32(v) => v.to_string(),
        Value::UInt64(v) => v.to_string(),
        Value::Int64(v) => v.to_string(),
        Value::Str(s) => s.clone(),
        Value::Map(_) => String::new(),
    }
}

/// Text KeyValues in Steam's own layout (tab indents, `"key"\t\t"value"`).
pub fn to_text(map: &Map) -> String {
    let mut out = String::new();
    write_text_map(map, 0, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_can_be_looked_up_in_any_case() {
        let data = parse_text(r#""Software" { "valve" { "x" "1" } }"#).unwrap();
        let valve = data
            .get_map("Software")
            .unwrap()
            .get_map_ignore_case("Valve");
        assert_eq!(valve.and_then(|m| m.get_str("x")), Some("1"));
    }

    /// Python `vdf.binary_dump` output for one shortcut, captured before the port.
    const GOLDEN: &[u8] = include_bytes!("../../testdata/shortcuts.vdf");

    #[test]
    fn binary_round_trip_is_byte_identical_to_python_vdf() {
        let root = parse_binary(GOLDEN).unwrap();
        let shortcut = root.get_map("shortcuts").unwrap().get_map("0").unwrap();
        assert_eq!(shortcut.get_str("AppName"), Some("Hollow Knight"));
        assert_eq!(shortcut.get_int("appid"), Some(-1630074248));
        assert_eq!(
            shortcut.get_map("tags").unwrap().get_str("0"),
            Some("golden")
        );
        assert_eq!(to_binary(&root), GOLDEN);
    }

    #[test]
    fn binary_rejects_truncated_files() {
        assert!(parse_binary(&GOLDEN[..40]).is_err());
    }

    #[test]
    fn text_parses_nesting_comments_and_merges_duplicates() {
        let text = "// header\n\"Root\"\n{\n\t\"a\"\t\t\"1\"\n\t\"path\"\t\"C:\\\\Games\"\n\t\"Sub\" { \"x\" \"y\" }\n\t\"Sub\" { \"z\" \"w\" }\n\tbare value [$WIN32]\n}\n";
        let map = parse_text(text).unwrap();
        let root = map.get_map("Root").unwrap();
        assert_eq!(root.get_str("a"), Some("1"));
        assert_eq!(root.get_str("path"), Some("C:\\\\Games"));
        assert_eq!(root.get_str("bare"), Some("value"));
        let sub = root.get_map("Sub").unwrap();
        assert_eq!((sub.get_str("x"), sub.get_str("z")), (Some("y"), Some("w")));
    }

    #[test]
    fn text_round_trip_keeps_escapes_and_layout() {
        let text = "\"Root\"\n{\n\t\"path\"\t\t\"C:\\\\Games \\\"x\\\"\"\n\t\"Sub\"\n\t{\n\t\t\"k\"\t\t\"v\"\n\t}\n}\n";
        assert_eq!(to_text(&parse_text(text).unwrap()), text);
    }

    #[test]
    fn map_entry_creates_nested_maps() {
        let mut map = Map::default();
        map.map_entry_ignore_case("A")
            .map_entry_ignore_case("B")
            .insert("k", Value::Str("v".into()));
        map.map_entry_ignore_case("a")
            .insert("x", Value::Str("y".into()));
        let a = map.get_map("A").unwrap();
        assert_eq!(a.get_map("B").unwrap().get_str("k"), Some("v"));
        assert_eq!(a.get_str("x"), Some("y"));
    }
}

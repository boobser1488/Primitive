//! Blockbench projects: read, and written.
//!
//! ## Why the game reads them now
//!
//! This module began as an exporter -- the animals written out so somebody
//! could open them. The models live in `assets/models` as `.bbmodel` files
//! now (see `logic::models`), so the format is the source rather than a
//! copy, and the same code has to go both ways: the file the game ships
//! is the file Blockbench saves, and `--export-models` writes what the
//! game has loaded.
//!
//! ## Why a JSON reader of its own
//!
//! The workspace has no JSON crate -- `serde` is there for the wire and
//! `toml` for the config -- and a `.bbmodel` is read a couple of dozen
//! times at start-up. Two hundred lines of recursive descent against a
//! dependency compiled into every build for that: the grammar has not
//! changed since 2006, and an error here can name the line in the file
//! a person just saved, which is the one thing a player editing a model
//! needs from it.
//!
//! Rejected: `serde_json` with derived structs. It would also silently
//! drop every field the structs did not name, and Blockbench writes
//! dozens -- the reader here keeps the whole tree and each caller takes
//! what it understands.
//!
//! ## What is read
//!
//! Only what decides geometry: `resolution`, each cube's `name`, `from`,
//! `to`, `origin`, `rotation` and its six faces' `uv` and `texture`, the
//! `outliner`'s groups (a name and an `origin` -- or, from Blockbench 5,
//! the same two in the `groups` list -- and which cubes are
//! inside), and each texture's `name`. What those *mean* for an animal or
//! a stool is `logic::models`' business, not this file's.

use std::path::{Path, PathBuf};

use primitive_shared::animals::Species;

/// A JSON value, kept whole.
///
/// Objects keep their keys in file order, as a list: a model file is a few
/// hundred keys, a linear lookup is nothing, and writing a file back in
/// the order Blockbench wrote it is what keeps a diff of an edit readable.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Parses a whole document, or says on which line it stopped making
    /// sense.
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut parser = Parser { bytes: text.as_bytes(), at: 0 };
        parser.space();
        let value = parser.value()?;
        parser.space();
        if parser.at != parser.bytes.len() {
            return Err(parser.error("something after the end of the document"));
        }
        Ok(value)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// For a test that edits a file the way a person would.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        match self {
            Json::Object(pairs) => pairs.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Json]> {
        match self {
            Json::List(items) => Some(items),
            _ => None,
        }
    }

    /// The document as text: short values on one line, long ones broken
    /// one item a line.
    ///
    /// **Indented, and not the one line the first exporter wrote.** These
    /// files are in version control, and a change to one ear of one animal
    /// should be a one-line diff rather than a rewritten megabyte line.
    pub fn pretty(&self) -> String {
        let mut out = String::new();
        self.write_pretty(&mut out, 0);
        out.push('\n');
        out
    }

    fn write_pretty(&self, out: &mut String, indent: usize) {
        const WIDTH: usize = 110;
        let compact = self.compact();
        let nested = match self {
            Json::List(items) => !items.is_empty(),
            Json::Object(pairs) => !pairs.is_empty(),
            _ => false,
        };
        if !nested || indent + compact.len() <= WIDTH {
            out.push_str(&compact);
            return;
        }
        let pad = " ".repeat(indent + 2);
        match self {
            Json::List(items) => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    out.push_str(&pad);
                    item.write_pretty(out, indent + 2);
                    out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
                }
                out.push_str(&" ".repeat(indent));
                out.push(']');
            }
            Json::Object(pairs) => {
                out.push_str("{\n");
                for (i, (key, value)) in pairs.iter().enumerate() {
                    out.push_str(&pad);
                    out.push_str(&quoted(key));
                    out.push_str(": ");
                    value.write_pretty(out, indent + 2);
                    out.push_str(if i + 1 < pairs.len() { ",\n" } else { "\n" });
                }
                out.push_str(&" ".repeat(indent));
                out.push('}');
            }
            _ => unreachable!("only a list or an object is nested"),
        }
    }

    fn compact(&self) -> String {
        match self {
            Json::Null => "null".into(),
            Json::Bool(b) => b.to_string(),
            Json::Number(n) => number(*n),
            Json::Text(s) => quoted(s),
            Json::List(items) => {
                let inner: Vec<String> = items.iter().map(Json::compact).collect();
                format!("[{}]", inner.join(", "))
            }
            Json::Object(pairs) => {
                let inner: Vec<String> =
                    pairs.iter().map(|(k, v)| format!("{}: {}", quoted(k), v.compact())).collect();
                format!("{{{}}}", inner.join(", "))
            }
        }
    }
}

/// A number the way a person would type it into Blockbench.
///
/// **Rounded to a millionth**, because the numbers going in were computed
/// -- a from is a centre less half a size -- and `-4.8` computed that way
/// is `-4.800000000000001`. The models are written in quarters and tenths
/// of a sixteenth, so a millionth loses nothing, and
/// `models::a_model_read_back_from_its_file_is_the_model_that_was_written`
/// is what would say so if it did.
fn number(n: f64) -> String {
    let n = (n * 1e6).round() / 1e6;
    if n == 0.0 {
        return "0".into();
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    let text = format!("{n:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
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
    out
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn error(&self, what: &str) -> String {
        let line = 1 + self.bytes[..self.at.min(self.bytes.len())].iter().filter(|&&b| b == b'\n').count();
        format!("line {line}: {what}")
    }

    fn space(&mut self) {
        while self.at < self.bytes.len() && matches!(self.bytes[self.at], b' ' | b'\t' | b'\n' | b'\r') {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Result<(), String> {
        if self.bytes.get(self.at) == Some(&byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected '{}'", byte as char)))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        match self.bytes.get(self.at) {
            Some(b'{') => self.object(),
            Some(b'[') => self.list(),
            Some(b'"') => self.text().map(Json::Text),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("expected a value")),
            None => Err(self.error("the document ends in the middle")),
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.error("expected a value"))
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        while self.at < self.bytes.len() && matches!(self.bytes[self.at], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
            self.at += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).unwrap_or("");
        text.parse::<f64>().map(Json::Number).map_err(|_| self.error(&format!("'{text}' is not a number")))
    }

    fn text(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(&byte) = self.bytes.get(self.at) else {
                return Err(self.error("a string is never closed"));
            };
            self.at += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let Some(&escape) = self.bytes.get(self.at) else {
                        return Err(self.error("a string is never closed"));
                    };
                    self.at += 1;
                    match escape {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let mut code = self.hex4()?;
                            // A character outside the first plane comes as a
                            // pair of escapes, and the pair is one character.
                            if (0xD800..0xDC00).contains(&code) && self.bytes[self.at..].starts_with(b"\\u") {
                                self.at += 2;
                                let low = self.hex4()?;
                                code = 0x10000 + ((code - 0xD800) << 10) + (low.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            let c = char::from_u32(code).unwrap_or('\u{FFFD}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err(self.error("an unknown escape in a string")),
                    }
                }
                _ => out.push(byte),
            }
        }
        // The input was a `str`, and every escape above wrote whole
        // characters, so this cannot fail.
        String::from_utf8(out).map_err(|_| self.error("a string is not UTF-8"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self.bytes.get(self.at..self.at + 4).ok_or_else(|| self.error("a short \\u escape"))?;
        let text = std::str::from_utf8(digits).unwrap_or("");
        let code = u32::from_str_radix(text, 16).map_err(|_| self.error("a bad \\u escape"))?;
        self.at += 4;
        Ok(code)
    }

    fn list(&mut self) -> Result<Json, String> {
        self.eat(b'[')?;
        let mut items = Vec::new();
        self.space();
        if self.bytes.get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(Json::List(items));
        }
        loop {
            self.space();
            items.push(self.value()?);
            self.space();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::List(items));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.eat(b'{')?;
        let mut pairs = Vec::new();
        self.space();
        if self.bytes.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Json::Object(pairs));
        }
        loop {
            self.space();
            let key = self.text()?;
            self.space();
            self.eat(b':')?;
            self.space();
            let value = self.value()?;
            pairs.push((key, value));
            self.space();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(pairs));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }
}

/// Blockbench's names for the six faces, in the mesher's face order:
/// 0 +Y, 1 -Y, 2 +X, 3 -X, 4 +Z, 5 -Z.
pub const FACE_NAMES: [&str; 6] = ["up", "down", "east", "west", "south", "north"];

/// A project, as far as geometry goes.
#[derive(Debug, Clone)]
pub struct Document {
    /// The texture size the face `uv`s are measured in.
    pub resolution: [f64; 2],
    /// Every cube, in file order -- which is the order they are drawn in.
    pub elements: Vec<Element>,
    /// Each texture's `name`, by index.
    pub textures: Vec<String>,
}

/// One cube.
#[derive(Debug, Clone)]
pub struct Element {
    pub name: String,
    pub from: [f64; 3],
    pub to: [f64; 3],
    /// The point Blockbench turns the cube about.
    pub origin: [f64; 3],
    /// Degrees about x, y and z.
    pub rotation: [f64; 3],
    /// In the mesher's face order (`FACE_NAMES`); `None` where the file has
    /// no such face.
    pub faces: [Option<Face>; 6],
    /// The groups the cube is inside, outermost first.
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone)]
pub struct Face {
    /// `[x1, y1, x2, y2]` in `resolution` units.
    pub uv: [f64; 4],
    /// Index into `Document::textures`.
    pub texture: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub name: String,
    pub origin: [f64; 3],
}

impl Document {
    pub fn parse(text: &str) -> Result<Document, String> {
        Document::from_json(&Json::parse(text)?)
    }

    pub fn from_json(json: &Json) -> Result<Document, String> {
        let resolution = match json.get("resolution") {
            Some(r) => [
                r.get("width").and_then(Json::as_f64).unwrap_or(16.0),
                r.get("height").and_then(Json::as_f64).unwrap_or(16.0),
            ],
            None => [16.0, 16.0],
        };

        let mut texture_uuids = Vec::new();
        let mut textures = Vec::new();
        for texture in json.get("textures").and_then(Json::as_list).unwrap_or(&[]) {
            textures.push(texture.get("name").and_then(Json::as_str).unwrap_or("").to_string());
            texture_uuids.push(texture.get("uuid").and_then(Json::as_str).unwrap_or("").to_string());
        }

        // Which groups each cube is in, from the outliner. A cube the
        // outliner does not mention is in none, which is what Blockbench
        // does with it too.
        //
        // **Two spellings of a group.** Blockbench 4 wrote a group whole in
        // the outliner -- name, origin, children. Blockbench 5 writes the
        // outliner as bare `{uuid, children}` and keeps each group's name and
        // origin in a top-level `groups` list, and the first model a player
        // saved from it came back with every part of every gull `still`,
        // because the names this reads the gait from were somewhere else.
        let declared = json.get("groups").and_then(Json::as_list).unwrap_or(&[]);
        let mut chains: Vec<(String, Vec<Group>)> = Vec::new();
        fn walk(node: &Json, declared: &[Json], above: &mut Vec<Group>, out: &mut Vec<(String, Vec<Group>)>) {
            match node {
                Json::Text(uuid) => out.push((uuid.clone(), above.clone())),
                Json::Object(_) => {
                    let uuid = node.get("uuid").and_then(Json::as_str);
                    let group = declared
                        .iter()
                        .find(|g| uuid.is_some() && g.get("uuid").and_then(Json::as_str) == uuid)
                        .unwrap_or(node);
                    above.push(Group {
                        name: group.get("name").and_then(Json::as_str).unwrap_or("").to_string(),
                        origin: triple(group.get("origin")).unwrap_or([0.0; 3]),
                    });
                    for child in node.get("children").and_then(Json::as_list).unwrap_or(&[]) {
                        walk(child, declared, above, out);
                    }
                    above.pop();
                }
                _ => {}
            }
        }
        for node in json.get("outliner").and_then(Json::as_list).unwrap_or(&[]) {
            walk(node, declared, &mut Vec::new(), &mut chains);
        }

        let mut elements = Vec::new();
        for (index, element) in json.get("elements").and_then(Json::as_list).unwrap_or(&[]).iter().enumerate() {
            let name = element.get("name").and_then(Json::as_str).unwrap_or("").to_string();
            let what = if name.is_empty() { format!("element {index}") } else { format!("\"{name}\"") };
            if let Some(kind) = element.get("type").and_then(Json::as_str) {
                if kind != "cube" {
                    return Err(format!("{what} is a {kind}, and only cubes can be drawn"));
                }
            }
            let from = triple(element.get("from")).ok_or_else(|| format!("{what} has no \"from\""))?;
            let to = triple(element.get("to")).ok_or_else(|| format!("{what} has no \"to\""))?;
            let origin = triple(element.get("origin")).unwrap_or([0.0; 3]);
            let rotation = triple(element.get("rotation")).unwrap_or([0.0; 3]);
            let mut faces: [Option<Face>; 6] = Default::default();
            if let Some(all) = element.get("faces") {
                for (slot, face_name) in faces.iter_mut().zip(FACE_NAMES) {
                    let Some(face) = all.get(face_name) else { continue };
                    let uv = match face.get("uv").and_then(Json::as_list) {
                        Some([a, b, c, d]) => [a, b, c, d].map(|n| n.as_f64().unwrap_or(0.0)),
                        _ => [0.0; 4],
                    };
                    let texture = match face.get("texture") {
                        Some(Json::Number(n)) if *n >= 0.0 => Some(*n as usize),
                        Some(Json::Text(uuid)) => texture_uuids.iter().position(|u| u == uuid),
                        _ => None,
                    };
                    *slot = Some(Face { uv, texture });
                }
            }
            let uuid = element.get("uuid").and_then(Json::as_str).unwrap_or("");
            let groups = chains.iter().find(|(u, _)| u == uuid).map(|(_, g)| g.clone()).unwrap_or_default();
            elements.push(Element { name, from, to, origin, rotation, faces, groups });
        }
        Ok(Document { resolution, elements, textures })
    }
}

fn triple(value: Option<&Json>) -> Option<[f64; 3]> {
    match value?.as_list()? {
        [a, b, c] => Some([a.as_f64()?, b.as_f64()?, c.as_f64()?]),
        _ => None,
    }
}

// ---- writing ----

pub fn numbers(values: &[f64]) -> Json {
    Json::List(values.iter().map(|&n| Json::Number(n)).collect())
}

pub fn text(s: &str) -> Json {
    Json::Text(s.to_string())
}

pub fn object(pairs: Vec<(&str, Json)>) -> Json {
    Json::Object(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// A stable identifier.
///
/// Blockbench wants a UUID per element and group and does not care what
/// it is, as long as nothing else in the file has the same one. Derived
/// from the model and the index rather than rolled, so exporting twice
/// gives the same file -- a model that changes every time it is written
/// is a model nobody can keep in version control.
pub fn uuid(seed: u32, index: usize) -> String {
    format!("00000000-0000-4000-8000-{:06x}{:06x}", seed & 0xFF_FFFF, index & 0xFF_FFFF)
}

/// One cube: its faces as `(uv, texture index)` in the mesher's order.
pub fn element(name: &str, uuid: &str, from: [f64; 3], to: [f64; 3], origin: [f64; 3], faces: [([f64; 4], usize); 6]) -> Json {
    let faces = Json::Object(
        FACE_NAMES
            .iter()
            .zip(faces)
            .map(|(face_name, (uv, texture))| {
                (face_name.to_string(), object(vec![("uv", numbers(&uv)), ("texture", Json::Number(texture as f64))]))
            })
            .collect(),
    );
    object(vec![
        ("name", text(name)),
        ("type", text("cube")),
        ("uuid", text(uuid)),
        ("from", numbers(&from)),
        ("to", numbers(&to)),
        ("origin", numbers(&origin)),
        ("faces", faces),
    ])
}

/// The same cube turned, `rotation` degrees about x, y and z, about its
/// `origin` -- which is how Blockbench writes a turned cube.
pub fn turned(element: Json, rotation: [f64; 3]) -> Json {
    match element {
        Json::Object(mut pairs) => {
            pairs.push(("rotation".to_string(), numbers(&rotation)));
            Json::Object(pairs)
        }
        other => other,
    }
}

pub fn group(name: &str, uuid: &str, origin: [f64; 3], children: Vec<Json>) -> Json {
    object(vec![
        ("name", text(name)),
        ("uuid", text(uuid)),
        ("origin", numbers(&origin)),
        ("children", Json::List(children)),
    ])
}

/// A texture, either pointing at a picture beside the project
/// (`relative_path`, which is how Blockbench finds it and saves an edit of
/// it back) or carrying the picture inside it (`source`).
pub fn texture(name: &str, uuid: &str, relative_path: Option<&str>, source: Option<String>) -> Json {
    let mut pairs = vec![
        ("name", text(name)),
        ("uuid", text(uuid)),
        ("id", text("0")),
        ("path", text("")),
        ("mode", text("bitmap")),
        ("render_mode", text("default")),
        ("visible", Json::Bool(true)),
    ];
    if let Some(path) = relative_path {
        pairs.push(("relative_path", text(path)));
    }
    if let Some(source) = source {
        pairs.push(("source", Json::Text(source)));
    }
    object(pairs)
}

pub fn project(name: &str, resolution: [u32; 2], elements: Vec<Json>, outliner: Vec<Json>, textures: Vec<Json>) -> Json {
    object(vec![
        (
            "meta",
            object(vec![
                ("format_version", text("4.5")),
                ("model_format", text("free")),
                ("box_uv", Json::Bool(false)),
            ]),
        ),
        ("name", text(name)),
        ("model_identifier", text(name)),
        (
            "resolution",
            object(vec![
                ("width", Json::Number(resolution[0] as f64)),
                ("height", Json::Number(resolution[1] as f64)),
            ]),
        ),
        ("elements", Json::List(elements)),
        ("outliner", Json::List(outliner)),
        ("textures", Json::List(textures)),
    ])
}

/// Writes every animal as a Blockbench project into `dir`, for
/// `--export-models`.
///
/// **Self-contained**, unlike the files in `assets/models`: the sheet is
/// carried inside as base64, so a file handed to somebody on its own opens
/// with its textures on. The shipped files point at the sheet instead, so
/// that painting in Blockbench paints the picture the game loads.
pub fn write_models(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    for &species in Species::ALL {
        let path = dir.join(format!("{}.bbmodel", species.name()));
        let sheet = crate::logic::obj_export::sheet_of(species);
        let source = format!("data:image/png;base64,{}", read_sheet(sheet)?);
        let json = crate::logic::models::animal_project(species, crate::logic::animal_model::parts(species), Some(source));
        std::fs::write(&path, json.pretty())?;
        written.push(path);
    }
    Ok(written)
}

/// The sheet, as base64, so the project carries its own texture.
fn read_sheet(file: &str) -> anyhow::Result<String> {
    let on_disk = Path::new("assets/textures").join(file);
    let bytes = if on_disk.exists() {
        std::fs::read(&on_disk)?
    } else if let Some(bytes) = crate::embedded::texture(file) {
        bytes.to_vec()
    } else {
        anyhow::bail!("no picture called {file}");
    };
    Ok(base64(&bytes))
}

/// Base64, written out rather than pulled in.
///
/// Twenty lines against a dependency that would be compiled into every
/// build of the game for the sake of a developer command that writes
/// four files. The encoding is fixed and has not changed since 1987.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let triple = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            // The tail is padded with '=' rather than with the zeroes
            // the bytes above were padded with: a decoder has to be able
            // to tell one trailing zero byte from none.
            if i <= chunk.len() {
                out.push(ALPHABET[(triple >> (18 - i * 6) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_encoding_everything_else_uses() {
        // The three cases: a whole triple, one byte over, two over.
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b""), "");
        // A PNG's first eight bytes, which is what actually goes through
        // here.
        assert_eq!(base64(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]), "iVBORw0KGgo=");
    }

    #[test]
    fn what_is_written_as_json_reads_back_as_the_same_json() {
        let value = object(vec![
            ("name", text("ear \"left\" \\ щ\n")),
            ("numbers", numbers(&[0.0, -4.8, 16.0, 0.125, -0.3])),
            ("empty", Json::List(vec![])),
            ("nothing", Json::Null),
            ("deep", object(vec![("list", Json::List(vec![Json::Bool(true); 60]))])),
        ]);
        assert_eq!(Json::parse(&value.pretty()), Ok(value));
    }

    #[test]
    fn a_broken_file_is_named_by_the_line_it_breaks_on() {
        let error = Json::parse("{\n  \"a\": [1, 2,\n  \"b\": }\n").unwrap_err();
        assert!(error.starts_with("line 3"), "{error}");
        assert_eq!(Json::parse("\"\\ud83d\\ude00\""), Ok(Json::Text("\u{1F600}".into())));
    }

    #[test]
    fn a_computed_number_is_written_the_way_a_person_would_type_it() {
        assert_eq!(number(-0.3 - 4.5), "-4.8");
        assert_eq!(number(16.0), "16");
        assert_eq!(number(-0.0), "0");
        assert_eq!(number(0.125), "0.125");
    }

    #[test]
    fn every_animal_is_a_project_that_names_its_boxes_and_carries_its_sheet() {
        let dir = std::env::temp_dir().join("primitive-bbmodel-test");
        let _ = std::fs::remove_dir_all(&dir);
        let written = write_models(&dir).expect("exported");
        assert_eq!(written.len(), Species::ALL.len());

        for (&species, path) in Species::ALL.iter().zip(&written) {
            let text = std::fs::read_to_string(path).expect("readable");
            let doc = Document::parse(&text).expect("the export reads back");
            let parts = crate::logic::animal_model::parts(species);
            let names: Vec<&str> = doc.elements.iter().map(|e| e.name.as_str()).collect();
            let expected: Vec<&str> = parts.iter().map(|p| p.name).collect();
            assert_eq!(names, expected, "{} is not named box for box", species.name());
            // Self-contained: the sheet rides along.
            assert!(
                text.contains("data:image/png;base64,iVBORw0KGgo"),
                "{} does not carry its texture",
                species.name()
            );
            assert!(
                doc.elements.iter().all(|e| e.faces.iter().all(|f| matches!(f, Some(Face { texture: Some(0), .. })))),
                "{} has faces with no picture",
                species.name()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

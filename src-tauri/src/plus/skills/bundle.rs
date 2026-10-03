//! Portable skill bundles: a zip holding `skills/` and `rules/` trees, the optional repo
//! manifest and `mcpm-skills-bundle.json`. The writer emits stored entries (valid zip, read by
//! any unzip); the reader also inflates the deflate entries mcpm's `zipfile` produces.

use super::clock::Clock;
use super::json::{self, J};
use super::parser::{discover_skills, SkillType};
use std::fs;
use std::path::{Path, PathBuf};

pub const BUNDLE_MANIFEST: &str = "mcpm-skills-bundle.json";
pub const BUNDLE_FORMAT: &str = "mcpm-skills-bundle";
const REPO_MANIFEST: &str = "mcpm-skills.yaml";

pub struct BundleOptions<'a> {
    pub output: Option<PathBuf>,
    pub skill_names: Option<Vec<String>>,
    pub clock: &'a dyn Clock,
}

fn repo_name(repo: &Path) -> String {
    let manifest = repo.join(REPO_MANIFEST);
    if manifest.exists() {
        let name = fs::read_to_string(&manifest)
            .ok()
            .and_then(|t| serde_yaml::from_str::<serde_yaml::Value>(&t).ok())
            .and_then(|v| v.get("name").and_then(|n| n.as_str().map(str::to_string)));
        return name.unwrap_or_else(|| "skills".into());
    }
    repo.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_files(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

pub fn create_bundle(repo: &Path, opts: &BundleOptions<'_>) -> Result<PathBuf, String> {
    let mut skills = discover_skills(repo);
    if let Some(names) = opts.skill_names.as_ref().filter(|n| !n.is_empty()) {
        skills.retain(|s| names.iter().any(|n| n == s.name()));
    }
    if skills.is_empty() {
        return Err("No skills found to bundle".into());
    }
    let output = opts
        .output
        .clone()
        .unwrap_or_else(|| repo.join(format!("{}-bundle.zip", repo_name(repo))));

    let mut zip = ZipWriter::default();
    let mut skill_entries = Vec::new();
    let mut rule_entries = Vec::new();
    for skill in &skills {
        let dir = skill.source_dir();
        let base = if skill.skill_type == SkillType::Rule {
            "rules"
        } else {
            "skills"
        };
        let mut files = Vec::new();
        walk_files(dir, &mut files);
        files.sort();
        for file in files {
            let rel = file.strip_prefix(dir).map_err(|e| e.to_string())?;
            let rel: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            let data = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            zip.add(&format!("{base}/{}/{}", skill.name(), rel.join("/")), &data);
        }
        let mut entry = vec![
            ("name".to_string(), J::str(skill.name())),
            ("description".into(), J::str(&skill.frontmatter.description)),
            (
                "activation".into(),
                J::str(skill.frontmatter.activation.as_str()),
            ),
            (
                "type".into(),
                J::str(if skill.skill_type == SkillType::Rule {
                    "rule"
                } else {
                    "skill"
                }),
            ),
        ];
        if dir.join("servers.json").exists() {
            entry.push(("has_server_deps".into(), J::Bool(true)));
        }
        if skill.skill_type == SkillType::Rule {
            rule_entries.push(J::Obj(entry));
        } else {
            skill_entries.push(J::Obj(entry));
        }
    }
    let manifest = repo.join(REPO_MANIFEST);
    if manifest.exists() {
        let data = fs::read(&manifest).map_err(|e| e.to_string())?;
        zip.add(REPO_MANIFEST, &data);
    }
    let doc = J::Obj(vec![
        ("format".into(), J::str(BUNDLE_FORMAT)),
        ("version".into(), J::int(1)),
        ("created_at".into(), J::str(opts.clock.now().isoformat())),
        ("skills".into(), J::Arr(skill_entries)),
        ("rules".into(), J::Arr(rule_entries)),
    ]);
    zip.add(BUNDLE_MANIFEST, doc.dumps().as_bytes());
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(&output, zip.finish()).map_err(|e| format!("{}: {e}", output.display()))?;
    Ok(output)
}

/// Extracts every file except the bundle manifest and returns the skill and rule names it lists.
pub fn extract_bundle(bundle: &Path, target: &Path) -> Result<Vec<String>, String> {
    let bytes = fs::read(bundle).map_err(|e| format!("{}: {e}", bundle.display()))?;
    let entries = read_zip(&bytes)?;
    let manifest = entries
        .iter()
        .find(|e| e.name == BUNDLE_MANIFEST)
        .ok_or_else(|| format!("Invalid bundle: missing {BUNDLE_MANIFEST}"))?;
    let doc = json::parse(&String::from_utf8_lossy(&manifest.data))
        .map_err(|e| format!("Invalid bundle: {e}"))?;
    if doc.get("format").and_then(J::as_str) != Some(BUNDLE_FORMAT) {
        return Err("Not a valid mcpm skills bundle".into());
    }
    for entry in &entries {
        if entry.name == BUNDLE_MANIFEST || entry.name.ends_with('/') {
            continue;
        }
        if entry.name.starts_with('/') || entry.name.contains("..") {
            continue;
        }
        let dest = target.join(&entry.name);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&dest, &entry.data).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    let mut names = Vec::new();
    for key in ["skills", "rules"] {
        if let Some(J::Arr(items)) = doc.get(key) {
            for item in items {
                let name = item
                    .get("name")
                    .and_then(J::as_str)
                    .ok_or("Invalid bundle: entry without a name")?;
                names.push(name.to_string());
            }
        }
    }
    Ok(names)
}

fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        t
    });
    let mut crc = 0xFFFF_FFFFu32;
    for b in data {
        crc = table[((crc ^ u32::from(*b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

/// 1980-01-01 00:00:00 in DOS format, so bundles are reproducible.
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = 0x0021;

#[derive(Default)]
struct ZipWriter {
    body: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

impl ZipWriter {
    fn add(&mut self, name: &str, data: &[u8]) {
        let offset = self.body.len() as u32;
        let crc = crc32(data);
        let flags: u16 = if name.is_ascii() { 0 } else { 0x0800 };
        let size = data.len() as u32;
        let u16le = |v: &mut Vec<u8>, n: u16| v.extend_from_slice(&n.to_le_bytes());
        let u32le = |v: &mut Vec<u8>, n: u32| v.extend_from_slice(&n.to_le_bytes());

        u32le(&mut self.body, 0x0403_4b50);
        u16le(&mut self.body, 20);
        u16le(&mut self.body, flags);
        u16le(&mut self.body, 0);
        u16le(&mut self.body, DOS_TIME);
        u16le(&mut self.body, DOS_DATE);
        u32le(&mut self.body, crc);
        u32le(&mut self.body, size);
        u32le(&mut self.body, size);
        u16le(&mut self.body, name.len() as u16);
        u16le(&mut self.body, 0);
        self.body.extend_from_slice(name.as_bytes());
        self.body.extend_from_slice(data);

        let c = &mut self.central;
        u32le(c, 0x0201_4b50);
        u16le(c, 20);
        u16le(c, 20);
        u16le(c, flags);
        u16le(c, 0);
        u16le(c, DOS_TIME);
        u16le(c, DOS_DATE);
        u32le(c, crc);
        u32le(c, size);
        u32le(c, size);
        u16le(c, name.len() as u16);
        u16le(c, 0);
        u16le(c, 0);
        u16le(c, 0);
        u16le(c, 0);
        u32le(c, 0o100_644 << 16);
        u32le(c, offset);
        c.extend_from_slice(name.as_bytes());
        self.count += 1;
    }

    fn finish(mut self) -> Vec<u8> {
        let cd_offset = self.body.len() as u32;
        let cd_size = self.central.len() as u32;
        self.body.extend_from_slice(&self.central);
        self.body.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        self.body.extend_from_slice(&[0, 0, 0, 0]);
        self.body.extend_from_slice(&self.count.to_le_bytes());
        self.body.extend_from_slice(&self.count.to_le_bytes());
        self.body.extend_from_slice(&cd_size.to_le_bytes());
        self.body.extend_from_slice(&cd_offset.to_le_bytes());
        self.body.extend_from_slice(&[0, 0]);
        self.body
    }
}

struct ZipEntry {
    name: String,
    data: Vec<u8>,
}

fn le16(b: &[u8], at: usize) -> Result<usize, String> {
    b.get(at..at + 2)
        .map(|s| usize::from(u16::from_le_bytes([s[0], s[1]])))
        .ok_or_else(|| "truncated zip".to_string())
}

fn le32(b: &[u8], at: usize) -> Result<usize, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
        .ok_or_else(|| "truncated zip".to_string())
}

fn read_zip(b: &[u8]) -> Result<Vec<ZipEntry>, String> {
    let eocd = (0..b.len().saturating_sub(21))
        .rev()
        .find(|&i| b[i..i + 4] == 0x0605_4b50u32.to_le_bytes())
        .ok_or("Invalid bundle: not a zip file")?;
    let count = le16(b, eocd + 10)?;
    let mut at = le32(b, eocd + 16)?;
    let mut out = Vec::new();
    for _ in 0..count {
        if le32(b, at)? != 0x0201_4b50 {
            return Err("Invalid bundle: bad central directory".into());
        }
        let method = le16(b, at + 10)?;
        let csize = le32(b, at + 20)?;
        let usize_ = le32(b, at + 24)?;
        let name_len = le16(b, at + 28)?;
        let extra_len = le16(b, at + 30)?;
        let comment_len = le16(b, at + 32)?;
        let local = le32(b, at + 42)?;
        let name_bytes = b.get(at + 46..at + 46 + name_len).ok_or("truncated zip")?;
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        at += 46 + name_len + extra_len + comment_len;

        if le32(b, local)? != 0x0403_4b50 {
            return Err("Invalid bundle: bad local header".into());
        }
        let data_at = local + 30 + le16(b, local + 26)? + le16(b, local + 28)?;
        let raw = b.get(data_at..data_at + csize).ok_or("truncated zip")?;
        let data = match method {
            0 => raw.to_vec(),
            8 => inflate(raw, usize_)?,
            m => {
                return Err(format!(
                    "Invalid bundle: unsupported compression method {m}"
                ))
            }
        };
        out.push(ZipEntry { name, data });
    }
    Ok(out)
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
    count: u32,
}

impl Bits<'_> {
    fn need(&mut self, n: u32) -> Result<(), String> {
        while self.count < n {
            let byte = *self.data.get(self.pos).ok_or("inflate: unexpected end")?;
            self.pos += 1;
            self.bit |= u32::from(byte) << self.count;
            self.count += 8;
        }
        Ok(())
    }

    fn take(&mut self, n: u32) -> Result<u32, String> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let v = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.count -= n;
        Ok(v)
    }
}

struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Huffman {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[usize::from(l)] += 1;
        }
        counts[0] = 0;
        let mut offs = [0u16; 16];
        for i in 1..16 {
            offs[i] = offs[i - 1] + counts[i - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[usize::from(offs[usize::from(l)])] = sym as u16;
                offs[usize::from(l)] += 1;
            }
        }
        Huffman { counts, symbols }
    }

    fn decode(&self, bits: &mut Bits<'_>) -> Result<usize, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.take(1)? as i32;
            let count = i32::from(self.counts[len]);
            if code - count < first {
                return Ok(usize::from(self.symbols[(index + (code - first)) as usize]));
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("inflate: bad code".into())
    }
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn inflate_codes(
    bits: &mut Bits<'_>,
    out: &mut Vec<u8>,
    lit: &Huffman,
    dist: &Huffman,
) -> Result<(), String> {
    loop {
        let sym = lit.decode(bits)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            257..=285 => {
                let i = sym - 257;
                let len = usize::from(LEN_BASE[i]) + bits.take(u32::from(LEN_EXTRA[i]))? as usize;
                let d = dist.decode(bits)?;
                if d >= 30 {
                    return Err("inflate: bad distance".into());
                }
                let distance =
                    usize::from(DIST_BASE[d]) + bits.take(u32::from(DIST_EXTRA[d]))? as usize;
                if distance > out.len() {
                    return Err("inflate: distance too far back".into());
                }
                for _ in 0..len {
                    out.push(out[out.len() - distance]);
                }
            }
            _ => return Err("inflate: bad symbol".into()),
        }
    }
}

fn inflate(data: &[u8], size_hint: usize) -> Result<Vec<u8>, String> {
    let mut bits = Bits {
        data,
        pos: 0,
        bit: 0,
        count: 0,
    };
    let mut out = Vec::with_capacity(size_hint);
    loop {
        let last = bits.take(1)?;
        match bits.take(2)? {
            0 => {
                bits.bit = 0;
                bits.count = 0;
                let len = le16(data, bits.pos)?;
                let start = bits.pos + 4;
                out.extend_from_slice(data.get(start..start + len).ok_or("inflate: truncated")?);
                bits.pos = start + len;
            }
            1 => {
                let mut lengths = [8u8; 288];
                lengths[144..256].fill(9);
                lengths[256..280].fill(7);
                inflate_codes(
                    &mut bits,
                    &mut out,
                    &Huffman::new(&lengths),
                    &Huffman::new(&[5u8; 30]),
                )?;
            }
            2 => {
                let nlen = bits.take(5)? as usize + 257;
                let ndist = bits.take(5)? as usize + 1;
                let ncode = bits.take(4)? as usize + 4;
                const ORDER: [usize; 19] = [
                    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                ];
                let mut cl = [0u8; 19];
                for &o in ORDER.iter().take(ncode) {
                    cl[o] = bits.take(3)? as u8;
                }
                let code_huff = Huffman::new(&cl);
                let mut lengths = vec![0u8; nlen + ndist];
                let mut i = 0;
                while i < nlen + ndist {
                    let sym = code_huff.decode(&mut bits)?;
                    if sym < 16 {
                        lengths[i] = sym as u8;
                        i += 1;
                        continue;
                    }
                    let (prev, rep) = match sym {
                        16 => {
                            let p = *lengths
                                .get(i.wrapping_sub(1))
                                .ok_or("inflate: bad repeat")?;
                            (p, 3 + bits.take(2)? as usize)
                        }
                        17 => (0, 3 + bits.take(3)? as usize),
                        _ => (0, 11 + bits.take(7)? as usize),
                    };
                    if i + rep > nlen + ndist {
                        return Err("inflate: repeat overflow".into());
                    }
                    lengths[i..i + rep].fill(prev);
                    i += rep;
                }
                inflate_codes(
                    &mut bits,
                    &mut out,
                    &Huffman::new(&lengths[..nlen]),
                    &Huffman::new(&lengths[nlen..]),
                )?;
            }
            _ => return Err("inflate: bad block type".into()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_reference_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn writer_output_reads_back() {
        let mut zip = ZipWriter::default();
        zip.add("skills/a/SKILL.md", b"hello");
        zip.add("é.txt", b"");
        let entries = read_zip(&zip.finish()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "skills/a/SKILL.md");
        assert_eq!(entries[0].data, b"hello");
        assert_eq!(entries[1].name, "é.txt");
    }

    #[test]
    fn inflate_handles_fixed_huffman_blocks() {
        // zlib.compressobj(wbits=-15).compress(b"aaaaaaaaaabbbbbbbbbb") from CPython
        let raw = [0x4b, 0x4c, 0x84, 0x81, 0x24, 0x38, 0x00, 0x00];
        assert_eq!(inflate(&raw, 20).unwrap(), b"aaaaaaaaaabbbbbbbbbb");
    }

    #[test]
    fn inflate_handles_dynamic_huffman_blocks() {
        // zlib level 9 raw deflate of DYNAMIC_TEXT; the first block header is type 2
        let hex = "65524112c32008fc4abe86a3d364aa8d334d2ebebe960581f662805d9705934bbd687b52efb41de70c1fd41a6da3aceab57fe34a2d652d2543a9f65d39a5bf8f7abea093591867bb85c1081f72c394d117fc50627a810aba09cc2ec0f15ed445ccd00fbc29247ac5a4fdb06e07dc0452a882019f2a2e86dc348aaca1adfb5835e48640743899380358024dcb3034835f49f74a13950b61b5d25a5dca37ad89c3a323e1baedc4f6144d2ae616e74d09d9efdcdcffff401f";
        let raw: Vec<u8> = (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        assert_eq!(raw[0] & 7, 5);
        let text = String::from_utf8(inflate(&raw, 0).unwrap()).unwrap();
        assert_eq!(text, DYNAMIC_TEXT);
    }

    const DYNAMIC_TEXT: &str = "delta kappa iota gamma zeta kappa theta lambda kappa beta kappa alpha theta epsilon iota delta delta mu theta iota iota theta eta lambda gamma delta lambda gamma iota eta mu alpha lambda beta gamma kappa alpha epsilon alpha epsilon theta kappa mu eta mu eta eta mu kappa theta gamma zeta beta alpha gamma theta delta epsilon lambda eta lambda epsilon eta iota eta kappa zeta iota kappa eta kappa delta zeta lambda alpha epsilon kappa lambda mu gamma mu zeta iota kappa kappa beta mu lambda delta lambda kappa epsilon epsilon beta beta theta lambda theta beta zeta beta eta gamma alpha epsilon eta eta beta alpha kappa kappa alpha eta mu kappa zeta iota epsilon iota delta";

    #[test]
    fn inflate_handles_stored_blocks() {
        let raw = [0x01, 0x03, 0x00, 0xfc, 0xff, b'a', b'b', b'c'];
        assert_eq!(inflate(&raw, 3).unwrap(), b"abc");
    }
}

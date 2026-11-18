//! The setup file is a zip of stored entries: written entry by entry to a file (the sessions part
//! can be gigabytes, so nothing here holds the whole archive) and read through its central
//! directory. The layout is the one of `skills bundle` (`skills::bundle`), whose checksum and
//! inflate this reuses; a file that was zipped again by another tool (deflate) still opens.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::skills::bundle::{crc32, inflate};

/// 1980-01-01 00:00:00, so the same content gives the same bytes.
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = 0x0021;
const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
const COPY_BUF: usize = 256 * 1024;
/// What one entry may be when it is read into memory.
pub const MAX_IN_MEMORY: u64 = 256 * 1024 * 1024;
pub const TOO_LARGE: &str = "the setup file would be larger than 4 GB: leave some sessions out";

fn crc_of(file: &mut File) -> Result<(u32, u64), String> {
    // The checksum of the skills bundle works on a slice, so a large file is folded in pieces
    // through the standard CRC-32 update.
    let mut crc = Crc::new();
    let mut buf = vec![0u8; COPY_BUF];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok((crc.finish(), size));
        }
        crc.update(&buf[..n]);
        size += n as u64;
    }
}

/// CRC-32 (IEEE) that can be fed in pieces; checked against `crc32` in the tests.
struct Crc(u32);

impl Crc {
    fn new() -> Self {
        Crc(0xFFFF_FFFF)
    }

    fn update(&mut self, data: &[u8]) {
        for byte in data {
            let mut c = (self.0 ^ u32::from(*byte)) & 0xFF;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            self.0 = c ^ (self.0 >> 8);
        }
    }

    fn finish(&self) -> u32 {
        !self.0
    }
}

pub struct StreamZip {
    file: File,
    offset: u64,
    central: Vec<u8>,
    count: u32,
}

fn put16(v: &mut Vec<u8>, n: u16) {
    v.extend_from_slice(&n.to_le_bytes());
}

fn put32(v: &mut Vec<u8>, n: u32) {
    v.extend_from_slice(&n.to_le_bytes());
}

impl StreamZip {
    /// Creates `path` readable by its owner only, and fails when it exists.
    pub fn create(path: &Path) -> Result<Self, String> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let file = options
            .open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self {
            file,
            offset: 0,
            central: Vec::new(),
            count: 0,
        })
    }

    fn header(&mut self, name: &str, crc: u32, size: u32) -> Result<u32, String> {
        if self.count >= u32::from(u16::MAX) {
            return Err("too many files for one setup file".into());
        }
        if self.offset >= u64::from(u32::MAX) - 1024 {
            return Err(TOO_LARGE.into());
        }
        let offset = self.offset as u32;
        let flags: u16 = if name.is_ascii() { 0 } else { 0x0800 };
        let mut local = Vec::with_capacity(30 + name.len());
        put32(&mut local, LOCAL);
        put16(&mut local, 20);
        put16(&mut local, flags);
        put16(&mut local, 0);
        put16(&mut local, DOS_TIME);
        put16(&mut local, DOS_DATE);
        put32(&mut local, crc);
        put32(&mut local, size);
        put32(&mut local, size);
        put16(&mut local, name.len() as u16);
        put16(&mut local, 0);
        local.extend_from_slice(name.as_bytes());
        self.file.write_all(&local).map_err(|e| e.to_string())?;
        self.offset += local.len() as u64;

        let c = &mut self.central;
        put32(c, CENTRAL);
        put16(c, 20);
        put16(c, 20);
        put16(c, flags);
        put16(c, 0);
        put16(c, DOS_TIME);
        put16(c, DOS_DATE);
        put32(c, crc);
        put32(c, size);
        put32(c, size);
        put16(c, name.len() as u16);
        for _ in 0..4 {
            put16(c, 0);
        }
        put32(c, 0o100_600 << 16);
        put32(c, offset);
        c.extend_from_slice(name.as_bytes());
        self.count += 1;
        Ok(offset)
    }

    pub fn add(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        if data.len() as u64 >= u64::from(u32::MAX) - self.offset {
            return Err(TOO_LARGE.into());
        }
        self.header(name, crc32(data), data.len() as u32)?;
        self.file.write_all(data).map_err(|e| e.to_string())?;
        self.offset += data.len() as u64;
        Ok(())
    }

    /// Copies a file in; it is read twice (checksum, then content) and must not change between.
    pub fn add_file(&mut self, name: &str, path: &Path) -> Result<u64, String> {
        let mut source = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (crc, size) = crc_of(&mut source)?;
        if size >= u64::from(u32::MAX) - self.offset {
            return Err(TOO_LARGE.into());
        }
        source.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        self.header(name, crc, size as u32)?;
        let copied =
            std::io::copy(&mut source.take(size), &mut self.file).map_err(|e| e.to_string())?;
        if copied != size {
            return Err(format!("{} changed while it was copied", path.display()));
        }
        self.offset += size;
        Ok(size)
    }

    pub fn finish(mut self) -> Result<(), String> {
        let start = self.offset as u32;
        let size = self.central.len() as u32;
        let mut end = Vec::new();
        end.extend_from_slice(&self.central);
        put32(&mut end, END);
        put16(&mut end, 0);
        put16(&mut end, 0);
        put16(&mut end, self.count as u16);
        put16(&mut end, self.count as u16);
        put32(&mut end, size);
        put32(&mut end, start);
        put16(&mut end, 0);
        self.file.write_all(&end).map_err(|e| e.to_string())?;
        self.file.sync_all().map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    method: u16,
    crc: u32,
    csize: u64,
    pub size: u64,
    local: u64,
}

pub struct ZipFile {
    file: File,
    pub entries: Vec<Entry>,
}

fn le16(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| "truncated zip".to_string())
}

fn le32(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| "truncated zip".to_string())
}

impl ZipFile {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let length = file.metadata().map_err(|e| e.to_string())?.len();
        let tail = length.min(22 + 65_535);
        file.seek(SeekFrom::Start(length - tail))
            .map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; tail as usize];
        file.read_exact(&mut buf)
            .map_err(|_| "not a zip file".to_string())?;
        let at = (0..buf.len().saturating_sub(21))
            .rev()
            .find(|&i| buf[i..i + 4] == END.to_le_bytes())
            .ok_or("not a zip file")?;
        let count = usize::from(le16(&buf, at + 10)?);
        let size = u64::from(le32(&buf, at + 12)?);
        let start = u64::from(le32(&buf, at + 16)?);
        if start + size > length {
            return Err("the zip directory is damaged".into());
        }
        file.seek(SeekFrom::Start(start))
            .map_err(|e| e.to_string())?;
        let mut dir = vec![0u8; size as usize];
        file.read_exact(&mut dir).map_err(|e| e.to_string())?;
        let mut entries = Vec::with_capacity(count);
        let mut cursor = 0usize;
        for _ in 0..count {
            if le32(&dir, cursor)? != CENTRAL {
                return Err("the zip directory is damaged".into());
            }
            let name_len = usize::from(le16(&dir, cursor + 28)?);
            let extra = usize::from(le16(&dir, cursor + 30)?);
            let comment = usize::from(le16(&dir, cursor + 32)?);
            let name = dir
                .get(cursor + 46..cursor + 46 + name_len)
                .ok_or("truncated zip")?;
            entries.push(Entry {
                name: String::from_utf8_lossy(name).into_owned(),
                method: le16(&dir, cursor + 10)?,
                crc: le32(&dir, cursor + 16)?,
                csize: u64::from(le32(&dir, cursor + 20)?),
                size: u64::from(le32(&dir, cursor + 24)?),
                local: u64::from(le32(&dir, cursor + 42)?),
            });
            cursor += 46 + name_len + extra + comment;
        }
        Ok(Self { file, entries })
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    fn data_start(&mut self, entry: &Entry) -> Result<u64, String> {
        let mut head = [0u8; 30];
        self.file
            .seek(SeekFrom::Start(entry.local))
            .map_err(|e| e.to_string())?;
        self.file
            .read_exact(&mut head)
            .map_err(|_| "truncated zip".to_string())?;
        if le32(&head, 0)? != LOCAL {
            return Err("the zip entry is damaged".into());
        }
        Ok(entry.local + 30 + u64::from(le16(&head, 26)?) + u64::from(le16(&head, 28)?))
    }

    /// The entry's content in memory; refuses one larger than [`MAX_IN_MEMORY`] and checks the checksum.
    pub fn read(&mut self, name: &str) -> Result<Vec<u8>, String> {
        let entry = self
            .entry(name)
            .cloned()
            .ok_or_else(|| format!("{name} is not in the file"))?;
        if entry.size > MAX_IN_MEMORY || entry.csize > MAX_IN_MEMORY {
            return Err(format!("{name} is too large to read at once"));
        }
        let start = self.data_start(&entry)?;
        self.file
            .seek(SeekFrom::Start(start))
            .map_err(|e| e.to_string())?;
        let mut raw = vec![0u8; entry.csize as usize];
        self.file
            .read_exact(&mut raw)
            .map_err(|_| "truncated zip".to_string())?;
        let data = match entry.method {
            0 => raw,
            8 => inflate(&raw, entry.size as usize)?,
            m => return Err(format!("{name}: unsupported compression method {m}")),
        };
        if crc32(&data) != entry.crc {
            return Err(format!("{name} does not match its checksum"));
        }
        Ok(data)
    }

    /// Streams a stored entry into `out`; a deflated one goes through memory.
    pub fn copy(&mut self, name: &str, out: &mut dyn Write) -> Result<(), String> {
        let entry = self
            .entry(name)
            .cloned()
            .ok_or_else(|| format!("{name} is not in the file"))?;
        if entry.method != 0 {
            return out.write_all(&self.read(name)?).map_err(|e| e.to_string());
        }
        let start = self.data_start(&entry)?;
        self.file
            .seek(SeekFrom::Start(start))
            .map_err(|e| e.to_string())?;
        std::io::copy(&mut (&mut self.file).take(entry.csize), out)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// True for an entry name that stays inside the folder it is extracted to.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains('\\')
        && !name
            .split('/')
            .any(|part| part == ".." || part.is_empty() && !name.ends_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_piecewise_checksum_agrees_with_the_one_of_skills_bundle() {
        let data: Vec<u8> = (0..100_000u32).map(|i| (i * 7 % 256) as u8).collect();
        let mut crc = Crc::new();
        for piece in data.chunks(777) {
            crc.update(piece);
        }
        assert_eq!(crc.finish(), crc32(&data));
    }

    #[test]
    fn entries_written_in_memory_and_from_a_file_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.bin");
        let content: Vec<u8> = (0..700_000u32).map(|i| (i % 253) as u8).collect();
        std::fs::write(&big, &content).unwrap();
        let path = dir.path().join("out.zip");
        let mut zip = StreamZip::create(&path).unwrap();
        zip.add("manifest.json", b"{}").unwrap();
        zip.add("memory/ünï.md", "tëxt".as_bytes()).unwrap();
        assert_eq!(zip.add_file("history.enc", &big).unwrap(), 700_000);
        zip.finish().unwrap();

        let mut read = ZipFile::open(&path).unwrap();
        assert_eq!(
            read.names().collect::<Vec<_>>(),
            ["manifest.json", "memory/ünï.md", "history.enc"]
        );
        assert_eq!(read.read("manifest.json").unwrap(), b"{}");
        assert_eq!(read.read("memory/ünï.md").unwrap(), "tëxt".as_bytes());
        let mut copied = Vec::new();
        read.copy("history.enc", &mut copied).unwrap();
        assert_eq!(copied, content);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_owner_only_and_an_existing_one_is_never_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.zip");
        StreamZip::create(&path).unwrap().finish().unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(StreamZip::create(&path).is_err());
    }

    #[test]
    fn a_zip_made_by_the_skills_bundle_reader_and_this_one_agree() {
        // `unzip -t` accepts what StreamZip writes: a stored entry, a valid directory.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.zip");
        let mut zip = StreamZip::create(&path).unwrap();
        zip.add("a.txt", b"hello").unwrap();
        zip.finish().unwrap();
        if let Ok(out) = std::process::Command::new("unzip")
            .arg("-t")
            .arg(&path)
            .output()
        {
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
    }

    #[test]
    fn a_damaged_checksum_and_a_non_zip_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.zip");
        let mut zip = StreamZip::create(&path).unwrap();
        zip.add("a.txt", b"hello").unwrap();
        zip.finish().unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        let at = bytes.windows(5).position(|w| w == b"hello").unwrap();
        bytes[at] = b'j';
        std::fs::write(&path, &bytes).unwrap();
        assert!(ZipFile::open(&path)
            .unwrap()
            .read("a.txt")
            .unwrap_err()
            .contains("checksum"));
        std::fs::write(&path, b"not a zip at all, no directory").unwrap();
        assert!(ZipFile::open(&path).is_err());
    }

    #[test]
    fn names_that_leave_the_folder_are_unsafe() {
        for bad in ["../x", "/etc/passwd", "a/../b", "a\\b", ""] {
            assert!(!safe_name(bad), "{bad}");
        }
        assert!(safe_name("claude-user/rules/a.md"));
    }
}

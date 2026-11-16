#![allow(dead_code)]
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const SEED_ENV: &str = "PLUS_PROP_SEED";
const CASES_ENV: &str = "PLUS_PROP_CASES";

pub(crate) struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Self(if z == 0 { 0x2545_F491_4F6C_DD1D } else { z })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    pub fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi.saturating_sub(lo) + 1)
    }

    pub fn chance(&mut self, percent: u64) -> bool {
        self.next_u64() % 100 < percent
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    pub fn string(&mut self, alphabet: &str, max_len: usize) -> String {
        let chars: Vec<char> = alphabet.chars().collect();
        (0..self.range(0, max_len))
            .map(|_| chars[self.below(chars.len())])
            .collect()
    }

    /// A name that satisfies the skill, agent and style name rules (1-64 chars, single hyphens).
    pub fn slug(&mut self, max_len: usize) -> String {
        let raw = self.string("abcdefghijklmnopqrstuvwxyz0123456789-", max_len);
        let mut out = String::new();
        for c in raw.chars() {
            if c == '-' && (out.is_empty() || out.ends_with('-')) {
                continue;
            }
            out.push(c);
        }
        let mut out = out.trim_end_matches('-').to_string();
        if out.is_empty() {
            out.push('a');
        }
        out.truncate(64);
        while out.ends_with('-') {
            out.pop();
        }
        out
    }

    pub fn tokens(&mut self, vocab: &[&str], max_tokens: usize) -> String {
        (0..self.range(0, max_tokens))
            .map(|_| *self.pick(vocab))
            .collect()
    }

    pub fn bytes(&mut self, max_len: usize) -> Vec<u8> {
        (0..self.range(0, max_len))
            .map(|_| self.next_u64() as u8)
            .collect()
    }

    /// Mixes ASCII, structural characters, controls, multi-byte text and arbitrary scalar values.
    pub fn garbage(&mut self, max_len: usize) -> String {
        const STRUCTURAL: &str = "-:#[]{}\"',|>!&*%@\\`~=<>/.$ ";
        const CONTROL: &[char] = &[
            '\n', '\n', '\r', '\t', '\0', '\u{1b}', '\u{7f}', '\u{b}', '\u{c}', '\u{85}',
            '\u{2028}', '\u{feff}', '\u{202e}', '\u{200b}',
        ];
        const WIDE: &[char] = &[
            'é',
            'ß',
            'ñ',
            'Ω',
            '日',
            '本',
            '語',
            'ﬁ',
            '\u{301}',
            '🙂',
            '𝔘',
            '\u{10ffff}',
        ];
        let mut out = String::new();
        for _ in 0..self.range(0, max_len) {
            let c = match self.below(100) {
                0..=39 => (b' ' + self.below(95) as u8) as char,
                40..=59 => *self.pick(&STRUCTURAL.chars().collect::<Vec<_>>()),
                60..=71 => *self.pick(CONTROL),
                72..=89 => *self.pick(WIDE),
                _ => char::from_u32(self.next_u64() as u32 % 0x11_0000).unwrap_or('\u{fffd}'),
            };
            out.push(c);
        }
        out
    }
}

fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn env_u64(name: &str) -> Option<u64> {
    let raw = std::env::var(name).ok()?;
    let raw = raw.trim();
    match raw.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => raw.parse().ok(),
    }
}

/// Runs `case` over a fixed seed sequence derived from `name`. A failing seed is printed and can
/// be replayed alone with `PLUS_PROP_SEED=<seed>`; `PLUS_PROP_CASES` overrides the case count.
pub(crate) fn run_cases(name: &str, cases: u64, mut case: impl FnMut(u64, &mut Rng)) {
    let seeds: Vec<u64> = match env_u64(SEED_ENV) {
        Some(seed) => vec![seed],
        None => {
            let base = fnv1a(name);
            (0..env_u64(CASES_ENV).unwrap_or(cases))
                .map(|i| base.wrapping_add(i))
                .collect()
        }
    };
    for seed in seeds {
        let mut rng = Rng::new(seed);
        if let Err(panic) = catch_unwind(AssertUnwindSafe(|| case(seed, &mut rng))) {
            eprintln!("randomized test `{name}` failed at seed {seed} (replay: {SEED_ENV}={seed})");
            resume_unwind(panic);
        }
    }
}

static SCRATCH_SEQ: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct ScratchDir(PathBuf);

impl ScratchDir {
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "plus-prop-{tag}-{}-{}",
            std::process::id(),
            SCRATCH_SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn reset(&self) {
        let _ = std::fs::remove_dir_all(&self.0);
        std::fs::create_dir_all(&self.0).unwrap();
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Every file and directory under `root`, directories as `None` under a trailing-slash key.
pub(crate) fn tree_snapshot(root: &Path) -> std::collections::BTreeMap<String, Option<Vec<u8>>> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if path.is_dir() {
                out.insert(format!("{rel}/"), None);
                stack.push(path);
            } else {
                out.insert(rel, Some(std::fs::read(&path).unwrap_or_default()));
            }
        }
    }
    out
}

//! Version numbers as registries print them: `1.4.0`, `v2.1`, `1.0.0-rc.2`, `2.0rc1`. A version
//! with a pre-release part sorts below the release it leads up to.

use std::cmp::Ordering;

struct Parsed {
    numbers: Vec<u64>,
    pre: String,
}

fn split(version: &str) -> Option<(Vec<u64>, &str)> {
    let version = version.strip_prefix(['v', 'V']).unwrap_or(version);
    let end = version
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(version.len());
    let head = version[..end].trim_end_matches('.');
    let rest = &version[head.len()..];
    let numbers: Option<Vec<u64>> = head
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect();
    let numbers = numbers.filter(|numbers| !numbers.is_empty())?;
    Some((numbers, rest))
}

fn parse(version: &str) -> Option<Parsed> {
    let (numbers, rest) = split(version)?;
    let pre = if rest.is_empty() || rest.starts_with('+') {
        String::new()
    } else {
        rest.trim_start_matches(['-', '.']).to_ascii_lowercase()
    };
    Some(Parsed { numbers, pre })
}

/// The dotted numbers a version starts with.
pub fn numbers(version: &str) -> Option<Vec<u64>> {
    split(version).map(|(numbers, _)| numbers)
}

/// Whether `version` names one release (`1.4.0`, `2.0rc1`) and not a range, tag or wildcard
/// (`^1.4`, `latest`, `1.x`).
pub fn exact(version: &str) -> bool {
    let Some((_, rest)) = split(version) else {
        return false;
    };
    version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
        && (rest.is_empty()
            || rest.starts_with(['-', '+'])
            || rest.starts_with(|c: char| c.is_ascii_alphabetic())
            || rest.starts_with(".post")
            || rest.starts_with(".dev"))
        && !rest.starts_with(['x', 'X'])
}

fn tokens(text: &str) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = Vec::new();
    for c in text.chars().filter(|c| c.is_ascii_alphanumeric()) {
        let digit = c.is_ascii_digit();
        match out.last_mut() {
            Some((last, token)) if *last == digit => token.push(c),
            _ => out.push((digit, c.to_string())),
        }
    }
    out
}

fn compare_pre(a: &str, b: &str) -> Ordering {
    let (a, b) = (tokens(a), tokens(b));
    for (left, right) in a.iter().zip(&b) {
        let order = match (left, right) {
            ((true, x), (true, y)) => x
                .parse::<u64>()
                .unwrap_or(0)
                .cmp(&y.parse::<u64>().unwrap_or(0)),
            ((l, x), (r, y)) if l == r => x.cmp(y),
            ((true, _), _) => Ordering::Less,
            _ => Ordering::Greater,
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

fn compare_numbers(a: &[u64], b: &[u64]) -> Ordering {
    let len = a.len().max(b.len());
    (0..len)
        .map(|i| {
            a.get(i)
                .copied()
                .unwrap_or(0)
                .cmp(&b.get(i).copied().unwrap_or(0))
        })
        .find(|order| *order != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

/// Whether `latest` is a later release than `pinned`; false when either is not a version.
pub fn newer(latest: &str, pinned: &str) -> bool {
    let (Some(latest), Some(pinned)) = (parse(latest), parse(pinned)) else {
        return false;
    };
    let order = compare_numbers(&latest.numbers, &pinned.numbers).then_with(|| {
        match (latest.pre.is_empty(), pinned.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => compare_pre(&latest.pre, &pinned.pre),
        }
    });
    order == Ordering::Greater
}

/// The shape of a container tag: whether it starts with `v`, how many dotted numbers it has and
/// what follows them. `None` for a tag that is no version (`latest`, `stable`) or whose tail does
/// not start with `-`.
pub fn tag_flavor(tag: &str) -> Option<String> {
    let (numbers, rest) = split(tag)?;
    if !(rest.is_empty() || rest.starts_with('-')) {
        return None;
    }
    let prefix = if tag.starts_with(['v', 'V']) { "v" } else { "" };
    Some(format!("{prefix}|{}|{rest}", numbers.len()))
}

/// The highest tag shaped like `flavor`.
pub fn newest_tag(tags: &[String], flavor: &str) -> Option<String> {
    tags.iter()
        .filter(|tag| tag_flavor(tag).as_deref() == Some(flavor))
        .filter_map(|tag| numbers(tag).map(|numbers| (numbers, tag)))
        .max_by(|(a, _), (b, _)| compare_numbers(a, b))
        .map(|(_, tag)| tag.clone())
}

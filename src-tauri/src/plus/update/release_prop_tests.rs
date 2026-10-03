//! Seeded randomized and edge-case tests for the update path: version comparison, pin specs,
//! release selection, checksum files, URL policy and the stored source metadata.

use super::net::{host_of, url_allowed, HttpClient, HttpError};
use super::pins::{
    compare_versions, is_floating, latest_npm, latest_pypi, parse_spec, rewrite, PinSpec,
};
use super::release::{check, expected_hash, resolve_pattern, valid_repo, Checked, ReleaseCheck};
use super::source::{expand_path, from_meta, Source};
use crate::plus::randutil::{run_cases, Rng};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

struct Canned {
    reply: Result<String, HttpError>,
    seen: RefCell<Vec<(String, Vec<(String, String)>)>>,
}

impl Canned {
    fn ok(body: impl Into<String>) -> Self {
        Self {
            reply: Ok(body.into()),
            seen: RefCell::default(),
        }
    }

    fn status(code: Option<u16>) -> Self {
        Self {
            reply: Err(HttpError::new(code, "mock")),
            seen: RefCell::default(),
        }
    }
}

impl HttpClient for Canned {
    fn get_text(&self, url: &str, headers: &[(String, String)]) -> Result<String, HttpError> {
        self.seen.borrow_mut().push((url.into(), headers.to_vec()));
        self.reply.clone()
    }

    fn download(&self, _url: &str, _dest: &Path) -> Result<String, HttpError> {
        Err(HttpError::new(None, "no downloads in these tests"))
    }
}

fn version_model(text: &str) -> Option<(Vec<u64>, bool)> {
    static SHAPE: OnceLock<Regex> = OnceLock::new();
    let shape =
        SHAPE.get_or_init(|| Regex::new(r"(?s)^([0-9]+(?:\.[0-9]+)*)(?:([-+]).*)?$").unwrap());
    let v = text.trim().trim_start_matches('v');
    let caps = shape.captures(v)?;
    let parts: Option<Vec<u64>> = caps[1].split('.').map(|p| p.parse().ok()).collect();
    Some((parts?, caps.get(2).is_some_and(|m| m.as_str() == "-")))
}

fn model_compare(a: &str, b: &str) -> Option<Ordering> {
    let ((mut x, xpre), (mut y, ypre)) = (version_model(a)?, version_model(b)?);
    let len = x.len().max(y.len());
    x.resize(len, 0);
    y.resize(len, 0);
    Some(match x.cmp(&y) {
        Ordering::Equal => match (xpre, ypre) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => Ordering::Equal,
        },
        other => other,
    })
}

fn version_text(rng: &mut Rng) -> String {
    match rng.below(5) {
        0 => rng.garbage(10),
        1 => rng.tokens(
            &[
                "1",
                ".",
                "0",
                "2",
                "v",
                "-",
                "+",
                "rc1",
                "beta",
                "x",
                " ",
                "99999999999999999999",
            ],
            8,
        ),
        _ => {
            let mut s = String::new();
            for i in 0..rng.range(1, 4) {
                if i > 0 {
                    s.push('.');
                }
                s.push_str(&rng.below(4).to_string());
            }
            s.push_str(rng.pick(&["", "", "-rc1", "-beta.2", "+build5", "+b-1"]));
            if rng.chance(30) {
                s.insert(0, 'v');
            }
            s
        }
    }
}

#[test]
fn version_comparison_follows_a_regex_model() {
    let (mut ordered, mut rejected) = (0, 0);
    run_cases("update-compare-model", 8000, |_, rng| {
        let (a, b) = (version_text(rng), version_text(rng));
        let got = compare_versions(&a, &b);
        assert_eq!(got, model_compare(&a, &b), "{a:?} vs {b:?}");
        if got.is_some() {
            ordered += 1;
        } else {
            rejected += 1;
        }
    });
    assert!(ordered > 3000 && rejected > 500, "{ordered} {rejected}");
}

#[test]
fn version_comparison_is_a_consistent_preorder() {
    run_cases("update-compare-laws", 6000, |_, rng| {
        let (a, b, c) = (version_text(rng), version_text(rng), version_text(rng));
        let ab = compare_versions(&a, &b);
        let ba = compare_versions(&b, &a);
        assert_eq!(ab, ba.map(Ordering::reverse), "antisymmetry {a:?} {b:?}");
        if compare_versions(&a, &a).is_some() {
            assert_eq!(compare_versions(&a, &a), Some(Ordering::Equal), "{a:?}");
        }
        if let (Some(x), Some(y), Some(z)) =
            (ab, compare_versions(&b, &c), compare_versions(&a, &c))
        {
            if x == y {
                assert_eq!(z, x, "transitivity {a:?} {b:?} {c:?}");
            }
            if x == Ordering::Equal {
                assert_eq!(z, y, "equal versions are interchangeable {a:?} {b:?} {c:?}");
            }
        }
    });
}

#[test]
fn version_comparison_edge_cases() {
    let cmp = compare_versions;
    assert_eq!(cmp("1.0", "1.0.0"), Some(Ordering::Equal));
    assert_eq!(cmp("1.0.0", "1.0.0.1"), Some(Ordering::Less));
    assert_eq!(cmp("v1.2.3", "1.2.3"), Some(Ordering::Equal));
    assert_eq!(cmp("vv1.2.3", "1.2.3"), Some(Ordering::Equal));
    assert_eq!(cmp("1.2.3-rc1", "1.2.3"), Some(Ordering::Less));
    assert_eq!(cmp("1.2.3+build", "1.2.3"), Some(Ordering::Equal));
    assert_eq!(
        cmp("1.2.3-rc1", "1.2.3-beta"),
        Some(Ordering::Equal),
        "tags are not ordered"
    );
    assert_eq!(cmp("1.10.0", "1.9.0"), Some(Ordering::Greater));
    assert_eq!(cmp("  1.2.3 ", "1.2.3"), Some(Ordering::Equal));
    assert_eq!(cmp("1.2.3-rc+b-1", "1.2.3-rc"), Some(Ordering::Equal));
    for bad in [
        "",
        "v",
        "x",
        "1..2",
        ".1",
        "1.",
        "1.2.x",
        "-1",
        "1.2.3.",
        "99999999999999999999",
    ] {
        assert_eq!(cmp(bad, "1.0.0"), None, "{bad:?}");
        assert_eq!(cmp("1.0.0", bad), None, "{bad:?}");
    }
    assert_eq!(
        cmp(&"1.".repeat(2000), "1.0.0"),
        None,
        "a long malformed version is just rejected"
    );
    let many = vec!["1"; 5000].join(".");
    assert_eq!(cmp(&many, &many), Some(Ordering::Equal));
}

fn pin_args(rng: &mut Rng) -> Vec<String> {
    let vocab = [
        "--from",
        "--package",
        "-p",
        "-y",
        "--yes",
        "pkg",
        "@scope/pkg",
        "pkg@1.2.3",
        "pkg==1.0",
        "@scope/pkg@2",
        "--registry",
        "x",
        "",
        "@",
        "pkg@",
        "a==b==c",
        "-",
    ];
    (0..rng.range(0, 5))
        .map(|_| rng.pick(&vocab).to_string())
        .collect()
}

fn model_spec(args: &[String], uvx: bool) -> Option<(usize, String)> {
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if ["--from", "--package", "-p"].contains(&arg) {
            return args.get(i + 1).map(|_| (i + 1, args[i + 1].clone()));
        }
        if !arg.starts_with('-') {
            let _ = uvx;
            return Some((i, args[i].clone()));
        }
        i += 1;
    }
    None
}

#[test]
fn pin_specs_locate_the_package_argument_and_rewrite_it_cleanly() {
    let (mut found, mut versions) = (0, 0);
    run_cases("update-pin-spec", 6000, |_, rng| {
        let args = pin_args(rng);
        let uvx = rng.chance(50);
        let spec = parse_spec(&args, uvx);
        let want = model_spec(&args, uvx);
        assert_eq!(
            spec.as_ref().map(|s| s.arg_index),
            want.as_ref().map(|w| w.0),
            "{args:?}"
        );
        let (Some(spec), Some((_, raw))) = (spec, want) else {
            return;
        };
        found += 1;
        let rebuilt = match &spec.version {
            Some(v) => format!("{}{}{}", spec.name, spec.separator, v),
            None => spec.name.clone(),
        };
        assert_eq!(rebuilt, raw, "spec parts rebuild the argument");
        if spec.version.is_some() {
            versions += 1;
        }
        assert!(!spec.name.contains(spec.separator) || spec.name.starts_with('@') || uvx);

        let mut changed = args.clone();
        let new_version = "9.8.7";
        let did = rewrite(&mut changed, &spec, new_version);
        assert_eq!(
            did,
            raw != format!("{}{}{}", spec.name, spec.separator, new_version)
        );
        for (i, (a, b)) in args.iter().zip(&changed).enumerate() {
            if i != spec.arg_index {
                assert_eq!(a, b, "only the package argument changes");
            }
        }
        assert_eq!(
            changed[spec.arg_index],
            format!("{}{}{}", spec.name, spec.separator, new_version)
        );
        assert!(
            !rewrite(&mut changed, &spec, new_version),
            "second rewrite is a no-op"
        );
        let again = parse_spec(&changed, uvx).unwrap();
        assert_eq!(again.arg_index, spec.arg_index);
        if spec.name.is_empty() && spec.separator == "@" {
            assert_eq!(
                again.name,
                format!("@{new_version}"),
                "an empty name turns scoped"
            );
            return;
        }
        assert_eq!(
            again.version.as_deref(),
            Some(new_version),
            "{args:?} {spec:?} {changed:?}"
        );
        assert_eq!(again.name, spec.name);
    });
    assert!(found > 3000 && versions > 1000, "{found} {versions}");
}

#[test]
fn pin_spec_edge_cases() {
    let args = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let spec = |items: &[&str], uvx| parse_spec(&args(items), uvx);
    assert_eq!(spec(&[], false), None);
    assert_eq!(spec(&["-y"], false), None);
    assert_eq!(spec(&["--from"], true), None);
    assert_eq!(spec(&["--from", "x", "pkg"], false).unwrap().arg_index, 1);
    let scoped = spec(&["-y", "@scope/pkg@1.2.3"], false).unwrap();
    assert_eq!(
        scoped,
        PinSpec {
            arg_index: 1,
            name: "@scope/pkg".into(),
            version: Some("1.2.3".into()),
            separator: "@",
        }
    );
    let uvx = spec(&["tool==2.0"], true).unwrap();
    assert_eq!((uvx.name.as_str(), uvx.separator), ("tool", "=="));
    let npx = spec(&["tool==2.0"], false).unwrap();
    assert_eq!(
        (npx.name.as_str(), npx.version.as_deref()),
        ("tool==2.0", None)
    );
    assert_eq!(spec(&["@"], false).unwrap().name, "@");
    assert_eq!(spec(&["pkg@"], false).unwrap().version.as_deref(), Some(""));
    let mut short = vec!["only".to_string()];
    let stale = PinSpec {
        arg_index: 5,
        name: "x".into(),
        version: None,
        separator: "@",
    };
    assert!(!rewrite(&mut short, &stale, "1.0.0"));
    assert_eq!(short, vec!["only"]);
    for floating in ["latest", "next", "*", ""] {
        assert!(is_floating(floating));
    }
    for fixed in ["1.2.3", "Latest", " ", "^1.0.0"] {
        assert!(!is_floating(fixed));
    }
}

#[test]
fn registry_lookups_read_only_a_string_version_and_build_safe_urls() {
    run_cases("update-latest-lookups", 1500, |_, rng| {
        let version = rng.garbage(6);
        let body = match rng.below(8) {
            0 => rng.garbage(30),
            1 => "{}".to_string(),
            2 => json!({"version": 5, "info": {"version": null}}).to_string(),
            3 => json!({"version": version, "info": {"version": version}}).to_string(),
            4 => json!({"info": "text"}).to_string(),
            5 => json!([1, 2]).to_string(),
            6 => json!({"version": "1.2.3"}).to_string(),
            _ => json!({"info": {"version": "4.5.6"}}).to_string(),
        };
        let parsed: Option<Value> = serde_json::from_str(&body).ok();
        let top = parsed
            .as_ref()
            .and_then(|v| v.get("version"))
            .and_then(Value::as_str);
        let nested = parsed
            .as_ref()
            .and_then(|v| v.get("info"))
            .and_then(|v| v.get("version"))
            .and_then(Value::as_str);
        let package = *rng.pick(&["left-pad", "@scope/pkg", "weird name", "a/b/c"]);
        let registry = *rng.pick(&[
            "https://registry.example.invalid",
            "https://registry.example.invalid/",
            "http://127.0.0.1:1//",
        ]);

        let http = Canned::ok(body.clone());
        let npm = latest_npm(&http, registry, package);
        assert_eq!(npm.as_deref().ok(), top, "{body}");
        let seen = http.seen.borrow();
        assert_eq!(
            seen[0].0,
            format!(
                "{}/{}/latest",
                registry.trim_end_matches('/'),
                package.replace('/', "%2f")
            )
        );
        assert!(
            !seen[0]
                .0
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .contains("//")
                || registry.ends_with("//")
        );
        assert_eq!(
            seen[0].1,
            vec![("Accept".to_string(), "application/json".to_string())]
        );

        let http = Canned::ok(body.clone());
        let pypi = latest_pypi(&http, registry, package);
        assert_eq!(pypi.as_deref().ok(), nested, "{body}");
        assert_eq!(
            http.seen.borrow()[0].0,
            format!("{}/pypi/{package}/json", registry.trim_end_matches('/'))
        );
    });
    let down = Canned::status(Some(503));
    assert!(latest_npm(&down, "https://r.invalid", "p")
        .unwrap_err()
        .contains("HTTP 503"));
    assert!(latest_pypi(&down, "https://r.invalid", "p")
        .unwrap_err()
        .contains("HTTP 503"));
}

#[test]
fn repo_names_follow_the_owner_slash_name_rule() {
    let shape = Regex::new(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$").unwrap();
    run_cases("update-valid-repo", 8000, |_, rng| {
        let repo = match rng.below(3) {
            0 => rng.garbage(14),
            _ => rng.tokens(
                &[
                    "a", "B", "0", "_", ".", "-", "/", "..", "é", " ", "owner", "repo",
                ],
                7,
            ),
        };
        let parts: Vec<&str> = repo.split('/').collect();
        let want = shape.is_match(&repo) && parts.iter().all(|p| *p != "." && *p != "..");
        assert_eq!(valid_repo(&repo), want, "{repo:?}");
    });
    for ok in ["a/b", "owner/repo.name", "o_w-n/r-1", "a/.hidden", "a/b..c"] {
        assert!(valid_repo(ok), "{ok}");
    }
    for bad in [
        "", "a", "a/", "/b", "a/b/c", "./b", "a/..", "../b", "a b/c", "a/b\n", "é/b",
    ] {
        assert!(!valid_repo(bad), "{bad:?}");
    }
}

#[test]
fn placeholders_resolve_in_one_pass_unless_a_value_contains_one() {
    let placeholders = Regex::new(r"\{(version|os|arch)\}").unwrap();
    run_cases("update-resolve-pattern", 4000, |_, rng| {
        let pattern = rng.tokens(
            &[
                "tool",
                "-",
                "{version}",
                "{os}",
                "{arch}",
                "{x}",
                "{",
                "}",
                "_",
                ".zip",
            ],
            8,
        );
        let (version, os, arch) = (
            rng.string("0123456789.v-", 8),
            rng.string("abcdefghijklmnopqrstuvwxyz", 8),
            rng.string("amd6486", 6),
        );
        let want = placeholders
            .replace_all(&pattern, |c: &regex::Captures| match &c[1] {
                "version" => version.clone(),
                "os" => os.clone(),
                _ => arch.clone(),
            })
            .into_owned();
        assert_eq!(
            resolve_pattern(&pattern, &version, &os, &arch),
            want,
            "{pattern:?}"
        );
        assert_eq!(
            resolve_pattern(&want, &version, &os, &arch),
            want,
            "resolved text has no placeholders left"
        );
    });
    assert_eq!(
        resolve_pattern("{version}", "{os}", "linux", "amd64"),
        "linux"
    );
    assert_eq!(resolve_pattern("", "1", "a", "b"), "");
}

fn hash_text(rng: &mut Rng) -> String {
    let mut h: String = (0..64)
        .map(|_| *rng.pick(&['0', '1', '9', 'a', 'f', 'A', 'F']))
        .collect();
    if rng.chance(15) {
        h.pop();
    }
    if rng.chance(10) {
        h.push('0');
    }
    if rng.chance(10) {
        h.replace_range(0..1, "g");
    }
    h
}

fn is_hash(s: &str) -> bool {
    static HASH: OnceLock<Regex> = OnceLock::new();
    HASH.get_or_init(|| Regex::new("^[0-9a-fA-F]{64}$").unwrap())
        .is_match(s)
}

#[test]
fn checksum_files_are_matched_by_the_first_exact_name() {
    let (mut hits, mut misses, mut bare) = (0, 0, 0);
    run_cases("update-expected-hash", 6000, |_, rng| {
        let asset = *rng.pick(&["tool.tar.gz", "tool-1.2.3-linux-amd64", "a b.zip"]);
        let mut lines: Vec<String> = Vec::new();
        for _ in 0..rng.range(0, 6) {
            let name = *rng.pick(&[asset, "other.tar.gz", "tool.tar.gz.sig", "x"]);
            lines.push(match rng.below(8) {
                0 => format!("{}  {name}", hash_text(rng)),
                1 => format!("{} *{name}", hash_text(rng)),
                2 => format!("{}  ./{name}", hash_text(rng)),
                3 => format!("{}\t{name}\textra", hash_text(rng)),
                4 => rng.garbage(20).replace('\n', " "),
                5 => hash_text(rng),
                6 => String::new(),
                _ => format!("  {}   {name}  ", hash_text(rng)),
            });
        }
        let eol = *rng.pick(&["\n", "\r\n"]);
        let text = lines.join(eol);

        let kept: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let mut want = None;
        for line in &kept {
            let mut parts = line.split_whitespace();
            let (Some(h), Some(n)) = (parts.next(), parts.next()) else {
                continue;
            };
            let n = n.trim_start_matches('*').trim_start_matches("./");
            if is_hash(h) && n == asset {
                want = Some(h.to_ascii_lowercase());
                break;
            }
        }
        if want.is_none() && kept.len() == 1 {
            let mut parts = kept[0].split_whitespace();
            if let (Some(h), None) = (parts.next(), parts.next()) {
                if is_hash(h) {
                    want = Some(h.to_ascii_lowercase());
                    bare += 1;
                }
            }
        }
        let got = expected_hash(&text, asset);
        assert_eq!(got, want, "{text:?} for {asset}");
        match got {
            Some(h) => {
                assert!(h.len() == 64 && !h.chars().any(|c| c.is_ascii_uppercase()));
                hits += 1;
            }
            None => misses += 1,
        }
    });
    assert!(
        hits > 800 && misses > 800 && bare > 30,
        "{hits} {misses} {bare}"
    );
}

const PATTERN: &str = "tool-{version}-{os}-{arch}";
const PLATFORM: (&str, &str) = ("linux", "amd64");

fn lower_name_is_metadata(name: &str) -> bool {
    static PATTERNS: OnceLock<(Regex, Regex)> = OnceLock::new();
    let (suffix, exact) = PATTERNS.get_or_init(|| {
        (
            Regex::new(r"(?i)(\.sha256|\.sha256sum|\.sha512|\.asc|\.sig|\.crt|\.sbom|\.pem|\.jsonl)$").unwrap(),
            Regex::new(r"(?i)^(checksums\.txt|sha256sums\.txt|sha256sums|dist-manifest\.json|changelog\.md|license)$").unwrap(),
        )
    });
    suffix.is_match(name) || exact.is_match(name)
}

fn model_check(
    repo: &str,
    reply: &Result<String, HttpError>,
    current: Option<&str>,
    pattern: Option<&str>,
) -> Result<Checked, String> {
    if !valid_repo(repo) {
        return Err(format!("invalid repo: {repo}"));
    }
    let pattern =
        pattern.ok_or("no asset pattern configured; set asset_pattern in the source metadata")?;
    let body = match reply {
        Ok(b) => b,
        Err(e) => {
            return Err(match e.status {
                Some(404) => format!("repo not found: {repo}"),
                Some(403) | Some(429) => "rate limited: set GITHUB_TOKEN for higher limits".into(),
                _ => format!("could not reach the release API: {e}"),
            })
        }
    };
    let release: Value =
        serde_json::from_str(body).map_err(|_| "release API returned invalid JSON".to_string())?;
    let tag = release
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or("release has no tag_name")?
        .to_string();
    let version = tag.trim_start_matches('v').to_string();
    match model_compare(current.unwrap_or("0.0.0"), &version) {
        None => return Err(format!("could not parse version '{tag}'")),
        Some(Ordering::Less) => {}
        Some(_) => return Ok(Checked::UpToDate { latest: version }),
    }
    let mut assets: Vec<(String, String)> = Vec::new();
    for item in release
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(n), Some(u)) = (
            item.get("name").and_then(Value::as_str),
            item.get("browser_download_url").and_then(Value::as_str),
        ) {
            assets.push((n.to_string(), u.to_string()));
        }
    }
    if assets.is_empty() {
        return Err(format!("release {tag} has no downloadable assets"));
    }
    let resolved = pattern
        .replace("{version}", &version)
        .replace("{os}", PLATFORM.0)
        .replace("{arch}", PLATFORM.1);
    let mut best: Option<&(String, String)> = None;
    for a in &assets {
        if a.0.starts_with(&resolved)
            && !lower_name_is_metadata(&a.0)
            && best.is_none_or(|b| a.0.len() < b.0.len())
        {
            best = Some(a);
        }
    }
    let Some(matched) = best else {
        let names: Vec<&str> = assets.iter().map(|(n, _)| n.as_str()).collect();
        return Err(format!(
            "no asset matching '{resolved}'; available: {}",
            names.join(", ")
        ));
    };
    let lower = matched.0.to_ascii_lowercase();
    let candidates = [
        format!("{lower}.sha256"),
        format!("{lower}.sha256sum"),
        "checksums.txt".to_string(),
        "sha256sums.txt".to_string(),
        "sha256sums".to_string(),
        "checksums.sha256".to_string(),
    ];
    let mut checksum = None;
    'outer: for c in &candidates {
        for a in &assets {
            if a.0.to_ascii_lowercase() == *c {
                checksum = Some(a);
                break 'outer;
            }
        }
    }
    Ok(Checked::Available(ReleaseCheck {
        tag,
        version,
        asset_name: matched.0.clone(),
        asset_url: matched.1.clone(),
        checksum_name: checksum.map(|c| c.0.clone()),
        checksum_url: checksum.map(|c| c.1.clone()),
    }))
}

fn release_body(rng: &mut Rng) -> String {
    let tag = match rng.below(16) {
        0 => None,
        1 => Some(json!(7)),
        2 => Some(Value::Null),
        _ => Some(json!(*rng.pick(&[
            "v1.2.3",
            "1.2.3",
            "v2.0.0-rc1",
            "x",
            "",
            "v",
            "1.2",
            "v0.0.1",
            "vv3.0.0",
            "v1.2.3+b"
        ]))),
    };
    let tag_text = tag
        .as_ref()
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let version = tag_text.trim_start_matches('v').to_string();
    let prefix = resolve_pattern(PATTERN, &version, PLATFORM.0, PLATFORM.1);
    let mut assets = Vec::new();
    for _ in 0..rng.range(0, 7) {
        let name = match rng.below(12) {
            0 => format!("{prefix}.tar.gz"),
            1 => prefix.clone(),
            2 => format!("{prefix}.sha256"),
            3 => "checksums.txt".to_string(),
            4 => "SHA256SUMS".to_string(),
            5 => format!("{prefix}-musl.tar.gz"),
            6 => format!("{prefix}.TAR.GZ.SHA256SUM"),
            7 => "other.zip".to_string(),
            8 => "LICENSE".to_string(),
            9 => format!("{prefix}.sig"),
            10 => format!("{}.sha256", prefix.to_ascii_uppercase()),
            _ => rng.garbage(8),
        };
        let mut asset = Map::new();
        if rng.chance(95) {
            asset.insert("name".into(), json!(name));
        }
        if rng.chance(93) {
            asset.insert(
                "browser_download_url".into(),
                json!(format!("https://dl.example.invalid/{name}")),
            );
        }
        if rng.chance(4) {
            asset.insert("name".into(), json!(5));
        }
        assets.push(Value::Object(asset));
    }
    let mut release = Map::new();
    if let Some(tag) = tag {
        release.insert("tag_name".into(), tag);
    }
    match rng.below(8) {
        0 => {}
        1 => {
            release.insert("assets".into(), json!("none"));
        }
        _ => {
            release.insert("assets".into(), Value::Array(assets));
        }
    }
    Value::Object(release).to_string()
}

#[test]
fn release_checks_match_a_reimplemented_selection_model() {
    let (mut available, mut up_to_date, mut failed) = (0, 0, 0);
    let mut kinds: std::collections::BTreeMap<String, usize> = Default::default();
    run_cases("update-release-check", 6000, |_, rng| {
        let repo = *rng.pick(&[
            "owner/tool",
            "owner/tool",
            "owner/tool",
            "owner/tool",
            "owner/tool",
            "bad repo",
            "a/b/c",
        ]);
        let current = *rng.pick(&[
            None,
            Some("0.0.0"),
            Some("1.2.3"),
            Some("v1.2.3"),
            Some("2.0.0"),
            Some("garbage"),
            Some("1.2.2"),
        ]);
        let pattern = *rng.pick(&[
            Some(PATTERN),
            Some(PATTERN),
            Some(PATTERN),
            Some(PATTERN),
            Some(PATTERN),
            Some("tool-"),
            Some(""),
            None,
        ]);
        let reply: Result<String, HttpError> = match rng.below(24) {
            0 => Err(HttpError::new(Some(404), "x")),
            1 => Err(HttpError::new(Some(*rng.pick(&[403, 429])), "x")),
            2 => Err(HttpError::new(*rng.pick(&[None, Some(500)]), "boom")),
            3 => Ok(rng.garbage(40)),
            _ => Ok(release_body(rng)),
        };
        let http = Canned {
            reply: reply.clone(),
            seen: RefCell::default(),
        };
        let platform = (PLATFORM.0.to_string(), PLATFORM.1.to_string());
        let token = rng.chance(30).then_some("synthetic-token");
        let got = check(
            &http,
            "https://api.example.invalid/",
            token,
            repo,
            current,
            pattern,
            &platform,
        );
        let want = model_check(repo, &reply, current, pattern);
        match (&got, &want) {
            (Ok(Checked::Available(_)), _) => available += 1,
            (Ok(Checked::UpToDate { .. }), _) => up_to_date += 1,
            (Err(e), _) => {
                failed += 1;
                *kinds.entry(e.chars().take(18).collect()).or_default() += 1;
            }
        }
        assert_eq!(got, want, "{repo:?} {current:?} {pattern:?} {reply:?}");
        let seen = http.seen.borrow();
        for (url, headers) in seen.iter() {
            assert_eq!(
                url,
                "https://api.example.invalid/repos/owner/tool/releases/latest"
            );
            assert_eq!(
                headers
                    .iter()
                    .any(|(k, v)| k == "Authorization" && v == "Bearer synthetic-token"),
                token.is_some()
            );
        }
    });
    assert!(
        available > 200 && up_to_date > 400 && failed > 500 && kinds.len() >= 14,
        "{available} {up_to_date} {failed} {kinds:#?}"
    );
}

#[test]
fn release_selection_prefers_the_shortest_matching_name() {
    let body = json!({
        "tag_name": "v1.2.3",
        "assets": [
            {"name": "tool-1.2.3-linux-amd64-musl.tar.gz", "browser_download_url": "https://x.invalid/musl"},
            {"name": "tool-1.2.3-linux-amd64.tar.gz", "browser_download_url": "https://x.invalid/gz"},
            {"name": "tool-1.2.3-linux-amd64.tar.gz.sha256", "browser_download_url": "https://x.invalid/sum"},
            {"name": "tool-1.2.3-linux-amd64.sig", "browser_download_url": "https://x.invalid/sig"},
        ]
    });
    let http = Canned::ok(body.to_string());
    let platform = ("linux".to_string(), "amd64".to_string());
    let got = check(
        &http,
        "https://api.example.invalid",
        None,
        "o/tool",
        Some("1.0.0"),
        Some(PATTERN),
        &platform,
    )
    .unwrap();
    let Checked::Available(found) = got else {
        panic!("expected an update");
    };
    assert_eq!(found.asset_name, "tool-1.2.3-linux-amd64.tar.gz");
    assert_eq!(
        found.checksum_name.as_deref(),
        Some("tool-1.2.3-linux-amd64.tar.gz.sha256")
    );
    let same = check(
        &http,
        "https://api.example.invalid",
        None,
        "o/tool",
        Some("1.2.3"),
        Some(PATTERN),
        &platform,
    )
    .unwrap();
    assert_eq!(
        same,
        Checked::UpToDate {
            latest: "1.2.3".into()
        }
    );
}

#[test]
fn an_unparseable_current_version_is_reported_against_the_tag() {
    let http = Canned::ok(json!({"tag_name": "v1.2.3", "assets": []}).to_string());
    let platform = ("linux".to_string(), "amd64".to_string());
    let err = check(
        &http,
        "https://api.example.invalid",
        None,
        "o/tool",
        Some("garbage"),
        Some(PATTERN),
        &platform,
    )
    .unwrap_err();
    assert_eq!(err, "could not parse version 'v1.2.3'");
}

fn url_text(rng: &mut Rng) -> String {
    format!(
        "{}://{}{}{}{}",
        rng.pick(&["https", "http", "HTTP", "Https", "ftp", "file", ""]),
        rng.pick(&[
            "",
            "user@",
            "localhost@",
            "user:pw@",
            "evil.example\\@",
            "a@b@",
            "127.0.0.1@"
        ]),
        rng.pick(&[
            "example.com",
            "localhost",
            "LOCALHOST",
            "127.0.0.1",
            "127.1.2.3",
            "[::1]",
            "[::2]",
            "localhost.evil.example",
            "127.0.0.1.evil.example",
            "0.0.0.0",
            "",
            "evil.example\\",
            "local\thost",
            "evil.example#@localhost",
            "evil.example?@localhost",
        ]),
        rng.pick(&["", ":8080", ":", ":abc", ":99999"]),
        rng.pick(&[
            "",
            "/",
            "/x",
            "?q=1",
            "#f",
            "/a@localhost",
            "\\@localhost",
            " /x",
            "/\u{0}"
        ]),
    )
}

fn same_host(parsed: &url::Url, host: &str) -> bool {
    let text = parsed.host_str().unwrap_or("");
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        return text.parse::<std::net::Ipv4Addr>() == Ok(ip);
    }
    if host.contains(':') {
        return text == format!("[{host}]");
    }
    text == host
}

#[test]
fn the_url_policy_agrees_with_a_standards_parser_about_the_host() {
    let (mut loopback, mut remote, mut parsed_count) = (0, 0, 0);
    run_cases("update-url-policy", 12000, |_, rng| {
        let url = url_text(rng);
        let host = host_of(&url);
        let allowed = url_allowed(&url).is_ok();
        let Ok(parsed) = url::Url::parse(&url) else {
            return;
        };
        parsed_count += 1;
        if let Some(h) = &host {
            let stripped = url.contains(['\t', '\n', '\r']);
            if matches!(parsed.scheme(), "http" | "https") && parsed.has_host() && !stripped {
                assert!(
                    same_host(&parsed, h),
                    "host_of {h:?} disagrees with the parser for {url:?}"
                );
            }
        }
        if !allowed {
            return;
        }
        let real = parsed
            .host()
            .unwrap_or_else(|| panic!("allowed without a host {url:?}"));
        match parsed.scheme() {
            "https" => remote += 1,
            "http" => {
                let local = match real {
                    url::Host::Domain(d) => d == "localhost",
                    url::Host::Ipv4(ip) => ip.is_loopback(),
                    url::Host::Ipv6(ip) => ip.is_loopback(),
                };
                assert!(
                    local,
                    "plain http to a non-loopback host was allowed: {url:?}"
                );
                loopback += 1;
            }
            other => panic!("scheme {other} was allowed: {url:?}"),
        }
    });
    assert!(
        loopback > 100 && remote > 100 && parsed_count > 2000,
        "{loopback} {remote} {parsed_count}"
    );
}

#[test]
fn url_policy_edge_cases() {
    for ok in [
        "https://example.com/x",
        "HTTPS://EXAMPLE.com",
        "http://localhost:8080/x",
        "http://127.0.0.1/x",
        "http://[::1]:9/x",
        "http://127.9.9.9/",
        "http://user@localhost/",
    ] {
        assert!(url_allowed(ok).is_ok(), "{ok}");
    }
    for bad in [
        "http://example.com/",
        "http://localhost.evil.example/",
        "http://127.0.0.1.evil.example/",
        "http://localhost@evil.example/",
        "http://0.0.0.0/",
        "http://[::2]/",
        "ftp://example.com/",
        "file:///etc/passwd",
        "example.com",
        "",
        "https://",
        "https:///path",
        "http://",
    ] {
        assert!(url_allowed(bad).is_err(), "{bad}");
    }
    assert_eq!(
        host_of("https://user:pw@Example.COM:8443/p?q#f").as_deref(),
        Some("example.com")
    );
    assert_eq!(host_of("http://[::1]:80/").as_deref(), Some("::1"));
    assert_eq!(host_of("nonsense"), None);
}

fn some_text(rng: &mut Rng) -> String {
    let mut text = rng.garbage(10);
    if text.is_empty() {
        text.push('x');
    }
    text
}

fn random_source(rng: &mut Rng) -> Source {
    let opt = |rng: &mut Rng| rng.chance(50).then(|| some_text(rng));
    match rng.below(6) {
        0 => Source::Git {
            path: some_text(rng),
            remote_url: opt(rng),
            branch: opt(rng),
            post_update: opt(rng),
        },
        1 => Source::GithubRelease {
            path: some_text(rng),
            repo: opt(rng),
            current_version: opt(rng),
            asset_pattern: opt(rng),
            verify_command: opt(rng),
        },
        2 => Source::Npx {
            package: some_text(rng),
        },
        3 => Source::Uvx {
            package: some_text(rng),
        },
        4 => Source::Remote,
        _ => Source::Unknown {
            reason: some_text(rng),
        },
    }
}

fn meta_value(rng: &mut Rng, depth: usize) -> Value {
    match rng.below(if depth == 0 { 5 } else { 7 }) {
        0 => Value::Null,
        1 => json!(rng.chance(50)),
        2 => json!(rng.below(100)),
        3 => json!(rng.garbage(6)),
        4 => json!(*rng.pick(&[
            "git",
            "github-release",
            "npx",
            "uvx",
            "remote",
            "unknown",
            ""
        ])),
        5 => json!([meta_value(rng, depth - 1)]),
        _ => {
            let mut map = Map::new();
            for key in [
                "type",
                "path",
                "package",
                "repo",
                "remote_url",
                "reason",
                "x",
            ] {
                if rng.chance(45) {
                    let value = meta_value(rng, depth - 1);
                    map.insert(key.to_string(), value);
                }
            }
            Value::Object(map)
        }
    }
}

#[test]
fn stored_source_metadata_round_trips_and_ignores_garbage() {
    run_cases("update-source-meta", 3000, |_, rng| {
        let source = random_source(rng);
        let meta = Value::Object(source.to_meta());
        assert_eq!(from_meta(&meta), Some(source.clone()), "{meta}");
        assert_eq!(meta["type"], source.kind());
        let _ = from_meta(&meta_value(rng, 3));
    });
}

#[test]
fn path_expansion_only_touches_a_leading_tilde() {
    let home = PathBuf::from("/home/synthetic");
    run_cases("update-expand-path", 3000, |_, rng| {
        let raw = rng.tokens(&["~", "/", "~/", "a", "..", "é", " ", "~x", ""], 5);
        let got = expand_path(&raw, Some(&home));
        if raw == "~" || raw.starts_with("~/") {
            assert!(got.starts_with(&home), "{raw:?} -> {got:?}");
        } else {
            assert_eq!(got, PathBuf::from(&raw), "{raw:?}");
        }
        assert_eq!(
            expand_path(&raw, None).as_os_str().is_empty(),
            raw.is_empty()
        );
    });
}

#[test]
fn path_expansion_edge_cases() {
    let home = PathBuf::from("/home/synthetic");
    for (raw, want) in [
        ("~", "/home/synthetic"),
        ("~/repo", "/home/synthetic/repo"),
        ("~//repo", "/home/synthetic/repo"),
        ("~///etc/passwd", "/home/synthetic/etc/passwd"),
        ("~repo", "~repo"),
        ("/abs", "/abs"),
        ("rel/x", "rel/x"),
        ("a/~/b", "a/~/b"),
    ] {
        assert_eq!(expand_path(raw, Some(&home)), PathBuf::from(want), "{raw}");
    }
    assert_eq!(expand_path("~", None), PathBuf::from("~"));
    assert_eq!(expand_path("~/x", None), PathBuf::from("~/x"));
}

#[test]
fn a_backslash_does_not_hide_the_real_host() {
    assert_eq!(
        host_of("https://evil.example\\@api.example/x").as_deref(),
        Some("evil.example")
    );
    assert_eq!(
        host_of("https://evil.example\\@api.example").as_deref(),
        Some("evil.example")
    );
    assert!(url_allowed("http://evil.example\\@localhost/").is_err());
}

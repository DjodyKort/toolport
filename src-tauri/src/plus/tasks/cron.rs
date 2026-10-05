//! Five-field cron in UTC (minute hour day-of-month month day-of-week) and RFC 3339 formatting.
//! No date crate: the civil-date arithmetic is the standard days-since-epoch conversion.

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn rfc3339(epoch: i64) -> String {
    let (y, m, d) = civil_from_days(epoch.div_euclid(86400));
    let s = epoch.rem_euclid(86400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", s / 3600, s % 3600 / 60, s % 60)
}

pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let b = text.strip_suffix('Z')?;
    let (date, time) = b.split_once('T')?;
    let d: Vec<i64> = date.split('-').filter_map(|p| p.parse().ok()).collect();
    let t: Vec<i64> = time.split(':').filter_map(|p| p.parse().ok()).collect();
    (d.len() == 3 && t.len() == 3).then(|| days_from_civil(d[0], d[1], d[2]) * 86400 + t[0] * 3600 + t[1] * 60 + t[2])
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cron {
    minutes: Vec<bool>,
    hours: Vec<bool>,
    days: Vec<bool>,
    months: Vec<bool>,
    weekdays: Vec<bool>,
    day_any: bool,
    weekday_any: bool,
}

fn field(text: &str, lo: usize, hi: usize, name: &str) -> Result<(Vec<bool>, bool), String> {
    let mut set = vec![false; hi + 1];
    for part in text.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<usize>().ok().filter(|s| *s > 0).ok_or_else(|| format!("{name}: bad step in {part:?}"))?),
            None => (part, 1),
        };
        let (from, to) = if range == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            (a.parse::<usize>().map_err(|_| format!("{name}: bad range {range:?}"))?, b.parse::<usize>().map_err(|_| format!("{name}: bad range {range:?}"))?)
        } else {
            let v = range.parse::<usize>().map_err(|_| format!("{name}: bad value {range:?}"))?;
            (v, if part.contains('/') { hi } else { v })
        };
        if from < lo || to > hi || from > to {
            return Err(format!("{name}: {part:?} is outside {lo}-{hi}"));
        }
        let mut v = from;
        while v <= to {
            set[v] = true;
            v += step;
        }
    }
    Ok((set, text.starts_with('*')))
}

impl Cron {
    pub fn parse(text: &str) -> Result<Cron, String> {
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(format!("{text:?} needs five fields (minute hour day-of-month month day-of-week)"));
        }
        let (minutes, _) = field(parts[0], 0, 59, "minute")?;
        let (hours, _) = field(parts[1], 0, 23, "hour")?;
        let (days, day_any) = field(parts[2], 1, 31, "day-of-month")?;
        let (months, _) = field(parts[3], 1, 12, "month")?;
        let (mut weekdays, weekday_any) = field(parts[4], 0, 7, "day-of-week")?;
        if weekdays[7] {
            weekdays[0] = true;
        }
        Ok(Cron { minutes, hours, days, months, weekdays, day_any, weekday_any })
    }

    pub fn matches(&self, epoch: i64) -> bool {
        let day = epoch.div_euclid(86400);
        let (_, month, dom) = civil_from_days(day);
        let dow = (day + 4).rem_euclid(7) as usize;
        let s = epoch.rem_euclid(86400);
        let day_ok = match (self.day_any, self.weekday_any) {
            (false, false) => self.days[dom as usize] || self.weekdays[dow],
            _ => self.days[dom as usize] && self.weekdays[dow],
        };
        self.minutes[(s % 3600 / 60) as usize] && self.hours[(s / 3600) as usize] && self.months[month as usize] && day_ok
    }

    pub fn next_after(&self, epoch: i64) -> Option<i64> {
        let mut t = epoch - epoch.rem_euclid(60) + 60;
        for _ in 0..(366 * 24 * 60 * 5) {
            if self.matches(t) {
                return Some(t);
            }
            t += 60;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_matches_in_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_781_000_000), "2026-06-09T10:13:20Z");
        let c = Cron::parse("*/15 9 * * 1-5").unwrap();
        let tue_0900 = days_from_civil(2026, 10, 6) * 86400 + 9 * 3600;
        assert!(c.matches(tue_0900) && c.matches(tue_0900 + 900) && !c.matches(tue_0900 + 60));
        assert!(!c.matches(days_from_civil(2026, 10, 10) * 86400 + 9 * 3600));
        assert_eq!(c.next_after(tue_0900), Some(tue_0900 + 900));
        assert_eq!(Cron::parse("0 0 1 1 *").unwrap().next_after(tue_0900), Some(days_from_civil(2027, 1, 1) * 86400));
    }

    #[test]
    fn rejects_malformed_expressions_readably() {
        assert!(Cron::parse("* * * *").unwrap_err().contains("five fields"));
        assert!(Cron::parse("61 * * * *").unwrap_err().contains("minute"));
        assert!(Cron::parse("*/0 * * * *").unwrap_err().contains("step"));
    }
}

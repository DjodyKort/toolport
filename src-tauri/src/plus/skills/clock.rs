use crate::usage_report::civil_from_days;

pub trait Clock {
    fn now(&self) -> Instant;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Instant {
    pub unix_secs: i64,
    pub micros: u32,
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Instant {
            unix_secs: d.as_secs() as i64,
            micros: d.subsec_micros(),
        }
    }
}

pub struct FixedClock(pub Instant);

impl Clock for FixedClock {
    fn now(&self) -> Instant {
        self.0
    }
}

impl Instant {
    fn parts(&self) -> (i64, u32, u32, i64, i64, i64) {
        let days = self.unix_secs.div_euclid(86_400);
        let rem = self.unix_secs.rem_euclid(86_400);
        let (y, m, d) = civil_from_days(days);
        (y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
    }

    /// Python `datetime.now(timezone.utc).isoformat()`: microseconds are omitted when zero.
    pub fn isoformat(&self) -> String {
        let (y, mo, d, h, mi, s) = self.parts();
        let frac = if self.micros == 0 {
            String::new()
        } else {
            format!(".{:06}", self.micros)
        };
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}{frac}+00:00")
    }

    /// `%Y%m%dT%H%M%SZ`, the suffix of collision backups.
    pub fn backup_stamp(&self) -> String {
        let (y, mo, d, h, mi, s) = self.parts();
        format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z")
    }
}

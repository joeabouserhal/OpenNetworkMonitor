use super::*;
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, TimeZone};

pub(super) const PERIODS: &[(i64, &str, &str)] = &[
    (300, "5m", "5 minutes"),
    (900, "15m", "15 minutes"),
    (1800, "30m", "30 minutes"),
    (3600, "1h", "1 hour"),
    (10800, "3h", "3 hours"),
    (21600, "6h", "6 hours"),
    (43200, "12h", "12 hours"),
    (86400, "24h", "24 hours"),
    (259200, "3d", "3 days"),
    (604800, "7d", "7 days"),
    (2592000, "30d", "30 days"),
    (7776000, "90d", "90 days"),
    (31536000, "1y", "1 year"),
    (0, "All time", "All time"),
];

pub(super) fn period_caption(filter: &Filter) -> &'static str {
    if filter.time_range.is_some() {
        "Custom"
    } else if filter.month_to_date() {
        "Month"
    } else {
        PERIODS
            .iter()
            .find(|p| p.0 == filter.seconds)
            .map_or("Custom", |p| p.1)
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum PeriodMode {
    Rolling,
    Dates,
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Unit {
    Minutes,
    Hours,
    Days,
    Weeks,
}
impl Unit {
    pub(super) const ALL: [Self; 4] = [Self::Minutes, Self::Hours, Self::Days, Self::Weeks];
    fn seconds(self) -> i64 {
        match self {
            Self::Minutes => 60,
            Self::Hours => 3600,
            Self::Days => 86400,
            Self::Weeks => 604800,
        }
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Minutes => "minutes",
            Self::Hours => "hours",
            Self::Days => "days",
            Self::Weeks => "weeks",
        }
    }
}
pub(super) struct CustomPeriod {
    pub(super) mode: PeriodMode,
    pub(super) amount: String,
    pub(super) unit: Unit,
    pub(super) from: DateTimeFields,
    pub(super) to: DateTimeFields,
    pub(super) error: String,
    pub(super) presented: bool,
}
impl CustomPeriod {
    pub(super) fn from_filter(filter: &Filter) -> Self {
        let now = chrono::Utc::now().timestamp();
        let (start, end) = if filter.all_time() {
            (now - 86400, now)
        } else {
            filter.time_bounds(now)
        };
        let duration = if filter.all_time() {
            86400
        } else if filter.month_to_date() {
            end.saturating_sub(start).max(60)
        } else {
            filter.seconds.max(60)
        };
        let unit = Unit::ALL
            .into_iter()
            .rev()
            .find(|u| duration % u.seconds() == 0)
            .unwrap_or(Unit::Minutes);
        Self {
            mode: if filter.time_range.is_some() || filter.month_to_date() {
                PeriodMode::Dates
            } else {
                PeriodMode::Rolling
            },
            amount: u32::try_from(duration / unit.seconds())
                .unwrap_or(u32::MAX)
                .max(1)
                .to_string(),
            unit,
            from: DateTimeFields::from_timestamp(start),
            to: DateTimeFields::from_timestamp(end),
            error: String::new(),
            presented: false,
        }
    }
    pub(super) fn selection(&self) -> std::result::Result<(i64, Option<TimeRange>), &'static str> {
        if self.mode == PeriodMode::Rolling {
            let amount = self
                .amount
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|amount| *amount > 0)
                .ok_or("Enter a whole number greater than zero.")?;
            return Ok((i64::from(amount) * self.unit.seconds(), None));
        }
        let start = self.from.timestamp()?;
        let end = self.to.timestamp()?.saturating_add(59);
        if start < 0 {
            return Err("Choose a start date in 1970 or later.");
        }
        if start > end {
            return Err("The end must be on or after the start.");
        }
        Ok((end - start + 1, Some(TimeRange { start, end })))
    }
}
#[derive(Clone)]
pub(super) struct DateTimeFields {
    pub(super) date: String,
    pub(super) time: String,
    pub(super) month: NaiveDate,
}
impl DateTimeFields {
    fn from_timestamp(timestamp: i64) -> Self {
        let date = Local
            .timestamp_opt(timestamp, 0)
            .single()
            .unwrap_or_else(Local::now);
        Self {
            date: date.format("%Y-%m-%d").to_string(),
            time: date.format("%H:%M").to_string(),
            month: date.date_naive().with_day(1).unwrap(),
        }
    }
    fn timestamp(&self) -> std::result::Result<i64, &'static str> {
        let date = NaiveDateTime::parse_from_str(
            &format!("{} {}", self.date.trim(), self.time.trim()),
            "%Y-%m-%d %H:%M",
        )
        .map_err(|_| "Enter dates as YYYY-MM-DD and times as HH:MM.")?;
        Local
            .from_local_datetime(&date)
            .single()
            .map(|d| d.timestamp())
            .ok_or(
                "This local time is ambiguous or skipped by a clock change. Choose another time.",
            )
    }
}

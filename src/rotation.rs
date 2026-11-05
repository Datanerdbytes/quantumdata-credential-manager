use chrono::{DateTime, Days, Months, NaiveDate};

use crate::models::Credential;

pub const WARN_DAYS: i64 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frequency {
    Days(i64),
    Months(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationAlert {
    pub name: String,
    /// Days until rotation; negative values mean the rotation is overdue.
    pub days: i64,
}

impl RotationAlert {
    pub fn is_overdue(&self) -> bool {
        self.days < 0
    }

    pub fn label(&self) -> String {
        match self.days {
            days if days < 0 => format!("overdue {}d", -days),
            0 => "due today".to_string(),
            1 => "due tomorrow".to_string(),
            days => format!("due in {days}d"),
        }
    }
}

pub fn parse_due_date(value: &str) -> Option<NaiveDate> {
    let value = value.trim();

    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .or_else(|| {
            DateTime::parse_from_rfc3339(value)
                .ok()
                .map(|timestamp| timestamp.date_naive())
        })
}

pub fn parse_frequency(value: &str) -> Option<Frequency> {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        return None;
    }

    if value.contains("year") || value.contains("annual") {
        return Some(Frequency::Months(12));
    }
    if value.contains("quarter") {
        return Some(Frequency::Months(3));
    }
    if value.contains("month") {
        return Some(Frequency::Months(
            number_in(&value).unwrap_or(1).max(1) as u32
        ));
    }
    if value.contains("week") {
        return Some(Frequency::Days(number_in(&value).unwrap_or(1).max(1) * 7));
    }
    if value.contains("day") {
        return Some(Frequency::Days(number_in(&value).unwrap_or(1).max(1)));
    }

    None
}

fn number_in(value: &str) -> Option<i64> {
    let digits: String = value
        .chars()
        .filter(|character| character.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

pub fn add_frequency(date: NaiveDate, frequency: Frequency) -> Option<NaiveDate> {
    match frequency {
        Frequency::Days(days) if days > 0 => date.checked_add_days(Days::new(days as u64)),
        Frequency::Months(months) if months > 0 => date.checked_add_months(Months::new(months)),
        _ => None,
    }
}

/// Uses the explicit due date when set, otherwise derives one from the last
/// rotation and the configured frequency.
pub fn effective_due_date(credential: &Credential) -> Option<NaiveDate> {
    if let Some(due) = credential
        .next_rotation_due
        .as_deref()
        .and_then(parse_due_date)
    {
        return Some(due);
    }

    let last = credential
        .last_rotated
        .as_deref()
        .and_then(parse_due_date)?;
    let frequency = parse_frequency(credential.rotation_frequency.as_deref()?)?;
    add_frequency(last, frequency)
}

pub fn due_alerts(credentials: &[Credential], today: NaiveDate) -> Vec<RotationAlert> {
    let mut alerts: Vec<RotationAlert> = credentials
        .iter()
        .filter_map(|credential| {
            let due = effective_due_date(credential)?;
            let days = (due - today).num_days();
            (days <= WARN_DAYS).then(|| RotationAlert {
                name: credential.name.clone(),
                days,
            })
        })
        .collect();

    alerts.sort_by(|left, right| {
        left.days
            .cmp(&right.days)
            .then_with(|| left.name.cmp(&right.name))
    });
    alerts
}

pub fn notification_body(alerts: &[RotationAlert]) -> String {
    let overdue = alerts.iter().filter(|alert| alert.is_overdue()).count();
    let due_soon = alerts.len() - overdue;

    let mut summary = Vec::new();
    if overdue > 0 {
        summary.push(format!("{overdue} overdue"));
    }
    if due_soon > 0 {
        summary.push(format!("{due_soon} due soon"));
    }

    let shown = if alerts.len() > 3 { 3 } else { alerts.len() };
    let mut names = alerts
        .iter()
        .take(shown)
        .map(|alert| alert.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if alerts.len() > shown {
        names.push_str(&format!(" +{} more", alerts.len() - shown));
    }

    format!("{}. {names}", summary.join(" · "))
}

pub fn notification_fingerprint(alerts: &[RotationAlert], today: NaiveDate) -> String {
    let alerts = alerts
        .iter()
        .map(|alert| format!("{}={}", alert.name, alert.days))
        .collect::<Vec<_>>()
        .join(";");

    format!("{today}|{alerts}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential(name: &str, due: Option<&str>) -> Credential {
        Credential {
            id: None,
            name: name.to_string(),
            description: None,
            service: None,
            username: None,
            status: Some("Active".into()),
            purpose: None,
            credential_reference: None,
            environment_variable: None,
            environment: None,
            repository: None,
            owner: None,
            rotation_frequency: None,
            last_rotated: None,
            next_rotation_due: due.map(str::to_string),
            created_at: None,
            updated_at: None,
        }
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 15).expect("valid date")
    }

    #[test]
    fn parses_iso_dates_and_timestamps() {
        assert_eq!(
            parse_due_date("2026-07-01"),
            NaiveDate::from_ymd_opt(2026, 7, 1)
        );
        assert_eq!(
            parse_due_date("2026-07-01T10:30:00Z"),
            NaiveDate::from_ymd_opt(2026, 7, 1)
        );
        assert!(parse_due_date("next tuesday").is_none());
        assert!(parse_due_date("").is_none());
    }

    #[test]
    fn flags_overdue_and_due_soon_in_order() {
        let credentials = vec![
            credential("Future", Some("2026-08-01")),
            credential("Soon", Some("2026-06-20")),
            credential("Overdue", Some("2026-06-10")),
            credential("Today", Some("2026-06-15")),
            credential("Missing", None),
        ];

        let alerts = due_alerts(&credentials, today());

        assert_eq!(
            alerts,
            vec![
                RotationAlert {
                    name: "Overdue".into(),
                    days: -5
                },
                RotationAlert {
                    name: "Today".into(),
                    days: 0
                },
                RotationAlert {
                    name: "Soon".into(),
                    days: 5
                },
            ]
        );
    }

    #[test]
    fn includes_the_warning_boundary_only() {
        let credentials = vec![
            credential("Boundary", Some("2026-06-22")),
            credential("Beyond", Some("2026-06-23")),
        ];

        let alerts = due_alerts(&credentials, today());

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].days, WARN_DAYS);
    }

    #[test]
    fn labels_read_naturally() {
        assert_eq!(
            RotationAlert {
                name: "A".into(),
                days: -2
            }
            .label(),
            "overdue 2d"
        );
        assert_eq!(
            RotationAlert {
                name: "A".into(),
                days: 0
            }
            .label(),
            "due today"
        );
        assert_eq!(
            RotationAlert {
                name: "A".into(),
                days: 1
            }
            .label(),
            "due tomorrow"
        );
        assert_eq!(
            RotationAlert {
                name: "A".into(),
                days: 4
            }
            .label(),
            "due in 4d"
        );
    }

    #[test]
    fn parses_rotation_frequencies() {
        assert_eq!(parse_frequency("Every 90 days"), Some(Frequency::Days(90)));
        assert_eq!(parse_frequency("every 2 weeks"), Some(Frequency::Days(14)));
        assert_eq!(
            parse_frequency("Every 6 months"),
            Some(Frequency::Months(6))
        );
        assert_eq!(parse_frequency("Quarterly"), Some(Frequency::Months(3)));
        assert_eq!(parse_frequency("Yearly"), Some(Frequency::Months(12)));
        assert_eq!(parse_frequency("monthly"), Some(Frequency::Months(1)));
        assert_eq!(parse_frequency("whenever"), None);
        assert_eq!(parse_frequency(""), None);
    }

    #[test]
    fn adds_days_and_months() {
        let date = NaiveDate::from_ymd_opt(2026, 1, 31).expect("valid date");

        assert_eq!(
            add_frequency(date, Frequency::Days(7)),
            NaiveDate::from_ymd_opt(2026, 2, 7)
        );
        assert_eq!(
            add_frequency(date, Frequency::Months(1)),
            NaiveDate::from_ymd_opt(2026, 2, 28)
        );
    }

    #[test]
    fn derives_a_due_date_when_missing() {
        let mut item = credential("OpenAI", None);
        item.last_rotated = Some("2026-06-01".into());
        item.rotation_frequency = Some("Every 90 days".into());

        assert_eq!(
            effective_due_date(&item),
            NaiveDate::from_ymd_opt(2026, 8, 30)
        );

        item.next_rotation_due = Some("2026-07-01".into());
        assert_eq!(
            effective_due_date(&item),
            NaiveDate::from_ymd_opt(2026, 7, 1)
        );
    }

    #[test]
    fn derived_dates_feed_alerts() {
        let mut item = credential("OpenAI", None);
        item.last_rotated = Some("2026-05-01".into());
        item.rotation_frequency = Some("Every 30 days".into());

        let alerts = due_alerts(&[item], today());

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].days, -15);
    }

    #[test]
    fn notification_body_summarizes_and_truncates_names() {
        let alerts = vec![
            RotationAlert {
                name: "A".into(),
                days: -3,
            },
            RotationAlert {
                name: "B".into(),
                days: 1,
            },
            RotationAlert {
                name: "C".into(),
                days: 2,
            },
            RotationAlert {
                name: "D".into(),
                days: 3,
            },
        ];

        assert_eq!(
            notification_body(&alerts),
            "1 overdue · 3 due soon. A, B, C +1 more"
        );
    }

    #[test]
    fn fingerprint_changes_with_due_dates() {
        let alerts = vec![RotationAlert {
            name: "OpenAI".into(),
            days: 3,
        }];
        let other = vec![RotationAlert {
            name: "OpenAI".into(),
            days: 4,
        }];

        assert_ne!(
            notification_fingerprint(&alerts, today()),
            notification_fingerprint(&other, today())
        );
    }
}

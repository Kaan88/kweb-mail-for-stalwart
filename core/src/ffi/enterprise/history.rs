//! The dashboard's history: the server's stored metric rows, put into one point per hour or
//! day. The names are those of the web dashboard (`dev/notes/stalwart-webui.md`, section 4).

use chrono::DateTime;
use serde_json::Value;

use super::{ChartPoint, DashboardHistory};
use crate::{Error, Result};

/// Messages the server took in.
pub const RECEIVED: &str = "queue.message-queued";
/// What the server sent: the metrics the web dashboard adds up as "Sent" (messages its accounts
/// sent, delivery reports and other reports). The server stores only a fixed list of metrics
/// and "delivery.delivered" is not one of them (it is a trace event only).
pub const SENT: [&str; 3] = ["queue.authenticated-message-queued", "queue.dsn-queued", "queue.report-queued"];
/// How long a delivery took, from the first attempt to the end (a histogram, in milliseconds).
pub const DELIVERY_TIME: &str = "delivery.total-time";

const HOUR: i64 = 3600;
const DAY: i64 = 86_400;

/// The points a chart has: where the first one starts, how far apart they are and how many.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: i64,
    pub step: i64,
    pub count: usize,
}

/// The span for the last `hours`, ending with the bucket `now` is in. Only 24, 168 and 720.
pub fn span(hours: u32, now: i64) -> Result<Span> {
    let step = match hours {
        24 => HOUR,
        168 | 720 => DAY,
        _ => return Err(Error::invalid("the history is for 24 hours, 7 days or 30 days")),
    };
    let count = (i64::from(hours) * HOUR / step) as usize;
    let last = now.div_euclid(step) * step;
    Ok(Span { start: last - (count as i64 - 1) * step, step, count })
}

/// Reads a time the server wrote.
pub fn parse_time(text: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(text).ok().map(|t| t.timestamp())
}

fn number(v: &Value) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).filter(|n| n.is_finite()).unwrap_or(0.0)
}

/// The three charts from the rows, which may hold other metrics and rows outside the span.
/// A counter row adds to its bucket; a histogram row adds its sum and its count, and the
/// bucket's value is the one divided by the other (a bucket of several rows is thus a true
/// average). A bucket nothing falls into is 0, so a chart has no holes.
pub fn history(rows: &[Value], span: Span) -> DashboardHistory {
    let mut received = vec![0.0; span.count];
    let mut delivered = vec![0.0; span.count];
    let mut time_sum = vec![0.0; span.count];
    let mut time_count = vec![0.0; span.count];
    for row in rows {
        let Some(at) = row["timestamp"].as_str().and_then(parse_time) else { continue };
        let slot = (at - span.start).div_euclid(span.step);
        let Some(slot) = usize::try_from(slot).ok().filter(|s| *s < span.count) else { continue };
        match row["metric"].as_str() {
            Some(RECEIVED) if row["@type"].as_str() != Some("Histogram") => received[slot] += number(&row["count"]),
            Some(m) if SENT.contains(&m) && row["@type"].as_str() != Some("Histogram") => {
                delivered[slot] += number(&row["count"])
            }
            Some(DELIVERY_TIME) if row["@type"].as_str() == Some("Histogram") => {
                time_sum[slot] += number(&row["sum"]);
                time_count[slot] += number(&row["count"]);
            }
            _ => {}
        }
    }
    let points = |values: Vec<f64>| -> Vec<ChartPoint> {
        values
            .into_iter()
            .enumerate()
            .map(|(i, value)| ChartPoint { at: span.start + i as i64 * span.step, value })
            .collect()
    };
    let seconds: Vec<f64> =
        time_sum.iter().zip(&time_count).map(|(sum, n)| if *n > 0.0 { sum / n / 1000.0 } else { 0.0 }).collect();
    DashboardHistory { received: points(received), delivered: points(delivered), delivery_seconds: points(seconds) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn spans_are_hours_then_days() {
        let now = 1_700_000_000;
        let day = span(24, now).unwrap();
        assert_eq!((day.step, day.count), (3600, 24));
        assert_eq!(day.start + 23 * 3600, now.div_euclid(3600) * 3600);
        assert_eq!(span(168, now).unwrap().count, 7);
        assert_eq!(span(720, now).unwrap().count, 30);
        assert!(span(48, now).is_err());
        assert!(span(0, now).is_err());
    }

    #[test]
    fn buckets_add_and_average() {
        let s = Span { start: 0, step: 3600, count: 3 };
        let rows = vec![
            json!({"metric": RECEIVED, "@type": "Counter", "count": 4, "timestamp": "1970-01-01T00:00:00Z"}),
            json!({"metric": RECEIVED, "@type": "Counter", "count": 6, "timestamp": "1970-01-01T00:30:00Z"}),
            json!({"metric": DELIVERY_TIME, "@type": "Histogram", "count": 2, "sum": 3000, "timestamp": "1970-01-01T02:00:00Z"}),
            json!({"metric": RECEIVED, "@type": "Counter", "count": 9, "timestamp": "1969-12-31T23:00:00Z"}),
            json!({"metric": "other", "@type": "Counter", "count": 9, "timestamp": "1970-01-01T01:00:00Z"}),
        ];
        let h = history(&rows, s);
        assert_eq!(h.received.iter().map(|p| p.value).collect::<Vec<_>>(), vec![10.0, 0.0, 0.0]);
        assert_eq!(h.delivery_seconds[2].value, 1.5);
        assert_eq!(h.delivered.len(), 3);
        let sent = vec![
            json!({"metric": SENT[0], "@type": "Counter", "count": 2, "timestamp": "1970-01-01T01:00:00Z"}),
            json!({"metric": SENT[1], "@type": "Counter", "count": 1, "timestamp": "1970-01-01T01:10:00Z"}),
            json!({"metric": "delivery.delivered", "@type": "Counter", "count": 7, "timestamp": "1970-01-01T01:10:00Z"}),
        ];
        assert_eq!(history(&sent, s).delivered[1].value, 3.0);
        assert_eq!(h.received[1].at, 3600);
    }
}

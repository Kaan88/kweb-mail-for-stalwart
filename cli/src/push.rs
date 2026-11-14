//! `kweb push status`: where this account's push through Google stands. Shows the state and
//! what waits for a pass, never a key, a token or a code.

use kweb_core::ffi::{Core, WebPushState, WorkWaiting};

pub fn status(core: &Core) -> Result<(), String> {
    for line in lines(&core.web_push_state(), &core.work_waiting().map_err(|e| e.to_string())?, now()) {
        println!("{line}");
    }
    Ok(())
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// "3 days", "5 hours", "12 minutes", "40 seconds".
fn span(secs: i64) -> String {
    let secs = secs.abs();
    let (n, unit) = match secs {
        s if s >= 2 * 86_400 => (s / 86_400, "days"),
        s if s >= 2 * 3_600 => (s / 3_600, "hours"),
        s if s >= 2 * 60 => (s / 60, "minutes"),
        s => (s, "seconds"),
    };
    format!("{n} {unit}")
}

fn lines(state: &WebPushState, waiting: &WorkWaiting, now: i64) -> Vec<String> {
    let state = match state {
        WebPushState::Off => "Push through Google: off".to_string(),
        WebPushState::Verifying { since } => {
            format!("Push through Google: waiting for the server's verification (asked {} ago)", span(now - since))
        }
        WebPushState::Registered { expires, last_message_at } => {
            let last = match last_message_at {
                Some(at) => format!("last message {} ago", span(now - at)),
                None => "no message yet".to_string(),
            };
            format!("Push through Google: registered for another {} ({last})", span(expires - now))
        }
        WebPushState::KeyChanged => "Push through Google: the server's key changed; register again".to_string(),
        WebPushState::Gone => "Push through Google: the server no longer has the subscription".to_string(),
    };
    let work = match waiting {
        WorkWaiting::Nothing => "Waiting for a pass: nothing".to_string(),
        WorkWaiting::Due { in_ms: 0 } => "Waiting for a pass: due now".to_string(),
        WorkWaiting::Due { in_ms } => format!("Waiting for a pass: due in {}", span((*in_ms / 1000) as i64)),
    };
    vec![state, work]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_state_has_a_line_and_no_secret() {
        let l = |s: WebPushState, w: WorkWaiting| lines(&s, &w, 1_000_000).join(" | ");
        assert_eq!(
            l(WebPushState::Off, WorkWaiting::Nothing),
            "Push through Google: off | Waiting for a pass: nothing"
        );
        assert!(l(WebPushState::Verifying { since: 999_970 }, WorkWaiting::Nothing).contains("30 seconds ago"));
        let reg =
            WebPushState::Registered { expires: 1_000_000 + 5 * 86_400, last_message_at: Some(1_000_000 - 7_200) };
        assert!(l(reg, WorkWaiting::Due { in_ms: 0 })
            .contains("another 5 days (last message 2 hours ago) | Waiting for a pass: due now"));
        assert!(l(
            WebPushState::Registered { expires: 1_000_100, last_message_at: None },
            WorkWaiting::Due { in_ms: 90_000 }
        )
        .contains("no message yet) | Waiting for a pass: due in 90 seconds"));
        assert!(l(WebPushState::KeyChanged, WorkWaiting::Nothing).contains("key changed"));
        assert!(l(WebPushState::Gone, WorkWaiting::Nothing).contains("no longer has"));
    }
}

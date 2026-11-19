//! wash-fps `client/NewUiApi.ts`'s `requestFailure`: a failed lobby request
//! in words a player reads. The lobby's reply codes ("Leaderboard:
//! rate_limited") and the client's timeouts are not for the page. The rest
//! of `NewUiApi` (reading the boards) lives with each page here.

/// `requestFailure`: `message` is the request's own failure (a reply code
/// after its prefix, a timeout, the lobby gone); `what` names what was asked
/// for, as the sentence uses it ("the rankings").
#[must_use]
pub fn request_failure(message: &str, what: &str) -> String {
    let lower = message.to_lowercase();
    if message.ends_with("rate_limited") {
        return format!("Too many requests for {what}. Try again in a minute.");
    }
    if lower.contains("timed out") {
        return format!(
            "The lobby server did not answer in time. {} come back on the next try.",
            capitalize(what)
        );
    }
    if lower.contains("disconnected") || lower.contains("offline") {
        return format!("The lobby server is offline. {} come back with it.", capitalize(what));
    }
    format!("The lobby server could not read {what}.")
}

/// `capitalize`: the first character upper-cased.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

#[cfg(test)]
mod tests {
    use super::request_failure;

    #[test]
    fn a_failed_request_reads_as_the_browser_says_it() {
        assert_eq!(
            request_failure("Rankings timed out.", "the rankings"),
            "The lobby server did not answer in time. The rankings come back on the next try."
        );
        assert_eq!(
            request_failure("Leaderboard: rate_limited", "the rankings"),
            "Too many requests for the rankings. Try again in a minute."
        );
        assert_eq!(
            request_failure("Lobby disconnected", "the campaign boards"),
            "The lobby server is offline. The campaign boards come back with it."
        );
        assert_eq!(
            request_failure("Featured demos: bad_request", "featured picks"),
            "The lobby server could not read featured picks."
        );
    }
}

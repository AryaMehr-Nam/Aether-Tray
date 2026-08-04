use super::profile::ConnectionProfile;

/// Aether (v1.0.1) prompts for exactly these three settings, in this exact
/// order, regardless of which protocol is chosen — confirmed by manually
/// running the real binary end to end with both MASQUE and WireGuard
/// selected; WireGuard/gool ask nothing extra (no key/endpoint prompts).
///
/// The literal input line Aether blocks on ("Choose [1-3] (default 1): ")
/// is NOT distinguishing text — it repeats verbatim between the "Protocol:"
/// and "IP version to scan:" prompts. Rules therefore match on the *header*
/// line that precedes each menu block ("Protocol:", "Scan mode:", "IP
/// version to scan:"), which are each unique. See core/pty.rs's read loop
/// for how `header_matches` and `answer` are used together.
pub struct PromptRule {
    pub id: &'static str,
    pub header_matches: fn(&str) -> bool,
    pub answer: fn(&ConnectionProfile) -> String,
}

/// Matches on suffix, not exact equality: Aether sometimes prints a header
/// on the same raw line as the preceding info log (no `\n` in between, e.g.
/// `"...primary profile: balancedScan mode:"`), which an exact-equality
/// check would never match — silently starving that prompt of an answer
/// until Aether's own read times out and restarts the whole sequence from
/// Protocol (observed in production: an infinite Protocol/Scan-mode loop).
///
/// The `trim_end()` is load-bearing too: these are terminal-emitted headers
/// that can carry trailing whitespace, and without it a header arriving as
/// `"Protocol: "` reopens exactly the starvation path above.
pub static PROMPT_TABLE: &[PromptRule] = &[
    PromptRule {
        id: "protocol",
        header_matches: |l| l.trim_end().ends_with("Protocol:"),
        answer: |p| p.protocol.as_menu_choice().to_string(),
    },
    PromptRule {
        id: "scan_mode",
        header_matches: |l| l.trim_end().ends_with("Scan mode:"),
        answer: |p| p.scan_mode.as_menu_choice().to_string(),
    },
    PromptRule {
        id: "ip_version",
        header_matches: |l| l.trim_end().ends_with("IP version to scan:"),
        answer: |p| p.ip_version.as_menu_choice().to_string(),
    },
    // Aether 1.2.0's MASQUE-only transport menu. Normally suppressed by the
    // AETHER_MASQUE_HTTP2 env var set at spawn (pty.rs) — fallback only.
    // Suffix match can't collide with the "[+] MASQUE transport: HTTP/…"
    // info-log line: that one continues past the colon.
    PromptRule {
        id: "masque_transport",
        header_matches: |l| l.trim_end().ends_with("MASQUE transport:"),
        answer: |p| if p.masque_http2 { "2" } else { "1" }.to_string(),
    },
];

/// True for a line that looks like Aether blocking on stdin for a menu
/// choice (real wording: "Choose [1-3] (default 1): ", "Choose [1-4]
/// (default 2): ", etc. — always ends with a colon, no trailing newline
/// since it's waiting for input right after printing this).
///
/// Deliberately narrow. Widening this to match any partial *containing*
/// "choose"/"select"/"option" breaks the bare-header guard in pty.rs's read
/// loop: that guard can only reject a partial which *is* a recognized
/// header, so a still-in-flight menu block ("Protocol:\n 1) auto\nChoose
/// [1-3]") would be answered early and the next menu would be double-fed.
pub fn looks_like_choice_prompt(partial_line: &str) -> bool {
    partial_line.trim_end().ends_with(':')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::profile::{IpVersion, Protocol, ScanMode};

    fn rule(id: &str) -> &'static PromptRule {
        PROMPT_TABLE
            .iter()
            .find(|r| r.id == id)
            .expect("no such rule")
    }

    #[test]
    fn headers_match_by_suffix_not_equality() {
        // The production case: a header glued onto the preceding info log.
        assert!((rule("scan_mode").header_matches)(
            "[+] using primary profile: balancedScan mode:"
        ));
        assert!((rule("protocol").header_matches)("Protocol:"));
    }

    #[test]
    fn headers_tolerate_trailing_whitespace() {
        assert!((rule("protocol").header_matches)("Protocol: "));
        assert!((rule("scan_mode").header_matches)("Scan mode:\t"));
    }

    #[test]
    fn ip_version_matches_aethers_real_header() {
        // Aether prints "IP version to scan:", not "IP version:". Shortening
        // this makes the rule unmatchable under suffix semantics.
        assert!((rule("ip_version").header_matches)("IP version to scan:"));
        assert!(!(rule("ip_version").header_matches)("IP version:"));
    }

    #[test]
    fn masque_transport_header_does_not_match_its_info_log() {
        assert!((rule("masque_transport").header_matches)(
            "MASQUE transport:"
        ));
        assert!(!(rule("masque_transport").header_matches)(
            "[+] MASQUE transport: HTTP/3"
        ));
    }

    #[test]
    fn answers_match_aethers_menu_numbering() {
        // Aether's protocol menu is 1) auto/masque 2) wireguard 3) gool —
        // auto and masque share slot 1.
        let with_protocol = |protocol| ConnectionProfile {
            protocol,
            ..Default::default()
        };
        assert_eq!(
            (rule("protocol").answer)(&with_protocol(Protocol::Auto)),
            "1"
        );
        assert_eq!(
            (rule("protocol").answer)(&with_protocol(Protocol::Masque)),
            "1"
        );
        assert_eq!(
            (rule("protocol").answer)(&with_protocol(Protocol::Wireguard)),
            "2"
        );
        assert_eq!(
            (rule("protocol").answer)(&with_protocol(Protocol::Gool)),
            "3"
        );

        let p = ConnectionProfile {
            scan_mode: ScanMode::Ironclad,
            ip_version: IpVersion::Both,
            ..Default::default()
        };
        assert_eq!((rule("scan_mode").answer)(&p), "5");
        assert_eq!((rule("ip_version").answer)(&p), "3");
    }

    #[test]
    fn choice_prompt_detects_trailing_colon_only() {
        assert!(looks_like_choice_prompt("Choose [1-3] (default 1): "));
        assert!(looks_like_choice_prompt("Choose [1-4] (default 2):"));
        assert!(!looks_like_choice_prompt("scanning for routes"));
    }

    #[test]
    fn choice_prompt_does_not_fire_on_an_in_flight_menu_block() {
        // Guards the double-feed bug: an unterminated menu block mentioning
        // "Choose" must not be treated as a blocking prompt until the
        // trailing "…: " actually arrives.
        assert!(!looks_like_choice_prompt(
            "Protocol:\n  1) auto\n  2) masque\nChoose [1-3]"
        ));
    }
}

//! Connection-info lookup: asks a public endpoint what IP the tunnel is
//! exiting through, and where that is.
//!
//! This is a tray-app nicety, not part of the ported core logic. The request
//! goes *through* the local SOCKS5 proxy (`socks5h://`, so DNS also resolves
//! inside the tunnel), which is the whole point: the answer describes the
//! tunneled connection, not the machine's own uplink. It is fire-and-forget —
//! a failure must never disturb the connection, so the supervisor records a
//! `None` and the UI simply omits the section.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;

/// The endpoint that describes the caller's exit IP. Plain `http://` would
/// avoid TLS, but the response is only trustworthy over HTTPS when the whole
/// point of the request is that a hostile network might be in the path.
const GEO_URL: &str = "https://ifconfig.co/json";

/// Every field beyond `ip` is optional: the endpoint can legitimately omit
/// some values (e.g. no region for some IPs), and a partial answer is still
/// useful. `#[serde(default)]` on each makes a future addition by the server
/// (or a value it chooses to omit) degrade gracefully instead of failing the
/// whole parse.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GeoInfo {
    pub ip: String,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub country_iso: Option<String>,
    #[serde(default)]
    pub region_name: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
    #[serde(default)]
    pub time_zone: Option<String>,
    #[serde(default)]
    pub asn: Option<String>,
    #[serde(default)]
    pub asn_org: Option<String>,
}

/// The outcome of the connection-info lookup for the *current* tunnel. The
/// menu needs to distinguish "still in flight" from "failed" so it does not
/// say "resolving…" forever after a failure.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GeoState {
    pub info: Option<GeoInfo>,
    /// True once the lookup for this tunnel has finished (success or
    /// failure); false while it is still running.
    pub done: bool,
}

impl GeoInfo {
    /// "City, Region, Country" with whatever parts exist, e.g. "Tehran, Iran".
    pub fn location(&self) -> Option<String> {
        let mut parts = Vec::with_capacity(3);
        if let Some(city) = self.city.as_deref().filter(|c| !c.trim().is_empty()) {
            parts.push(city.trim());
        }
        if let Some(region) = self.region_name.as_deref().filter(|r| !r.trim().is_empty()) {
            parts.push(region.trim());
        }
        if let Some(country) = self.country.as_deref().filter(|c| !c.trim().is_empty()) {
            parts.push(country.trim());
        }
        // Some providers report the city and region with the same name
        // ("Tehran, Tehran, Iran"); a repeated part reads as a mistake, so
        // collapse consecutive duplicates.
        parts.dedup();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    }

    /// "Cloudflare, Inc. (AS13335)", or just the org / asn alone.
    pub fn isp(&self) -> Option<String> {
        match (
            self.asn_org.as_deref().map(str::trim),
            self.asn.as_deref().map(str::trim),
        ) {
            (Some(org), Some(asn)) if !org.is_empty() && !asn.is_empty() => {
                Some(format!("{org} ({asn})"))
            }
            (Some(org), _) if !org.is_empty() => Some(org.to_string()),
            (_, Some(asn)) if !asn.is_empty() => Some(asn.to_string()),
            _ => None,
        }
    }

    /// One-line form for `status` and headless output.
    pub fn summary(&self) -> String {
        match (self.location(), self.isp()) {
            (Some(loc), Some(isp)) => format!("{} · {} · {}", self.ip, loc, isp),
            (Some(loc), None) => format!("{} · {}", self.ip, loc),
            (None, Some(isp)) => format!("{} · {}", self.ip, isp),
            (None, None) => self.ip.clone(),
        }
    }
}

/// Resolves the IP/location of the tunnel by asking the endpoint through the
/// local SOCKS5 proxy. `listen` is the proxy's *listen* address — when it is
/// `0.0.0.0` (LAN sharing) the request still exits via the loopback
/// interface, exactly as `status::port_is_live` probes.
pub fn lookup(listen: &SocketAddr) -> Result<GeoInfo> {
    // `socks5://`, not `socks5h://`: ureq's Proxy::new only knows
    // socks4/socks4a/socks5. It makes no difference for HTTPS — the hostname
    // travels inside the SOCKS CONNECT request, so the *proxy* still resolves
    // ifconfig.co; remote DNS only matters for plain-HTTP targets.
    let proxy = ureq::Proxy::new(format!("socks5://{}", super::status::probe_addr(listen)))
        .map_err(|e| anyhow!("bad SOCKS proxy address: {e}"))?;
    let agent = ureq::AgentBuilder::new()
        .proxy(proxy)
        // The endpoint should answer in a second; the budget below is for a
        // degraded tunnel. Whatever happens, this must return — it runs on a
        // detached thread but still must not hang the process on quit.
        .timeout_connect(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build();

    let response = agent
        .get(GEO_URL)
        .call()
        .map_err(|e| anyhow!("connection-info request failed: {e}"))?;
    let body = response
        .into_string()
        .map_err(|e| anyhow!("reading connection-info response: {e}"))?;
    serde_json::from_str(&body).map_err(|e| anyhow!("parsing connection-info response: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact payload from the project's reference `curl ifconfig.co/json`.
    const SAMPLE: &str = r#"{
      "ip": "104.28.246.164",
      "ip_decimal": 1746728612,
      "country": "Iran",
      "country_iso": "IR",
      "country_eu": false,
      "region_name": "Tehran",
      "region_code": "23",
      "city": "Tehran",
      "latitude": 35.6824,
      "longitude": 51.4158,
      "time_zone": "Asia/Tehran",
      "asn": "AS13335",
      "asn_org": "CLOUDFLARENET",
      "user_agent": {
        "product": "Mozilla",
        "version": "5.0"
      }
    }"#;

    #[test]
    fn parses_the_reference_payload() {
        let info: GeoInfo = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(info.ip, "104.28.246.164");
        assert_eq!(info.country.as_deref(), Some("Iran"));
        assert_eq!(info.country_iso.as_deref(), Some("IR"));
        assert_eq!(info.region_name.as_deref(), Some("Tehran"));
        assert_eq!(info.city.as_deref(), Some("Tehran"));
        assert_eq!(info.time_zone.as_deref(), Some("Asia/Tehran"));
        assert_eq!(info.asn.as_deref(), Some("AS13335"));
        assert_eq!(info.asn_org.as_deref(), Some("CLOUDFLARENET"));
        // Fields we do not model (ip_decimal, country_eu, user_agent, …) are
        // ignored rather than fatal.
    }

    #[test]
    fn a_sparse_answer_still_parses() {
        let info: GeoInfo = serde_json::from_str(r#"{"ip":"1.2.3.4"}"#).unwrap();
        assert_eq!(info.ip, "1.2.3.4");
        assert_eq!(info.location(), None);
        assert_eq!(info.isp(), None);
        assert_eq!(info.summary(), "1.2.3.4");
    }

    #[test]
    fn location_and_isp_format_human_readably() {
        let info: GeoInfo = serde_json::from_str(SAMPLE).unwrap();
        // City and region both read "Tehran" — the duplicate must collapse.
        assert_eq!(info.location().as_deref(), Some("Tehran, Iran"));
        assert_eq!(
            info.summary(),
            "104.28.246.164 · Tehran, Iran · CLOUDFLARENET (AS13335)"
        );
    }

    #[test]
    fn duplicate_city_and_region_collapse() {
        let info: GeoInfo = serde_json::from_str(
            r#"{"ip":"1.2.3.4","city":"Tehran","region_name":"Tehran","country":"Iran"}"#,
        )
        .unwrap();
        assert_eq!(info.location().as_deref(), Some("Tehran, Iran"));
        // A genuine city/region difference is kept.
        let info: GeoInfo = serde_json::from_str(
            r#"{"ip":"1.2.3.4","city":"San Francisco","region_name":"California","country":"United States"}"#,
        )
        .unwrap();
        assert_eq!(
            info.location().as_deref(),
            Some("San Francisco, California, United States")
        );
    }

    /// Regression guard for the "bad SOCKS proxy address: Malformed proxy"
    /// failure: ureq's `Proxy::new` must accept the `socks5://` URL. A tiny
    /// loopback SOCKS5 server completes the greeting + CONNECT handshake, so
    /// the lookup proceeds past proxy parsing; the subsequent TLS exchange
    /// against the mock cannot complete, which is the expected end of the
    /// test. What must NOT happen is a failure *before* any network I/O.
    #[test]
    fn proxy_address_is_accepted_and_handshake_happens() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handshake_done = Arc::new(AtomicBool::new(false));
        let handshake_flag = Arc::clone(&handshake_done);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Greeting: VER CMD RSV — reply "no auth required".
            let mut greeting = [0u8; 3];
            stream.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting[0], 0x05);
            stream.write_all(&[0x05, 0x00]).unwrap();
            // CONNECT request: VER CMD RSV ATYP, then the address.
            let mut head = [0u8; 4];
            stream.read_exact(&mut head).unwrap();
            assert_eq!(head[0], 0x05);
            assert_eq!(head[1], 0x01);
            match head[3] {
                0x01 => {
                    let mut rest = [0u8; 6];
                    stream.read_exact(&mut rest).unwrap();
                }
                0x03 => {
                    let mut len = [0u8; 1];
                    stream.read_exact(&mut len).unwrap();
                    let mut name = vec![0u8; len[0] as usize];
                    stream.read_exact(&mut name).unwrap();
                    let mut port = [0u8; 2];
                    stream.read_exact(&mut port).unwrap();
                }
                0x04 => {
                    let mut rest = [0u8; 18];
                    stream.read_exact(&mut rest).unwrap();
                }
                other => panic!("unexpected ATYP {other}"),
            }
            // Accept the tunnel; ureq now starts TLS against this mock,
            // which cannot complete — that failure is the expected outcome.
            stream
                .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .unwrap();
            handshake_flag.store(true, Ordering::SeqCst);
            let _ = stream.read(&mut [0u8; 4096]);
        });

        // The lookup must fail *after* the tunnel was established, not with a
        // proxy-parsing error — that is the regression being guarded.
        let err = lookup(&addr).unwrap_err().to_string();
        server.join().unwrap();
        assert!(
            handshake_done.load(Ordering::SeqCst),
            "the SOCKS handshake never happened: {err}"
        );
        assert!(!err.contains("bad SOCKS proxy address"), "{err}");
    }

    #[test]
    fn missing_parts_are_omitted_not_padded() {
        let info: GeoInfo =
            serde_json::from_str(r#"{"ip":"9.9.9.9","country":"Switzerland","asn_org":"QUAD9"}"#)
                .unwrap();
        assert_eq!(info.location().as_deref(), Some("Switzerland"));
        assert_eq!(info.isp().as_deref(), Some("QUAD9"));
        assert_eq!(info.summary(), "9.9.9.9 · Switzerland · QUAD9");
    }
}

//! CLI ↔ tray IPC over a local socket.
//!
//! One newline-delimited JSON request per connection, one JSON response back.
//! `interprocess` supplies the transport, so this is a Unix domain socket on
//! Unix and a named pipe on Windows — the previous `std::os::unix::net`
//! version could not compile for Windows at all.

pub mod client;
pub mod server;

use serde::{Deserialize, Serialize};

/// Namespaced rather than a filesystem path: on Windows this becomes
/// `\\.\pipe\aether-tray.sock` and on Linux an abstract socket, neither of
/// which leaves a stale file behind after a crash.
pub const SOCKET_NAME: &str = "aether-tray.sock";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "request", rename_all = "snake_case")]
pub enum Request {
    Connect {
        #[serde(default)]
        overrides: Overrides,
    },
    Disconnect,
    Status,
}

/// Per-invocation profile overrides. `None` means "leave the saved value".
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Overrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_address: Option<String>,
}

impl Overrides {
    /// Applies the overrides to a profile, rejecting unparseable values
    /// rather than silently keeping the old one — a typo in `--protocol`
    /// must not connect with the wrong protocol.
    pub fn apply(
        &self,
        profile: &mut crate::core::profile::ConnectionProfile,
    ) -> Result<(), String> {
        if let Some(v) = &self.protocol {
            profile.protocol = v.parse()?;
        }
        if let Some(v) = &self.scan_mode {
            profile.scan_mode = v.parse()?;
        }
        if let Some(v) = &self.ip_version {
            profile.ip_version = v.parse()?;
        }
        if let Some(v) = &self.bind_address {
            if v.parse::<std::net::SocketAddr>().is_err() {
                return Err(format!("invalid bind address {v:?}; expected host:port"));
            }
            profile.bind_address = v.clone();
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum Response {
    Ok { message: String },
    Error { message: String },
    Status { state: crate::core::ConnectionState },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::profile::{ConnectionProfile, IpVersion, Protocol};

    #[test]
    fn requests_round_trip_as_json() {
        for request in [
            Request::Disconnect,
            Request::Status,
            Request::Connect {
                overrides: Overrides::default(),
            },
            Request::Connect {
                overrides: Overrides {
                    protocol: Some("masque".into()),
                    scan_mode: Some("turbo".into()),
                    ip_version: None,
                    bind_address: Some("0.0.0.0:1819".into()),
                },
            },
        ] {
            let encoded = serde_json::to_string(&request).unwrap();
            assert!(!encoded.contains('\n'), "framing needs single-line JSON");
            let decoded: Request = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, request);
        }
    }

    #[test]
    fn responses_round_trip_as_json() {
        let response = Response::Status {
            state: crate::core::ConnectionState::Connected {
                socks_addr: "127.0.0.1:1819".into(),
                connected_at_ms: 12,
            },
        };
        let encoded = serde_json::to_string(&response).unwrap();
        assert!(!encoded.contains('\n'));
        assert_eq!(
            serde_json::from_str::<Response>(&encoded).unwrap(),
            response
        );
    }

    #[test]
    fn overrides_apply_to_a_profile() {
        let mut p = ConnectionProfile::default();
        Overrides {
            protocol: Some("masque".into()),
            ip_version: Some("both".into()),
            ..Default::default()
        }
        .apply(&mut p)
        .unwrap();
        assert_eq!(p.protocol, Protocol::Masque);
        assert_eq!(p.ip_version, IpVersion::Both);
        // Untouched fields keep their saved value.
        assert_eq!(p.bind_address, "127.0.0.1:1819");
    }

    #[test]
    fn a_bad_override_is_rejected_not_ignored() {
        let mut p = ConnectionProfile::default();
        let err = Overrides {
            protocol: Some("quantum".into()),
            ..Default::default()
        }
        .apply(&mut p)
        .unwrap_err();
        assert!(err.contains("quantum"), "{err}");
        // The profile must be left alone rather than half-applied.
        assert_eq!(p.protocol, Protocol::Auto);

        let err = Overrides {
            bind_address: Some("not-an-address".into()),
            ..Default::default()
        }
        .apply(&mut p)
        .unwrap_err();
        assert!(err.contains("host:port"), "{err}");
    }

    #[test]
    fn defaults_leave_the_saved_profile_untouched() {
        let mut p = ConnectionProfile::default();
        let before = p.clone();
        Overrides::default().apply(&mut p).unwrap();
        assert_eq!(p, before);
    }
}

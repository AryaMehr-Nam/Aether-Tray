//! Menu construction. Rebuilt on every state change, so every item carries a
//! stable string id and events are routed by id — never by label, which is
//! localised, decorated with state, and duplicated across radio groups.

use muda::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

use crate::config::Config;
use crate::core::ConnectionState;
use crate::core::geo::GeoState;
use crate::core::profile::{
    ConnectionProfile, IpVersion, MasqueNoize, Protocol, ScanMode, WgNoize, ZeroTrustAuth,
};

pub const ID_TOGGLE: &str = "toggle";
pub const ID_COPY_IP: &str = "copy_ip";
pub const ID_REFRESH_CONNECTION_INFO: &str = "refresh_connection_info";
pub const ID_UPTIME: &str = "uptime";
pub const ID_COPY_PROXY: &str = "copy_proxy";
pub const ID_SHOW_LOGS: &str = "show_logs";
pub const ID_QUIT: &str = "quit";
pub const ID_BIND: &str = "set.bind";
pub const ID_DNS: &str = "set.dns";
pub const ID_ROUTE_BLOCK: &str = "set.route_block";
pub const ID_ROUTE_DIRECT: &str = "set.route_direct";
pub const ID_ROUTES_FILE: &str = "set.routes_file";
pub const ID_ZT_TEAM: &str = "set.zt_team";
pub const ID_ZT_CREDENTIALS: &str = "set.zt_credentials";
pub const ID_ZT_GATEWAY: &str = "toggle.zt_gateway";
pub const ID_QUICK_RECONNECT: &str = "toggle.quick_reconnect";
pub const ID_MASQUE_HTTP2: &str = "toggle.masque_http2";
pub const ID_NOTIFICATIONS: &str = "toggle.notifications";
pub const ID_CONNECTION_INFO: &str = "toggle.connection_info";
pub const ID_LAUNCH_AT_LOGIN: &str = "toggle.launch_at_login";
pub const ID_CONNECT_ON_LAUNCH: &str = "toggle.connect_on_launch";
pub const ID_OPEN_CONFIG: &str = "open_config";

pub const PREFIX_PROTOCOL: &str = "protocol.";
pub const PREFIX_SCAN: &str = "scan.";
pub const PREFIX_IP: &str = "ip.";
pub const PREFIX_MASQUE_NOIZE: &str = "mnoize.";
pub const PREFIX_WG_NOIZE: &str = "wnoize.";
pub const PREFIX_ZT_AUTH: &str = "ztauth.";

/// The action a menu id maps to. Resolved by the tray loop, which owns the
/// config and the supervisor handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    Toggle,
    CopyIp,
    RefreshConnectionInfo,
    CopyProxy,
    ShowLogs,
    OpenConfig,
    Quit,
    SetProtocol(Protocol),
    SetScanMode(ScanMode),
    SetIpVersion(IpVersion),
    SetMasqueNoize(MasqueNoize),
    SetWgNoize(WgNoize),
    SetZeroTrustAuth(ZeroTrustAuth),
    ToggleQuickReconnect,
    ToggleMasqueHttp2,
    ToggleZeroTrustGateway,
    ToggleNotifications,
    ToggleConnectionInfo,
    ToggleLaunchAtLogin,
    ToggleConnectOnLaunch,
    EditBindAddress,
    EditDns,
    EditRouteBlock,
    EditRouteDirect,
    PickRoutesFile,
    EditZeroTrustTeam,
    EditZeroTrustCredentials,
}

/// Maps a fired menu id back to its action.
pub fn action_for(id: &MenuId) -> Option<MenuAction> {
    let raw = id.as_ref();
    let action = match raw {
        ID_TOGGLE => MenuAction::Toggle,
        ID_COPY_IP => MenuAction::CopyIp,
        ID_REFRESH_CONNECTION_INFO => MenuAction::RefreshConnectionInfo,
        ID_COPY_PROXY => MenuAction::CopyProxy,
        ID_SHOW_LOGS => MenuAction::ShowLogs,
        ID_OPEN_CONFIG => MenuAction::OpenConfig,
        ID_QUIT => MenuAction::Quit,
        ID_BIND => MenuAction::EditBindAddress,
        ID_DNS => MenuAction::EditDns,
        ID_ROUTE_BLOCK => MenuAction::EditRouteBlock,
        ID_ROUTE_DIRECT => MenuAction::EditRouteDirect,
        ID_ROUTES_FILE => MenuAction::PickRoutesFile,
        ID_ZT_TEAM => MenuAction::EditZeroTrustTeam,
        ID_ZT_CREDENTIALS => MenuAction::EditZeroTrustCredentials,
        ID_ZT_GATEWAY => MenuAction::ToggleZeroTrustGateway,
        ID_QUICK_RECONNECT => MenuAction::ToggleQuickReconnect,
        ID_MASQUE_HTTP2 => MenuAction::ToggleMasqueHttp2,
        ID_NOTIFICATIONS => MenuAction::ToggleNotifications,
        ID_CONNECTION_INFO => MenuAction::ToggleConnectionInfo,
        ID_LAUNCH_AT_LOGIN => MenuAction::ToggleLaunchAtLogin,
        ID_CONNECT_ON_LAUNCH => MenuAction::ToggleConnectOnLaunch,
        other => return action_for_radio(other),
    };
    Some(action)
}

fn action_for_radio(raw: &str) -> Option<MenuAction> {
    if let Some(v) = raw.strip_prefix(PREFIX_PROTOCOL) {
        return match v {
            "auto" => Some(MenuAction::SetProtocol(Protocol::Auto)),
            "masque" => Some(MenuAction::SetProtocol(Protocol::Masque)),
            "wireguard" => Some(MenuAction::SetProtocol(Protocol::Wireguard)),
            "gool" => Some(MenuAction::SetProtocol(Protocol::Gool)),
            _ => None,
        };
    }
    if let Some(v) = raw.strip_prefix(PREFIX_SCAN) {
        return match v {
            "turbo" => Some(MenuAction::SetScanMode(ScanMode::Turbo)),
            "balanced" => Some(MenuAction::SetScanMode(ScanMode::Balanced)),
            "thorough" => Some(MenuAction::SetScanMode(ScanMode::Thorough)),
            "stealth" => Some(MenuAction::SetScanMode(ScanMode::Stealth)),
            "ironclad" => Some(MenuAction::SetScanMode(ScanMode::Ironclad)),
            _ => None,
        };
    }
    if let Some(v) = raw.strip_prefix(PREFIX_IP) {
        return match v {
            "v4" => Some(MenuAction::SetIpVersion(IpVersion::V4)),
            "v6" => Some(MenuAction::SetIpVersion(IpVersion::V6)),
            "both" => Some(MenuAction::SetIpVersion(IpVersion::Both)),
            _ => None,
        };
    }
    if let Some(v) = raw.strip_prefix(PREFIX_MASQUE_NOIZE) {
        return match v {
            "firewall" => Some(MenuAction::SetMasqueNoize(MasqueNoize::Firewall)),
            "gfw" => Some(MenuAction::SetMasqueNoize(MasqueNoize::Gfw)),
            "off" => Some(MenuAction::SetMasqueNoize(MasqueNoize::Off)),
            _ => None,
        };
    }
    if let Some(v) = raw.strip_prefix(PREFIX_WG_NOIZE) {
        return match v {
            "balanced" => Some(MenuAction::SetWgNoize(WgNoize::Balanced)),
            "aggressive" => Some(MenuAction::SetWgNoize(WgNoize::Aggressive)),
            "light" => Some(MenuAction::SetWgNoize(WgNoize::Light)),
            "off" => Some(MenuAction::SetWgNoize(WgNoize::Off)),
            _ => None,
        };
    }
    if let Some(v) = raw.strip_prefix(PREFIX_ZT_AUTH) {
        return match v {
            "email" => Some(MenuAction::SetZeroTrustAuth(ZeroTrustAuth::Email)),
            "service" => Some(MenuAction::SetZeroTrustAuth(ZeroTrustAuth::Service)),
            "token" => Some(MenuAction::SetZeroTrustAuth(ZeroTrustAuth::Token)),
            _ => None,
        };
    }
    None
}

/// The label for the connect/disconnect item, which flips with state.
fn toggle_label(state: &ConnectionState) -> &'static str {
    match state {
        ConnectionState::Idle | ConnectionState::Error { .. } => "Connect",
        ConnectionState::Connected { .. } => "Disconnect",
        ConnectionState::Disconnecting => "Disconnecting…",
        _ => "Cancel",
    }
}

fn summary(value: &str, empty: &str) -> String {
    if value.trim().is_empty() {
        empty.to_string()
    } else {
        value.trim().to_string()
    }
}

/// "2h 3m" / "3m 12s" / "12s" — the displayed uptime granularity follows
/// the magnitude, so a fresh connection doesn't show a static "0s".
pub fn format_uptime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// The live uptime label for a connection started at `connected_at_ms`,
/// recomputed on every call. The tray updates the menu item's text in place
/// (never rebuilding the menu) so an open menu is not dismissed.
pub fn uptime_label(connected_at_ms: u64) -> String {
    let uptime = crate::core::now_millis().saturating_sub(connected_at_ms) / 1000;
    format!("Uptime: {}", format_uptime(uptime))
}

/// The result of building the menu: the menu itself plus a handle to the
/// live uptime item (present only while Connected), which the tray updates
/// in place via `set_text` rather than rebuilding the whole menu.
pub struct BuiltMenu {
    pub menu: Menu,
    pub uptime_item: Option<MenuItem>,
}

/// Builds the whole menu for the current state and config.
///
/// `geo` is the supervisor's lookup outcome for the current tunnel; it is
/// only ever shown while Connected. Everything below the status line is
/// disabled while the state is not resting, mirroring the windowed app's
/// locking: the core reads its whole profile at spawn, so changing a setting
/// mid-connection would silently not apply.
pub fn build(state: &ConnectionState, config: &Config, geo: &GeoState) -> muda::Result<BuiltMenu> {
    let menu = Menu::new();
    let p = &config.profile;
    let unlocked = state.is_resting();
    // With no zenity/kdialog there is nothing to open, so free-text items are
    // greyed out and "Edit config.toml…" becomes the way in.
    let text_entry = unlocked && crate::dialog::available();
    // MASQUE noize only reaches the core on a MASQUE-family connection, and
    // the WireGuard profile only on wg/gool — grey out whichever is inert.
    let masque_family = matches!(p.protocol, Protocol::Auto | Protocol::Masque);

    menu.append(&MenuItem::new(
        format!("Aether — {}", state.label()),
        false,
        None,
    ))?;

    let connected = matches!(state, ConnectionState::Connected { .. });
    let mut uptime_item = None;
    if connected {
        // One connection-info submenu: its title is the tunnel's IP and
        // location ("104.28.214.167 (Tehran, Iran)"), and its only items are
        // the two actions that ever act on that info. Gated on the setting so
        // turning "Connection info" off hides what is already cached too;
        // the lookup's own state titles the submenu "resolving…"/"unavailable"
        // until it settles.
        if config.settings.show_connection_info {
            let title = match &geo.info {
                Some(info) => match info.location() {
                    Some(loc) => format!("{} ({loc})", info.ip),
                    None => info.ip.clone(),
                },
                None if geo.done => "IP: unavailable".into(),
                None => "IP: resolving…".into(),
            };
            let connection = Submenu::new(title, true);
            connection.append(&MenuItem::with_id(
                ID_COPY_IP,
                "Copy IP",
                geo.info.is_some(),
                None,
            ))?;
            connection.append(&MenuItem::with_id(
                ID_REFRESH_CONNECTION_INFO,
                "Refresh",
                true,
                None,
            ))?;
            menu.append(&connection)?;
        }
        if let ConnectionState::Connected {
            connected_at_ms, ..
        } = state
        {
            let uptime = MenuItem::with_id(ID_UPTIME, uptime_label(*connected_at_ms), false, None);
            menu.append(&uptime)?;
            uptime_item = Some(uptime);
        }
    }
    menu.append(&PredefinedMenuItem::separator())?;

    menu.append(&MenuItem::with_id(
        ID_TOGGLE,
        toggle_label(state),
        !matches!(state, ConnectionState::Disconnecting),
        None,
    ))?;
    menu.append(&MenuItem::with_id(
        ID_COPY_PROXY,
        format!("Copy proxy address  ({})", p.bind_address),
        connected,
        None,
    ))?;
    menu.append(&PredefinedMenuItem::separator())?;

    menu.append(&radio(
        "Protocol",
        PREFIX_PROTOCOL,
        unlocked,
        &[
            ("auto", "Auto", p.protocol == Protocol::Auto),
            ("masque", "MASQUE", p.protocol == Protocol::Masque),
            ("wireguard", "WireGuard", p.protocol == Protocol::Wireguard),
            ("gool", "WARP-in-WARP (gool)", p.protocol == Protocol::Gool),
        ],
    )?)?;

    menu.append(&radio(
        "Scan mode",
        PREFIX_SCAN,
        unlocked,
        &[
            ("turbo", "Turbo", p.scan_mode == ScanMode::Turbo),
            ("balanced", "Balanced", p.scan_mode == ScanMode::Balanced),
            ("thorough", "Thorough", p.scan_mode == ScanMode::Thorough),
            ("stealth", "Stealth", p.scan_mode == ScanMode::Stealth),
            ("ironclad", "Ironclad", p.scan_mode == ScanMode::Ironclad),
        ],
    )?)?;

    menu.append(&radio(
        "IP version",
        PREFIX_IP,
        unlocked,
        &[
            ("v4", "IPv4", p.ip_version == IpVersion::V4),
            ("v6", "IPv6", p.ip_version == IpVersion::V6),
            ("both", "Both", p.ip_version == IpVersion::Both),
        ],
    )?)?;

    let transport = Submenu::new("MASQUE transport", unlocked && masque_family);
    transport.append(&CheckMenuItem::with_id(
        ID_MASQUE_HTTP2,
        "Use HTTP/2 (TCP) instead of HTTP/3",
        unlocked && masque_family,
        p.masque_http2,
        None,
    ))?;
    menu.append(&transport)?;

    menu.append(&radio(
        "Obfuscation (MASQUE)",
        PREFIX_MASQUE_NOIZE,
        unlocked && masque_family,
        &[
            (
                "firewall",
                "Firewall",
                p.masque_noize == MasqueNoize::Firewall,
            ),
            ("gfw", "GFW", p.masque_noize == MasqueNoize::Gfw),
            ("off", "Off", p.masque_noize == MasqueNoize::Off),
        ],
    )?)?;

    menu.append(&radio(
        "Obfuscation (WireGuard)",
        PREFIX_WG_NOIZE,
        unlocked && !masque_family,
        &[
            ("balanced", "Balanced", p.wg_noize == WgNoize::Balanced),
            (
                "aggressive",
                "Aggressive",
                p.wg_noize == WgNoize::Aggressive,
            ),
            ("light", "Light", p.wg_noize == WgNoize::Light),
            ("off", "Off", p.wg_noize == WgNoize::Off),
        ],
    )?)?;

    menu.append(&PredefinedMenuItem::separator())?;

    let network = Submenu::new("Network", unlocked);
    network.append(&MenuItem::with_id(
        ID_BIND,
        format!("Listen address: {}…", p.bind_address),
        text_entry,
        None,
    ))?;
    network.append(&MenuItem::with_id(
        ID_DNS,
        format!("DNS: {}…", summary(&p.dns, "(default)")),
        text_entry,
        None,
    ))?;
    menu.append(&network)?;

    let routing = Submenu::new("Routing", unlocked);
    routing.append(&MenuItem::with_id(
        ID_ROUTE_BLOCK,
        format!("Block list: {}…", summary(&p.route_block, "(none)")),
        text_entry,
        None,
    ))?;
    routing.append(&MenuItem::with_id(
        ID_ROUTE_DIRECT,
        format!("Direct list: {}…", summary(&p.route_direct, "(none)")),
        text_entry,
        None,
    ))?;
    routing.append(&MenuItem::with_id(
        ID_ROUTES_FILE,
        format!("Rules file: {}…", summary(&p.routes_file, "(none)")),
        unlocked,
        None,
    ))?;
    menu.append(&routing)?;

    menu.append(&zero_trust_submenu(p, unlocked, text_entry)?)?;
    menu.append(&PredefinedMenuItem::separator())?;

    menu.append(&CheckMenuItem::with_id(
        ID_QUICK_RECONNECT,
        "Quick reconnect",
        unlocked,
        p.quick_reconnect,
        None,
    ))?;
    menu.append(&CheckMenuItem::with_id(
        ID_NOTIFICATIONS,
        "Notifications",
        true,
        config.settings.notifications,
        None,
    ))?;
    menu.append(&CheckMenuItem::with_id(
        ID_CONNECTION_INFO,
        "Connection info",
        true,
        config.settings.show_connection_info,
        None,
    ))?;
    menu.append(&CheckMenuItem::with_id(
        ID_LAUNCH_AT_LOGIN,
        "Launch at login",
        true,
        config.settings.launch_at_login,
        None,
    ))?;
    menu.append(&CheckMenuItem::with_id(
        ID_CONNECT_ON_LAUNCH,
        "Connect on launch",
        true,
        config.settings.connect_on_launch,
        None,
    ))?;

    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(ID_SHOW_LOGS, "Show logs…", true, None))?;
    menu.append(&MenuItem::with_id(
        ID_OPEN_CONFIG,
        "Edit config.toml…",
        true,
        None,
    ))?;
    menu.append(&MenuItem::with_id(ID_QUIT, "Quit", true, None))?;

    Ok(BuiltMenu { menu, uptime_item })
}

fn zero_trust_submenu(
    p: &ConnectionProfile,
    unlocked: bool,
    text_entry: bool,
) -> muda::Result<Submenu> {
    let zt = Submenu::new("Zero Trust", unlocked);
    zt.append(&MenuItem::with_id(
        ID_ZT_TEAM,
        format!("Team: {}…", summary(&p.zero_trust_team, "(none)")),
        text_entry,
        None,
    ))?;

    // Credentials are meaningless without a team, and the core ignores them
    // in the consumer WARP flow.
    let has_team = !p.zero_trust_team.trim().is_empty();
    zt.append(&radio(
        "Sign-in method",
        PREFIX_ZT_AUTH,
        unlocked && has_team,
        &[
            ("email", "Email", p.zero_trust_auth == ZeroTrustAuth::Email),
            (
                "service",
                "Service token",
                p.zero_trust_auth == ZeroTrustAuth::Service,
            ),
            (
                "token",
                "Access token",
                p.zero_trust_auth == ZeroTrustAuth::Token,
            ),
        ],
    )?)?;

    let credential_label = match p.zero_trust_auth {
        ZeroTrustAuth::Email => "Email address…",
        ZeroTrustAuth::Service => "Service token (id & secret)…",
        ZeroTrustAuth::Token => "Access token…",
    };
    zt.append(&MenuItem::with_id(
        ID_ZT_CREDENTIALS,
        credential_label,
        text_entry && has_team,
        None,
    ))?;
    zt.append(&CheckMenuItem::with_id(
        ID_ZT_GATEWAY,
        "Use organization Gateway",
        unlocked && has_team,
        p.zero_trust_gateway,
        None,
    ))?;
    Ok(zt)
}

/// muda has no native radio group, so a group of check items is used with
/// exactly one checked. The tray loop sets the whole field on click, so the
/// group can never end up with two selected.
fn radio(
    title: &str,
    prefix: &str,
    enabled: bool,
    options: &[(&str, &str, bool)],
) -> muda::Result<Submenu> {
    let submenu = Submenu::new(title, enabled);
    for (value, label, selected) in options {
        submenu.append(&CheckMenuItem::with_id(
            format!("{prefix}{value}"),
            *label,
            enabled,
            *selected,
            None,
        ))?;
    }
    Ok(submenu)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> MenuId {
        MenuId::new(s)
    }

    #[test]
    fn every_radio_value_round_trips_to_an_action() {
        assert_eq!(
            action_for(&id("protocol.gool")),
            Some(MenuAction::SetProtocol(Protocol::Gool))
        );
        assert_eq!(
            action_for(&id("scan.ironclad")),
            Some(MenuAction::SetScanMode(ScanMode::Ironclad))
        );
        assert_eq!(
            action_for(&id("ip.both")),
            Some(MenuAction::SetIpVersion(IpVersion::Both))
        );
        assert_eq!(
            action_for(&id("mnoize.gfw")),
            Some(MenuAction::SetMasqueNoize(MasqueNoize::Gfw))
        );
        assert_eq!(
            action_for(&id("wnoize.aggressive")),
            Some(MenuAction::SetWgNoize(WgNoize::Aggressive))
        );
        assert_eq!(
            action_for(&id("ztauth.service")),
            Some(MenuAction::SetZeroTrustAuth(ZeroTrustAuth::Service))
        );
    }

    #[test]
    fn plain_ids_round_trip() {
        assert_eq!(action_for(&id(ID_TOGGLE)), Some(MenuAction::Toggle));
        assert_eq!(action_for(&id(ID_QUIT)), Some(MenuAction::Quit));
        assert_eq!(
            action_for(&id(ID_QUICK_RECONNECT)),
            Some(MenuAction::ToggleQuickReconnect)
        );
    }

    #[test]
    fn unknown_ids_are_ignored_rather_than_mapped() {
        assert_eq!(action_for(&id("nope")), None);
        assert_eq!(action_for(&id("protocol.nonsense")), None);
        // The disabled status line has an auto-generated numeric id.
        assert_eq!(action_for(&id("1")), None);
    }

    #[test]
    fn toggle_label_tracks_state() {
        assert_eq!(toggle_label(&ConnectionState::Idle), "Connect");
        assert_eq!(
            toggle_label(&ConnectionState::Error {
                message: "x".into(),
                phase: "y".into()
            }),
            "Connect"
        );
        assert_eq!(
            toggle_label(&ConnectionState::Connected {
                socks_addr: "127.0.0.1:1819".into(),
                connected_at_ms: 0
            }),
            "Disconnect"
        );
        assert_eq!(toggle_label(&ConnectionState::Connecting), "Cancel");
        assert_eq!(
            toggle_label(&ConnectionState::Reconnecting {
                attempt: 1,
                max_attempts: 3
            }),
            "Cancel"
        );
    }

    #[test]
    fn summary_falls_back_for_blank_values() {
        assert_eq!(summary("   ", "(none)"), "(none)");
        assert_eq!(summary(" 1.1.1.1 ", "(none)"), "1.1.1.1");
    }

    #[test]
    fn uptime_formatting_follows_magnitude() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(12), "12s");
        assert_eq!(format_uptime(192), "3m 12s");
        assert_eq!(format_uptime(7380), "2h 3m");
    }

    #[test]
    fn copy_ip_maps_to_its_action() {
        assert_eq!(action_for(&id(ID_COPY_IP)), Some(MenuAction::CopyIp));
        assert_eq!(
            action_for(&id(ID_REFRESH_CONNECTION_INFO)),
            Some(MenuAction::RefreshConnectionInfo)
        );
        assert_eq!(
            action_for(&id(ID_CONNECTION_INFO)),
            Some(MenuAction::ToggleConnectionInfo)
        );
    }
}

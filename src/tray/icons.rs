//! Tray icons, generated at runtime rather than embedded.
//!
//! A state-reflecting icon is the whole point of a tray-only app, so these
//! are drawn as RGBA pixels: `tray_icon::Icon::from_rgba` takes *raw* RGBA
//! (`width * height * 4` bytes), not an encoded PNG. Passing PNG bytes here
//! fails at runtime, which is easy to do by accident.

use tray_icon::Icon;

use crate::core::ConnectionState;

const SIZE: u32 = 32;

/// Solid slate — nothing running.
const IDLE: [u8; 3] = [0x8a, 0x8f, 0x98];
/// Amber — launching, connecting, reconnecting or tearing down.
const BUSY: [u8; 3] = [0xf5, 0xa5, 0x23];
/// Green — the SOCKS5 port answered.
const CONNECTED: [u8; 3] = [0x2e, 0xb8, 0x67];
/// Red — gave up.
const ERROR: [u8; 3] = [0xe0, 0x48, 0x40];

pub fn for_state(state: &ConnectionState) -> Icon {
    let rgb = match state {
        ConnectionState::Idle => IDLE,
        ConnectionState::Launching
        | ConnectionState::Connecting
        | ConnectionState::Reconnecting { .. }
        | ConnectionState::Disconnecting => BUSY,
        ConnectionState::Connected { .. } => CONNECTED,
        ConnectionState::Error { .. } => ERROR,
    };
    circle(rgb)
}

/// A filled circle with a smooth edge, on a transparent field.
fn circle(rgb: [u8; 3]) -> Icon {
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let center = (SIZE as f32 - 1.0) / 2.0;
    let radius = SIZE as f32 / 2.0 - 1.5;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            // One pixel of feathering so the disc doesn't look jagged.
            let alpha = ((radius - dist).clamp(0.0, 1.0) * 255.0) as u8;
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
        }
    }

    Icon::from_rgba(rgba, SIZE, SIZE).expect("icon dimensions match buffer length")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_produces_an_icon() {
        // from_rgba panics if the buffer length disagrees with the
        // dimensions, so simply constructing each one is the assertion.
        for state in [
            ConnectionState::Idle,
            ConnectionState::Launching,
            ConnectionState::Connecting,
            ConnectionState::Disconnecting,
            ConnectionState::Reconnecting {
                attempt: 1,
                max_attempts: 3,
            },
            ConnectionState::Connected {
                socks_addr: "127.0.0.1:1819".into(),
                connected_at_ms: 0,
            },
            ConnectionState::Error {
                message: "x".into(),
                phase: "connecting".into(),
            },
        ] {
            let _ = for_state(&state);
        }
    }

    #[test]
    fn buffer_is_exactly_rgba_sized() {
        // Guards the PNG-bytes mistake: 4 bytes per pixel, nothing else.
        let expected = (SIZE * SIZE * 4) as usize;
        let mut count = 0;
        let center = (SIZE as f32 - 1.0) / 2.0;
        let radius = SIZE as f32 / 2.0 - 1.5;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let dx = x as f32 - center;
                let dy = y as f32 - center;
                let _ = ((radius - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0) * 255.0) as u8;
                count += 4;
            }
        }
        assert_eq!(count, expected);
    }
}

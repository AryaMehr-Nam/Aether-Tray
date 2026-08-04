//! Tray icons, drawn at runtime rather than embedded.
//!
//! `tray_icon::Icon::from_rgba` takes *raw* RGBA (`width * height * 4`
//! bytes), not an encoded PNG — passing PNG bytes here fails at runtime.
//!
//! The glyph is an "AT" lettermark with a coloured status dot in the corner.
//! Colour is the reason these are **not** macOS template images: a template
//! is rendered from its alpha channel alone, so the green/amber/red dot
//! would come out the same shade as the letters and the whole point of the
//! indicator would be lost. Instead the lettermark is drawn in the colour
//! that suits the current menu-bar appearance — see `Appearance`.

use tray_icon::Icon;

use crate::core::ConnectionState;

/// Logical canvas. Menu bars ask for ~16pt; drawing at 2x and letting the
/// system downscale keeps the strokes from crawling on Retina.
const SIZE: u32 = 32;

/// Which way to paint the lettermark so it stays legible against the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    /// Light menu bar — the glyph must be dark.
    Light,
    /// Dark menu bar — the glyph must be light.
    Dark,
}

impl Appearance {
    /// Foreground for the lettermark. Not pure black/white: the menu bar
    /// tints slightly, and fully saturated ends look heavier than the system
    /// glyphs sitting next to it.
    fn glyph(self) -> [u8; 3] {
        match self {
            Appearance::Light => [0x1c, 0x1c, 0x1e],
            Appearance::Dark => [0xf2, 0xf2, 0xf7],
        }
    }
}

/// Status dot colours. Kept clear of the glyph tone in both appearances so
/// the indicator reads as a status light rather than part of the letters.
const DOT_CONNECTED: [u8; 3] = [0x30, 0xd1, 0x58];
const DOT_BUSY: [u8; 3] = [0xff, 0x9f, 0x0a];
const DOT_ERROR: [u8; 3] = [0xff, 0x45, 0x3a];

/// The dot for a state, or `None` when idle — an idle tray should look inert
/// rather than lit up in a colour that implies something is happening.
fn dot_for(state: &ConnectionState) -> Option<[u8; 3]> {
    match state {
        ConnectionState::Idle => None,
        ConnectionState::Launching
        | ConnectionState::Connecting
        | ConnectionState::Reconnecting { .. }
        | ConnectionState::Disconnecting => Some(DOT_BUSY),
        ConnectionState::Connected { .. } => Some(DOT_CONNECTED),
        ConnectionState::Error { .. } => Some(DOT_ERROR),
    }
}

pub fn for_state(state: &ConnectionState, appearance: Appearance) -> Icon {
    let mut canvas = Canvas::new();
    draw_lettermark(&mut canvas, appearance.glyph());
    if let Some(dot) = dot_for(state) {
        draw_dot(&mut canvas, dot, appearance);
    }
    canvas.into_icon()
}

/// A straight RGBA buffer with alpha-over compositing.
struct Canvas {
    px: Vec<[f32; 4]>,
}

impl Canvas {
    fn new() -> Self {
        Self {
            px: vec![[0.0; 4]; (SIZE * SIZE) as usize],
        }
    }

    /// Composites `rgb` at `coverage` (0..=1) over whatever is already there.
    fn blend(&mut self, x: i32, y: i32, rgb: [u8; 3], coverage: f32) {
        if coverage <= 0.0 || x < 0 || y < 0 || x >= SIZE as i32 || y >= SIZE as i32 {
            return;
        }
        let a = coverage.min(1.0);
        let p = &mut self.px[(y as u32 * SIZE + x as u32) as usize];
        let out_a = a + p[3] * (1.0 - a);
        if out_a <= 0.0 {
            *p = [0.0; 4];
            return;
        }
        for c in 0..3 {
            let src = rgb[c] as f32 / 255.0;
            p[c] = (src * a + p[c] * p[3] * (1.0 - a)) / out_a;
        }
        p[3] = out_a;
    }

    /// Axis-aligned bar with antialiased edges, in fractional pixel units.
    fn bar(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, rgb: [u8; 3]) {
        let (x0, x1) = (x0.min(x1), x0.max(x1));
        let (y0, y1) = (y0.min(y1), y0.max(y1));
        for y in y0.floor() as i32..=y1.ceil() as i32 {
            for x in x0.floor() as i32..=x1.ceil() as i32 {
                // Fraction of this pixel covered by the rectangle.
                let cx = (x as f32 + 1.0).min(x1) - (x as f32).max(x0);
                let cy = (y as f32 + 1.0).min(y1) - (y as f32).max(y0);
                self.blend(x, y, rgb, cx.max(0.0) * cy.max(0.0));
            }
        }
    }

    /// Bar between two arbitrary points, used for the A's diagonals.
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, rgb: [u8; 3]) {
        let steps = ((x1 - x0).abs().max((y1 - y0).abs()) * 4.0).ceil().max(1.0) as i32;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let cx = x0 + (x1 - x0) * t;
            let cy = y0 + (y1 - y0) * t;
            let h = width / 2.0;
            self.bar(cx - h, cy - h, cx + h, cy + h, rgb);
        }
    }

    fn disc(&mut self, cx: f32, cy: f32, r: f32, rgb: [u8; 3]) {
        let lo_x = (cx - r - 1.0).floor() as i32;
        let hi_x = (cx + r + 1.0).ceil() as i32;
        let lo_y = (cy - r - 1.0).floor() as i32;
        let hi_y = (cy + r + 1.0).ceil() as i32;
        for y in lo_y..=hi_y {
            for x in lo_x..=hi_x {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let d = (dx * dx + dy * dy).sqrt();
                self.blend(x, y, rgb, (r - d + 0.5).clamp(0.0, 1.0));
            }
        }
    }

    /// Clears a disc to fully transparent, punching a gap so the status dot
    /// never touches the letters at small sizes.
    fn erase_disc(&mut self, cx: f32, cy: f32, r: f32) {
        let lo_x = (cx - r - 1.0).floor().max(0.0) as u32;
        let hi_x = (cx + r + 1.0).ceil().min(SIZE as f32 - 1.0) as u32;
        let lo_y = (cy - r - 1.0).floor().max(0.0) as u32;
        let hi_y = (cy + r + 1.0).ceil().min(SIZE as f32 - 1.0) as u32;
        for y in lo_y..=hi_y {
            for x in lo_x..=hi_x {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let keep = ((dx * dx + dy * dy).sqrt() - r + 0.5).clamp(0.0, 1.0);
                self.px[(y * SIZE + x) as usize][3] *= keep;
            }
        }
    }

    fn into_icon(self) -> Icon {
        let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
        for p in &self.px {
            rgba.extend_from_slice(&[
                (p[0] * 255.0).round() as u8,
                (p[1] * 255.0).round() as u8,
                (p[2] * 255.0).round() as u8,
                (p[3] * 255.0).round() as u8,
            ]);
        }
        Icon::from_rgba(rgba, SIZE, SIZE).expect("icon dimensions match buffer length")
    }
}

/// "AT" as a compact monogram. Deliberately geometric with a heavy stroke:
/// at an effective 16pt, anything lighter or more calligraphic turns to mush.
fn draw_lettermark(c: &mut Canvas, rgb: [u8; 3]) {
    const STROKE: f32 = 3.2;
    const TOP: f32 = 7.0;
    const BOTTOM: f32 = 25.0;

    // A — two splayed legs plus a crossbar.
    let a_apex = 8.0;
    let a_left = 2.0;
    let a_right = 14.0;
    c.line(a_apex, TOP, a_left, BOTTOM, STROKE, rgb);
    c.line(a_apex, TOP, a_right, BOTTOM, STROKE, rgb);
    // Crossbar sits low so the counter stays open when downscaled.
    c.bar(a_left + 2.6, 18.4, a_right - 2.6, 18.4 + STROKE * 0.8, rgb);

    // T — cap and stem.
    let t_left = 16.5;
    let t_right = 29.0;
    let t_mid = (t_left + t_right) / 2.0;
    c.bar(t_left, TOP, t_right, TOP + STROKE, rgb);
    c.bar(t_mid - STROKE / 2.0, TOP, t_mid + STROKE / 2.0, BOTTOM, rgb);
}

/// Status dot, bottom-right, with a transparent gap around it.
fn draw_dot(c: &mut Canvas, rgb: [u8; 3], appearance: Appearance) {
    let (cx, cy, r) = (24.5, 24.5, 6.0);
    // The gap is what keeps the dot legible where it overlaps the T's stem;
    // without it the two shapes merge into an ambiguous blob at 16pt.
    c.erase_disc(cx, cy, r + 1.6);
    // A faint ring against the bar keeps the dot from vanishing when its
    // colour is close to the backdrop (amber on a light bar, for instance).
    let ring = match appearance {
        Appearance::Light => [0xff, 0xff, 0xff],
        Appearance::Dark => [0x00, 0x00, 0x00],
    };
    c.disc(cx, cy, r + 0.9, ring);
    c.disc(cx, cy, r, rgb);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states() -> Vec<ConnectionState> {
        vec![
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
        ]
    }

    #[test]
    fn every_state_and_appearance_produces_an_icon() {
        // from_rgba panics if the buffer length disagrees with the
        // dimensions, so constructing each one is itself the assertion.
        for state in states() {
            for appearance in [Appearance::Light, Appearance::Dark] {
                let _ = for_state(&state, appearance);
            }
        }
    }

    #[test]
    fn idle_has_no_status_dot_but_active_states_do() {
        assert!(dot_for(&ConnectionState::Idle).is_none());
        for state in states().into_iter().filter(|s| s != &ConnectionState::Idle) {
            assert!(dot_for(&state).is_some(), "{state:?} should have a dot");
        }
    }

    #[test]
    fn state_colours_are_distinguishable() {
        assert_eq!(
            dot_for(&ConnectionState::Connected {
                socks_addr: String::new(),
                connected_at_ms: 0
            }),
            Some(DOT_CONNECTED)
        );
        assert_eq!(
            dot_for(&ConnectionState::Error {
                message: String::new(),
                phase: String::new()
            }),
            Some(DOT_ERROR)
        );
        assert_eq!(dot_for(&ConnectionState::Connecting), Some(DOT_BUSY));
        assert_ne!(DOT_CONNECTED, DOT_ERROR);
    }

    #[test]
    fn glyph_contrasts_with_the_menu_bar_it_sits_on() {
        // The whole reason these are not template images: the tone has to
        // flip, or the lettermark disappears into the bar.
        let light = Appearance::Light.glyph();
        let dark = Appearance::Dark.glyph();
        let luma = |c: [u8; 3]| 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;
        assert!(luma(light) < 64.0, "light-bar glyph must be dark");
        assert!(luma(dark) > 192.0, "dark-bar glyph must be light");
    }

    #[test]
    fn the_lettermark_actually_marks_pixels() {
        // Guards against a silent regression to a fully transparent icon,
        // which would still construct successfully and show nothing.
        let mut c = Canvas::new();
        draw_lettermark(&mut c, [255, 255, 255]);
        let inked = c.px.iter().filter(|p| p[3] > 0.5).count();
        assert!(inked > 100, "lettermark covered only {inked} px");

        // And it should not swallow the canvas either.
        assert!(
            inked < (SIZE * SIZE) as usize / 2,
            "lettermark covered {inked} px — far too heavy"
        );
    }

    #[test]
    fn the_dot_occupies_its_corner() {
        let mut c = Canvas::new();
        draw_lettermark(&mut c, [255, 255, 255]);
        draw_dot(&mut c, DOT_CONNECTED, Appearance::Dark);
        // Sample the dot centre rather than a corner pixel, which the gap
        // and the ring both touch.
        let p = c.px[(24 * SIZE + 24) as usize];
        assert!(p[3] > 0.9, "dot centre should be opaque");
        assert!(p[1] > p[0], "connected dot should read green");
    }
}

//! Desktop notifications for state changes.
//!
//! `notify-rust` covers all three platforms — macOS through
//! `mac-notification-sys`, Linux through D-Bus, Windows through WinRT toasts
//! — so no per-platform code is needed here.
//!
//! Delivery happens on a dedicated worker thread, and that is load-bearing on
//! macOS rather than a nicety. `NotificationHandle`'s `Drop` calls into
//! `mac-notification-sys`, which pumps the main run loop; doing that from
//! inside a winit event handler makes winit abort the process with "tried to
//! handle event while another event is currently being handled". The tray
//! calls `send` from exactly that context, so the actual `show()` must not
//! run on the main thread.

use anyhow::{Result, anyhow};
use log::warn;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};

struct Toast {
    title: String,
    body: String,
}

static SENDER: OnceLock<Sender<Toast>> = OnceLock::new();

fn sender() -> &'static Sender<Toast> {
    SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Toast>();
        std::thread::Builder::new()
            .name("notify".into())
            .spawn(move || {
                for toast in rx {
                    // `show()` returns a handle whose Drop does the run-loop
                    // pumping; keeping it in this thread's scope is the point.
                    if let Err(e) = notify_rust::Notification::new()
                        .summary(&toast.title)
                        .body(&toast.body)
                        .appname("Aether")
                        .show()
                    {
                        warn!("notification failed: {e}");
                    }
                }
            })
            .expect("spawning the notification thread");
        tx
    })
}

/// Queues a notification. Returns once it is handed to the worker — never
/// blocks on the platform notification system.
pub fn send(title: &str, body: &str) -> Result<()> {
    sender()
        .send(Toast {
            title: title.to_string(),
            body: body.to_string(),
        })
        .map_err(|_| anyhow!("the notification thread has stopped"))
}

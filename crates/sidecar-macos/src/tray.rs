//! macOS menu-bar (system tray) UI for the sidecar.
//!
//! Runs an `NSApplication` on the main thread with a status bar item
//! and a two-entry menu: a disabled label for the current connection
//! state, and a Quit button that calls `NSApp.terminate(:)`. The
//! activation policy is `Accessory`, so we get a status item without
//! a Dock icon or app-switcher entry.
//!
//! The status label is driven off an `Arc<AtomicU8>` shared with the
//! tokio worker that owns the QUIC dial loop. A 0.5 s `NSTimer` on
//! the main run loop polls the atomic and updates the label whenever
//! it changes — small enough latency for a humans-watching-it use
//! case, no cross-thread ObjC dance required.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, AllocAnyThread, DefinedClass};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSMenu, NSMenuItem, NSStatusBar,
    NSVariableStatusItemLength,
};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString, NSTimer};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ConnState {
    Connecting = 0,
    Connected = 1,
    Disconnected = 2,
}

impl ConnState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => ConnState::Connected,
            2 => ConnState::Disconnected,
            _ => ConnState::Connecting,
        }
    }

    fn label(self) -> &'static str {
        match self {
            ConnState::Connecting => "Status: Connecting…",
            ConnState::Connected => "Status: Connected",
            ConnState::Disconnected => "Status: Disconnected",
        }
    }
}

/// Update this from any thread to drive the menu's status label.
pub fn store(state: &Arc<AtomicU8>, value: ConnState) {
    state.store(value as u8, Ordering::Relaxed);
}

#[derive(Default)]
pub(crate) struct StatusRefreshIvars {
    /// The "Status: …" menu item whose title we keep in sync with
    /// the atomic. `Mutex<Option<…>>` rather than a bare ivar so the
    /// type can be `Default`-derived (required by `define_class!`'s
    /// instance allocation path).
    item: Mutex<Option<Retained<NSMenuItem>>>,
    state: Mutex<Option<Arc<AtomicU8>>>,
    /// Last value we wrote to the menu item — skip the `setTitle:`
    /// round-trip when nothing has changed since the last tick.
    last: Mutex<u8>,
}

define_class!(
    /// NSObject subclass so we can hand a `target` + `selector` pair
    /// to `NSTimer`. The `refresh:` method is invoked on the main
    /// run loop every tick.
    #[unsafe(super(NSObject))]
    #[ivars = StatusRefreshIvars]
    pub(crate) struct StatusRefresh;

    impl StatusRefresh {
        #[unsafe(method(refresh:))]
        fn refresh(&self, _sender: Option<&AnyObject>) {
            let Some(state_byte) = self
                .ivars()
                .state
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(|a| a.load(Ordering::Relaxed)))
            else {
                return;
            };
            let mut last = match self.ivars().last.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            // Sentinel 0xff = "haven't written yet"; force a write
            // on first refresh even if the state is the default 0.
            if *last == state_byte {
                return;
            }
            *last = state_byte;
            drop(last);

            let label = ConnState::from_u8(state_byte).label();
            if let Ok(g) = self.ivars().item.lock() {
                if let Some(item) = g.as_ref() {
                    item.setTitle(&NSString::from_str(label));
                }
            }
        }
    }

    unsafe impl NSObjectProtocol for StatusRefresh {}
);

impl StatusRefresh {
    fn new(item: Retained<NSMenuItem>, state: Arc<AtomicU8>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(StatusRefreshIvars {
            item: Mutex::new(Some(item)),
            state: Mutex::new(Some(state)),
            // Sentinel that won't match any real ConnState (0..=2).
            last: Mutex::new(0xff),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Build the menu bar UI and start the AppKit event loop. Blocks
/// the caller (which **must** be the main thread) until the user
/// chooses Quit.
pub fn run_event_loop(state: Arc<AtomicU8>) {
    let mtm = MainThreadMarker::new().expect("tray must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);

    // Accessory = no Dock icon, no app-switcher entry. The status
    // item is the only UI surface.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let bar = NSStatusBar::systemStatusBar();
    let status_item = bar.statusItemWithLength(NSVariableStatusItemLength);

    if let Some(button) = status_item.button(mtm) {
        // Short text shown in the menu bar. Replace with an
        // NSImage template later if we want a glyph.
        button.setTitle(&NSString::from_str("x11-web"));
    }

    let menu = NSMenu::new(mtm);

    // Disabled label item — clicks do nothing, but it's visible.
    let label_item = NSMenuItem::new(mtm);
    label_item.setTitle(&NSString::from_str(ConnState::Connecting.label()));
    label_item.setEnabled(false);
    menu.addItem(&label_item);

    menu.addItem(&NSMenuItem::separatorItem(mtm));

    let quit_item = NSMenuItem::new(mtm);
    quit_item.setTitle(&NSString::from_str("Quit"));
    // `terminate:` on `NSApplication` cleanly tears down the
    // process (closes windows, fires app-delegate hooks, exits).
    unsafe { quit_item.setAction(Some(sel!(terminate:))) };
    unsafe { quit_item.setTarget(Some(&app)) };
    menu.addItem(&quit_item);

    status_item.setMenu(Some(&menu));

    // Periodic refresh of the label from the atomic. 500 ms is
    // imperceptible for a status display and keeps the AppKit run
    // loop quiet otherwise.
    let refresher = StatusRefresh::new(label_item, state);
    let _timer = unsafe {
        NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
            0.5,
            &*refresher,
            sel!(refresh:),
            None,
            true,
        )
    };

    // The status item must outlive the run loop, otherwise dropping
    // its `Retained` removes the menu bar icon. `app.run()` blocks
    // until terminate: fires, after which the process exits anyway.
    std::mem::forget(status_item);
    std::mem::forget(refresher);

    app.run();
}

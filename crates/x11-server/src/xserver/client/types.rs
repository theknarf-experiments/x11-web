//! Auxiliary types used by ClientState.

/// Security authorization token. None of the fields are consumed by
/// dispatch today — the SECURITY extension stores them so clients can
/// query/revoke; effective enforcement of `trust_level` happens through
/// the parallel `SecurityTokenInfo` entry in `shared_security_tokens`.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct SecurityAuthorization {
    pub(crate) auth_id: u32,
    pub(crate) trust_level: u32, // 0 = trusted, 1 = untrusted
    pub(crate) timeout: u32,
    pub(crate) group: u32,
    pub(crate) event_mask: u32,
}

/// Access control host entry.
#[derive(Clone, Debug)]
pub(crate) struct AccessHost {
    pub(crate) family: u8,
    pub(crate) address: Vec<u8>,
}

/// Keyboard control settings (for Get/ChangeKeyboardControl).
#[derive(Clone)]
pub(crate) struct KeyboardControl {
    pub(crate) key_click_percent: u8,
    pub(crate) bell_percent: u8,
    pub(crate) bell_pitch: u16,
    pub(crate) bell_duration: u16,
    pub(crate) led_mask: u32,
    pub(crate) global_auto_repeat: u8,
    pub(crate) auto_repeats: [u8; 32],
}

impl Default for KeyboardControl {
    fn default() -> Self {
        // Match Xvfb defaults: all keys auto-repeat EXCEPT modifiers and lock keys.
        // The auto_repeats vector is a bitmap indexed by keycode (bit n of byte
        // n/8 is keycode n). Per X11 spec §6.5 modifier keys must not auto-repeat
        // — otherwise holding Shift would emit a stream of Shift events.
        // Standard XKB modifier keycodes from the default `pc105+inet(evdev)`
        // layout that ships with the keymap helper.
        let mut auto_repeats = [0xFFu8; 32];
        const MODIFIER_KEYCODES: &[u8] = &[
            37,  // Control_L
            50,  // Shift_L
            62,  // Shift_R
            64,  // Alt_L (Meta_L)
            66,  // Caps_Lock
            77,  // Num_Lock
            105, // Control_R
            108, // Alt_R (ISO_Level3_Shift)
            133, // Super_L
            134, // Super_R
            135, // Menu
            203, // Meta_L (alt mapping)
            204, // Meta_R (alt mapping)
            207, // Hyper_L
            208, // Hyper_R
        ];
        for &kc in MODIFIER_KEYCODES {
            crate::xserver::types::keycode_bitset::clear(&mut auto_repeats, kc);
        }
        Self {
            key_click_percent: 0,
            bell_percent: 50,
            bell_pitch: 400,
            bell_duration: 100,
            led_mask: 0,
            global_auto_repeat: 1, // on
            auto_repeats,
        }
    }
}

/// Pointer control settings (for Get/ChangePointerControl).
#[derive(Clone)]
pub(crate) struct PointerControl {
    pub(crate) acceleration_numerator: u16,
    pub(crate) acceleration_denominator: u16,
    pub(crate) threshold: u16,
}

impl Default for PointerControl {
    fn default() -> Self {
        Self {
            acceleration_numerator: 2,
            acceleration_denominator: 1,
            threshold: 4,
        }
    }
}

/// Per-connection selection / clipboard state.
pub(crate) struct SelectionState {
    /// Selection ownership: atom → owner window id.
    pub(crate) owners: std::collections::HashMap<u32, u32>,
    /// Timestamps when each selection was acquired (atom → timestamp).
    pub(crate) timestamps: std::collections::HashMap<u32, u32>,
    /// Shared cross-connection selection registry.
    pub(crate) shared: super::super::types::SharedSelections,
    /// Pending INCR (incremental) selection transfers.
    pub(crate) incr_transfers: Vec<super::super::types::IncrTransfer>,
    /// Channel for clipboard events (selection ownership changes, data responses).
    pub(crate) clipboard_notify_tx: Option<tokio::sync::mpsc::UnboundedSender<()>>,
    /// Persistent clipboard data saved when a clipboard owner disconnects.
    pub(crate) persistent_clipboard: super::super::types::PersistentClipboard,
}

impl SelectionState {
    pub(crate) fn new(
        shared: super::super::types::SharedSelections,
        clipboard_notify_tx: tokio::sync::mpsc::UnboundedSender<()>,
        persistent_clipboard: super::super::types::PersistentClipboard,
    ) -> Self {
        Self {
            owners: std::collections::HashMap::new(),
            timestamps: std::collections::HashMap::new(),
            shared,
            incr_transfers: Vec::new(),
            clipboard_notify_tx: Some(clipboard_notify_tx),
            persistent_clipboard,
        }
    }
}

/// Per-connection pointer state (subsystem-private bookkeeping; hot-path
/// `pointer_x`/`pointer_y` live directly on `ClientState`).
pub(crate) struct PointerState {
    /// Current pointer button mask (bits 8-12 for buttons 1-5).
    pub(crate) button_mask: u16,
    /// POINTER_MOTION_HINT_MASK: when true, motion events are suppressed
    /// until QueryPointer/GetMotionEvents or button/crossing event occurs.
    pub(crate) motion_hint_suppressed: bool,
    /// Motion history buffer (circular): (timestamp_ms, x, y).
    pub(crate) motion_history: Vec<(u32, i16, i16)>,
    /// Pointer button mapping (button 1-7 -> mapped button).
    pub(crate) mapping: [u8; 7],
    /// Pointer control settings (acceleration, threshold).
    pub(crate) control: PointerControl,
}

impl Default for PointerState {
    fn default() -> Self {
        Self {
            button_mask: 0,
            motion_hint_suppressed: false,
            motion_history: Vec::new(),
            mapping: [1, 2, 3, 4, 5, 6, 7],
            control: PointerControl::default(),
        }
    }
}

/// Per-connection keyboard state (subsystem-private bookkeeping; XKB
/// modifier/group state lives on `ClientState::xkb`).
pub(crate) struct KeyboardState {
    /// Keyboard control settings (auto-repeat, bell, LED mask).
    pub(crate) control: KeyboardControl,
    /// Currently pressed keys (for QueryKeymap).
    pub(crate) pressed_keys: [u8; 32],
    /// Modifier mapping: 8 modifiers x N keycodes.
    pub(crate) modifier_map: Vec<Vec<u8>>,
    /// Custom keycode→keysym mapping (ChangeKeyboardMapping).
    /// Key = keycode, value = list of keysyms for that keycode.
    /// Server-wide (shared across connections) per the X11 spec —
    /// `xmodmap` from one client must be observable from another.
    pub(crate) custom_keymap: super::super::types::SharedKeymap,
}

impl KeyboardState {
    pub(crate) fn new(custom_keymap: super::super::types::SharedKeymap) -> Self {
        Self {
            control: KeyboardControl::default(),
            pressed_keys: [0; 32],
            modifier_map: Vec::new(),
            custom_keymap,
        }
    }
}

/// Per-connection screen-saver state. Bundles the core `SetScreenSaver`
/// settings together with the MIT-SCREEN-SAVER extension fields, both of
/// which describe the same subsystem.
#[derive(Default)]
pub(crate) struct ScreenSaverState {
    // -- core SetScreenSaver / GetScreenSaver --
    pub(crate) timeout: u16,
    pub(crate) interval: u16,
    pub(crate) prefer_blanking: u8,
    pub(crate) allow_exposures: u8,
    /// Whether the screen saver is currently active.
    pub(crate) active: bool,
    /// Timestamp (ms since server start) of the last screen saver timer reset.
    pub(crate) last_reset_ms: u32,

    // -- MIT-SCREEN-SAVER extension --
    /// Event mask for `ScreenSaverNotify`.
    pub(crate) event_mask: u32,
    /// Window ID for the screen saver window.
    pub(crate) window: u32,
    /// Attributes stored by `SetAttributes`.
    pub(crate) attrs: Option<super::super::handlers::screensaver::ScreenSaverAttrs>,
    /// Reference-counted suspend level.
    pub(crate) suspend_count: u32,
}

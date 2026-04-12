//! Auxiliary types used by ClientState.

/// Security authorization token.
#[derive(Clone, Debug)]
#[allow(dead_code)]
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
        // Match Xvfb defaults: all keys auto-repeat except modifiers
        let auto_repeats = [0xFFu8; 32];
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

/// Screen saver settings (for Get/SetScreenSaver).
#[derive(Clone, Default)]
pub(crate) struct ScreenSaverSettings {
    pub(crate) timeout: u16,
    pub(crate) interval: u16,
    pub(crate) prefer_blanking: u8,
    pub(crate) allow_exposures: u8,
    /// Whether the screen saver is currently active.
    pub(crate) active: bool,
    /// Timestamp (ms since server start) of the last screen saver timer reset.
    pub(crate) last_reset_ms: u32,
}

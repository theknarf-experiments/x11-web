//! X11 compose key and dead key support, backed by libxkbcommon.
//!
//! Wraps xkbcommon's compose state machine so the rest of the sidecar can
//! treat compose like any other key processor: feed keysyms, get back a
//! decision (pass through, consume, complete with text, or cancel/replay).
//!
//! xkbcommon does the heavy lifting of locating the right compose file via
//! `LANG`/`XCOMPOSEFILE`, parsing it, and walking the trie. We only keep a
//! small mirror of the in-flight keysym buffer so we can replay it when a
//! sequence is cancelled (xkbcommon discards that buffer).

use std::ffi::OsString;
use xkbcommon::xkb;

/// Compose state machine for handling multi-key sequences.
pub struct ComposeState {
    /// xkbcommon owns the actual compose trie and state.
    inner: Option<xkb::compose::State>,
    /// In-flight keysyms since the last reset/finalize. Used to populate
    /// `ComposeResult::Cancelled` so the caller can replay them.
    buffered: Vec<u32>,
}

// xkbcommon state objects are documented as "single-owner, thread-compatible"
// (not thread-safe — but movable). Our ComposeState lives in ClientState,
// which is owned by exactly one tokio task at a time, so making it Send is
// sound. This impl is required because tokio::spawn needs Send futures.
unsafe impl Send for ComposeState {}

/// Return type of [`ComposeState::process`].
pub enum ComposeResult {
    /// Not composing — forward the keysym unchanged.
    Pass(u32),
    /// Key consumed by compose sequence — don't forward.
    Consumed,
    /// Compose sequence complete — inject this text.
    Composed(String),
    /// Compose sequence failed — replay these keysyms.
    Cancelled(Vec<u32>),
}

impl ComposeState {
    pub fn new() -> Self {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        // Locale resolution: prefer LC_ALL > LC_CTYPE > LANG > "C.UTF-8".
        let locale = std::env::var_os("LC_ALL")
            .or_else(|| std::env::var_os("LC_CTYPE"))
            .or_else(|| std::env::var_os("LANG"))
            .unwrap_or_else(|| OsString::from("C.UTF-8"));
        let inner =
            xkb::compose::Table::new_from_locale(&context, &locale, xkb::compose::COMPILE_NO_FLAGS)
                .ok()
                .map(|table| xkb::compose::State::new(&table, xkb::compose::STATE_NO_FLAGS));
        Self {
            inner,
            buffered: Vec::new(),
        }
    }

    /// Process a keysym through the compose state machine.
    pub fn process(&mut self, keysym: u32) -> ComposeResult {
        let Some(state) = self.inner.as_mut() else {
            // Compose unavailable for this locale — every key passes through.
            return ComposeResult::Pass(keysym);
        };

        let was_composing = !self.buffered.is_empty();
        let _ = state.feed(xkb::Keysym::new(keysym));

        match state.status() {
            xkb::compose::Status::Nothing => {
                // Not in or starting a sequence. xkbcommon may have
                // recognised the key (Accepted) or not (Ignored), but
                // either way nothing is in flight, so pass through.
                self.buffered.clear();
                ComposeResult::Pass(keysym)
            }
            xkb::compose::Status::Composing => {
                // Sequence in progress — could be the starter or a continuation.
                self.buffered.push(keysym);
                ComposeResult::Consumed
            }
            xkb::compose::Status::Composed => {
                // Full sequence matched. Grab the utf8, reset, return text.
                let text = state.utf8().unwrap_or_default();
                state.reset();
                self.buffered.clear();
                if text.is_empty() {
                    // No printable result — fall back to passing the final keysym
                    // through so the caller still gets *something*.
                    ComposeResult::Pass(keysym)
                } else {
                    ComposeResult::Composed(text)
                }
            }
            xkb::compose::Status::Cancelled => {
                // Bad sequence. Caller replays the keys it has been suppressing
                // plus this one (which xkbcommon already swallowed).
                let mut replay = std::mem::take(&mut self.buffered);
                replay.push(keysym);
                state.reset();
                if !was_composing {
                    // Edge case: cancellation on the very first key. Just pass
                    // it through.
                    return ComposeResult::Pass(keysym);
                }
                ComposeResult::Cancelled(replay)
            }
        }
    }
}

impl Default for ComposeState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `a` should pass through untouched.
    #[test]
    fn plain_key_passes() {
        let mut cs = ComposeState::new();
        match cs.process(0x61) {
            ComposeResult::Pass(k) => assert_eq!(k, 0x61),
            _ => panic!("expected Pass for plain 'a'"),
        }
    }

    /// Dead acute + 'a' should compose to 'á' (U+00E1) when a system compose
    /// file is available. If the runtime has no compose data we accept Pass.
    #[test]
    fn dead_acute_plus_a_composes_to_aacute() {
        let mut cs = ComposeState::new();
        const XK_DEAD_ACUTE: u32 = 0xfe51;
        // First key: dead_acute starts a sequence (or passes through).
        let _ = cs.process(XK_DEAD_ACUTE);
        // Second key: 'a' completes it.
        match cs.process(0x61) {
            ComposeResult::Composed(text) => {
                // Must contain á; xkbcommon may give us either the bare
                // codepoint or a longer cluster.
                assert!(
                    text.contains('\u{00e1}'),
                    "expected á in composed result, got {text:?}"
                );
            }
            ComposeResult::Pass(_) => {
                // No compose data on this host — acceptable.
            }
            other => panic!("unexpected compose result: {:?}", classify(&other)),
        }
    }

    fn classify(r: &ComposeResult) -> &'static str {
        match r {
            ComposeResult::Pass(_) => "Pass",
            ComposeResult::Consumed => "Consumed",
            ComposeResult::Composed(_) => "Composed",
            ComposeResult::Cancelled(_) => "Cancelled",
        }
    }
}

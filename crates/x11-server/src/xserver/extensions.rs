//! Extension registry — central source of truth for all X11 extension metadata.
//!
//! Every extension is described by an [`ExtensionInfo`] entry.  The
//! [`ExtensionRegistry`] owns the full table and is the single place that
//! `QueryExtension`, `ListExtensions`, and the request dispatcher consult to
//! decide which extensions are available and what their opcodes / event / error
//! bases are.
//!
//! Extensions can be **compiled out** via Cargo feature flags (e.g.
//! `ext-glx`) or **disabled at runtime** through
//! [`ExtensionRegistry::set_enabled`].

use std::collections::HashMap;

use super::client::ClientState;
use super::handlers;

/// Signature every extension dispatcher implements.
///
/// `data` is the full request packet (header + body); `seq` is the
/// sequence number to stamp on the reply. The handler returns the
/// raw reply bytes (or `Vec::new()` for void requests).
pub(crate) type ExtHandlerFn = fn(&mut ClientState, &[u8], u16) -> Vec<u8>;

// Extension `first_event` bases — the wire event-type byte assigned to each
// extension's first event. Must stay in sync with the table in `add(...)`
// calls in `ExtensionRegistry::new` (covered by `extension_event_bases_no_overlap`).
// SHAPE, XKB, XFIXES, RANDR, SECURITY, XVIDEO have their first_event recorded
// in the registry table below; only the few that handlers explicitly write into
// outgoing events need named constants here.
pub(crate) const SHM_FIRST_EVENT: u8 = 65;
pub(crate) const SYNC_FIRST_EVENT: u8 = 83;
pub(crate) const DAMAGE_FIRST_EVENT: u8 = 91;

// ---------------------------------------------------------------------------
// ExtensionId — one variant per extension we support
// ---------------------------------------------------------------------------

/// Identifies a specific X11 extension.  Used as the key in the registry and
/// for feature-gating via `cfg`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ExtensionId {
    Shape,
    MitShm,
    XInput,
    BigRequests,
    Sync,
    GenericEvent,
    Xkb,
    Xfixes,
    Render,
    Randr,
    XcMisc,
    Composite,
    Damage,
    Present,
    Xtest,
    Dpms,
    ScreenSaver,
    VidMode,
    Record,
    Security,
    XVideo,
    Dbe,
    Xinerama,
    Glx,
    XResource,
}

// ---------------------------------------------------------------------------
// ExtensionInfo — per-extension metadata
// ---------------------------------------------------------------------------

/// Static metadata for a single extension, mirroring what `QueryExtension`
/// returns to the client, plus a handler function pointer used by
/// the request dispatcher.
#[derive(Clone)]
pub(crate) struct ExtensionInfo {
    /// Extension identifier. Read by `set_enabled` (test-only) and by
    /// debugging/diagnostic code; the runtime dispatcher selects by
    /// opcode, not by id.
    #[allow(dead_code)]
    pub id: ExtensionId,
    /// The canonical X11 wire name (e.g. `"RENDER"`, `"MIT-SHM"`).
    pub wire_name: &'static str,
    /// Major opcode assigned by the server.
    pub major_opcode: u8,
    /// First event code, or 0 if the extension defines no events.
    pub first_event: u8,
    /// First error code, or 0 if the extension defines no errors.
    pub first_error: u8,
    /// Dispatch entry point for this extension.
    pub handler: ExtHandlerFn,
    /// Whether the extension is currently enabled.
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// ExtensionRegistry
// ---------------------------------------------------------------------------

/// Central registry holding every extension the server can serve.
///
/// Built once at startup via [`ExtensionRegistry::new`] with all compiled-in
/// extensions enabled.  Individual extensions can be toggled off at runtime
/// with [`set_enabled`](ExtensionRegistry::set_enabled).
pub(crate) struct ExtensionRegistry {
    /// Ordered list (insertion order = `ListExtensions` order).
    entries: Vec<ExtensionInfo>,
    /// Wire name → index into `entries` for O(1) lookup.
    by_name: HashMap<&'static str, usize>,
    /// Major opcode → index into `entries` for O(1) dispatch.
    by_opcode: HashMap<u8, usize>,
}

impl ExtensionRegistry {
    /// Create a new registry with the default set of extensions.
    ///
    /// Extensions that have been compiled out via Cargo features are simply
    /// omitted from the table.
    pub fn new() -> Self {
        let mut reg = Self {
            entries: Vec::with_capacity(26),
            by_name: HashMap::with_capacity(26),
            by_opcode: HashMap::with_capacity(26),
        };

        // Helper closure to avoid repetition.
        let add = |reg: &mut Self,
                   id,
                   wire_name,
                   major_opcode,
                   first_event,
                   first_error,
                   handler: ExtHandlerFn| {
            let idx = reg.entries.len();
            reg.entries.push(ExtensionInfo {
                id,
                wire_name,
                major_opcode,
                first_event,
                first_error,
                handler,
                enabled: true,
            });
            reg.by_name.insert(wire_name, idx);
            reg.by_opcode.insert(major_opcode, idx);
        };

        use ExtensionId::*;

        // --- ext-core (always compiled in) -----------------------------------
        add(&mut reg, Shape, "SHAPE", 128, 64, 0, handlers::shape::handle_shape_request);
        add(&mut reg, MitShm, "MIT-SHM", 130, 65, 128, handlers::shm::handle_shm_request);
        add(
            &mut reg,
            BigRequests,
            "BIG-REQUESTS",
            133,
            0,
            0,
            handlers::big_requests::handle_big_requests_request,
        );
        add(&mut reg, Sync, "SYNC", 134, 83, 0, handlers::sync::handle_sync_request);
        add(
            &mut reg,
            GenericEvent,
            "Generic Event Extension",
            135,
            0,
            0,
            handlers::generic_event::handle_ge_request,
        );
        add(
            &mut reg,
            Xfixes,
            "XFIXES",
            138,
            87,
            0,
            handlers::xfixes::handle_xfixes_request,
        );
        add(&mut reg, Randr, "RANDR", 140, 89, 0, handlers::randr::handle_randr_request);
        add(
            &mut reg,
            XcMisc,
            "XC-MISC",
            141,
            0,
            0,
            handlers::xc_misc::handle_xc_misc_request,
        );
        add(
            &mut reg,
            XResource,
            "X-Resource",
            160,
            0,
            0,
            handlers::xresource::handle_xresource_request,
        );

        // --- ext-input -------------------------------------------------------
        #[cfg(feature = "ext-input")]
        {
            add(
                &mut reg,
                XInput,
                "XInputExtension",
                131,
                105,
                152,
                handlers::xinput::handle_xinput_request,
            );
            add(&mut reg, Xtest, "XTEST", 150, 0, 0, handlers::xtest::handle_xtest_request);
            add(&mut reg, Xkb, "XKEYBOARD", 136, 85, 0, handlers::xkb::handle_xkb_request);
        }

        // --- ext-render ------------------------------------------------------
        #[cfg(feature = "ext-render")]
        {
            add(&mut reg, Render, "RENDER", 139, 0, 142, handlers::render::handle_render_request);
            add(
                &mut reg,
                Composite,
                "Composite",
                142,
                0,
                0,
                handlers::composite::handle_x_composite_request,
            );
            add(
                &mut reg,
                Damage,
                "DAMAGE",
                143,
                91,
                // 153, not 152 — XInputExtension owns 152; two
                // extensions sharing an error base makes error codes
                // ambiguous for clients.
                153,
                handlers::damage::handle_damage_request,
            );
            add(
                &mut reg,
                Present,
                "Present",
                148,
                0,
                0,
                handlers::present::handle_present_request,
            );
        }

        // --- ext-glx ---------------------------------------------------------
        #[cfg(feature = "ext-glx")]
        {
            add(&mut reg, Glx, "GLX", 159, 0, 159, handlers::glx::handle_glx_request);
            // DRI3 is NOT registered: our server does not provide GPU/DRM
            // access, so advertising DRI3 causes Mesa to attempt (and fail)
            // hardware-accelerated DRI rendering before falling back.
        }

        // --- ext-media -------------------------------------------------------
        #[cfg(feature = "ext-media")]
        {
            add(
                &mut reg,
                XVideo,
                "XVideo",
                156,
                95,
                156,
                handlers::xvideo::handle_xvideo_request,
            );
            add(&mut reg, Dbe, "DOUBLE-BUFFER", 157, 0, 157, handlers::dbe::handle_dbe_request);
        }

        // --- ext-compat ------------------------------------------------------
        #[cfg(feature = "ext-compat")]
        {
            add(&mut reg, Dpms, "DPMS", 151, 0, 0, handlers::dpms::handle_dpms_request);
            add(
                &mut reg,
                ScreenSaver,
                "MIT-SCREEN-SAVER",
                152,
                92,
                0,
                handlers::screensaver::handle_screen_saver_request,
            );
            add(
                &mut reg,
                VidMode,
                "XFree86-VidModeExtension",
                153,
                0,
                0,
                handlers::vidmode::handle_vidmode_request,
            );
            add(
                &mut reg,
                Record,
                "RECORD",
                154,
                0,
                154,
                handlers::record::handle_record_request,
            );
            add(
                &mut reg,
                Security,
                "SECURITY",
                155,
                93,
                155,
                handlers::security::handle_security_request,
            );
            add(
                &mut reg,
                Xinerama,
                "XINERAMA",
                158,
                0,
                0,
                handlers::xinerama::handle_xinerama_request,
            );
        }

        reg
    }

    // -- Queries --------------------------------------------------------------

    /// Look up extension info by its wire name. Returns `None` if the
    /// extension is not compiled in.
    pub fn by_name(&self, name: &str) -> Option<&ExtensionInfo> {
        self.by_name.get(name).map(|&i| &self.entries[i])
    }

    /// Look up extension info by major opcode. Returns `None` if no extension
    /// uses that opcode.
    pub fn by_opcode(&self, opcode: u8) -> Option<&ExtensionInfo> {
        self.by_opcode.get(&opcode).map(|&i| &self.entries[i])
    }

    /// Iterate over all extensions (for `ListExtensions`).  Only enabled
    /// extensions are returned.
    pub fn enabled_extensions(&self) -> impl Iterator<Item = &ExtensionInfo> {
        self.entries.iter().filter(|e| e.enabled)
    }

    // -- Runtime toggling -----------------------------------------------------

    /// Enable or disable an extension at runtime.  Returns `true` if the
    /// extension was found (regardless of its previous state).
    #[cfg(test)]
    pub fn set_enabled(&mut self, id: ExtensionId, enabled: bool) -> bool {
        for entry in &mut self.entries {
            if entry.id == id {
                entry.enabled = enabled;
                return true;
            }
        }
        false
    }

    /// Disable extensions by wire name from a comma-separated list —
    /// the `X11WEB_DISABLE_EXTENSIONS` kill switch. Disabled
    /// extensions vanish from QueryExtension / ListExtensions and
    /// their requests answer BadRequest, so clients take their
    /// no-extension fallback paths. Exists to bisect app breakage
    /// caused by partially implemented extensions without rebuilding.
    pub fn disable_by_names(&mut self, list: &str) {
        for raw in list.split(',') {
            let name = raw.trim();
            if name.is_empty() {
                continue;
            }
            let mut found = false;
            for entry in &mut self.entries {
                if entry.wire_name.eq_ignore_ascii_case(name) {
                    entry.enabled = false;
                    found = true;
                    tracing::warn!(
                        "extension {:?} disabled via X11WEB_DISABLE_EXTENSIONS",
                        entry.wire_name
                    );
                }
            }
            if !found {
                tracing::warn!(
                    "X11WEB_DISABLE_EXTENSIONS: no extension named {name:?} \
                     (known: {})",
                    self.entries
                        .iter()
                        .map(|e| e.wire_name)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
    }
}

// Adapter so x11rb's `Request::parse` can dispatch to extensions by
// looking up their major opcode / event / error ranges in our registry.
impl x11rb_protocol::x11_utils::ExtInfoProvider for ExtensionRegistry {
    fn get_from_major_opcode(
        &self,
        major_opcode: u8,
    ) -> Option<(&str, x11rb_protocol::x11_utils::ExtensionInformation)> {
        let info = self.by_opcode(major_opcode)?;
        if !info.enabled {
            return None;
        }
        Some((
            info.wire_name,
            x11rb_protocol::x11_utils::ExtensionInformation {
                major_opcode: info.major_opcode,
                first_event: info.first_event,
                first_error: info.first_error,
            },
        ))
    }

    fn get_from_event_code(
        &self,
        event_code: u8,
    ) -> Option<(&str, x11rb_protocol::x11_utils::ExtensionInformation)> {
        // Linear scan — the registry is small (<30 entries).
        let info = self
            .entries
            .iter()
            .find(|e| e.enabled && e.first_event != 0 && event_code >= e.first_event)?;
        Some((
            info.wire_name,
            x11rb_protocol::x11_utils::ExtensionInformation {
                major_opcode: info.major_opcode,
                first_event: info.first_event,
                first_error: info.first_error,
            },
        ))
    }

    fn get_from_error_code(
        &self,
        error_code: u8,
    ) -> Option<(&str, x11rb_protocol::x11_utils::ExtensionInformation)> {
        let info = self
            .entries
            .iter()
            .find(|e| e.enabled && e.first_error != 0 && error_code >= e.first_error)?;
        Some((
            info.wire_name,
            x11rb_protocol::x11_utils::ExtensionInformation {
                major_opcode: info.major_opcode,
                first_event: info.first_event,
                first_error: info.first_error,
            },
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_major_opcodes_are_unique() {
        let reg = ExtensionRegistry::new();
        let mut seen = HashSet::new();
        for ext in &reg.entries {
            assert!(
                seen.insert(ext.major_opcode),
                "Duplicate major opcode {} for {:?} and a previous extension",
                ext.major_opcode,
                ext.id,
            );
        }
    }

    #[test]
    fn wire_names_are_unique() {
        let reg = ExtensionRegistry::new();
        let mut seen = HashSet::new();
        for ext in &reg.entries {
            assert!(
                seen.insert(ext.wire_name),
                "Duplicate wire name {:?}",
                ext.wire_name,
            );
        }
    }

    #[test]
    fn by_name_lookup_works() {
        let reg = ExtensionRegistry::new();
        let info = reg.by_name("SHAPE").expect("SHAPE must be present");
        assert_eq!(info.major_opcode, 128);
        assert_eq!(info.id, ExtensionId::Shape);
    }

    #[test]
    fn by_opcode_lookup_works() {
        let reg = ExtensionRegistry::new();
        let info = reg.by_opcode(128).expect("opcode 128 must be present");
        assert_eq!(info.wire_name, "SHAPE");
    }

    #[test]
    fn set_enabled_toggles() {
        let mut reg = ExtensionRegistry::new();
        assert!(reg.by_name("SHAPE").unwrap().enabled);
        reg.set_enabled(ExtensionId::Shape, false);
        assert!(!reg.by_name("SHAPE").unwrap().enabled);
        // Disabled extension should not appear in enabled_extensions()
        assert!(reg.enabled_extensions().all(|e| e.id != ExtensionId::Shape));
    }

    #[test]
    fn enabled_extensions_filters() {
        let mut reg = ExtensionRegistry::new();
        let total = reg.entries.len();
        reg.set_enabled(ExtensionId::Shape, false);
        let enabled_count = reg.enabled_extensions().count();
        assert_eq!(enabled_count, total - 1);
    }

    #[test]
    fn query_extension_returns_correct_randr_event_base() {
        let reg = ExtensionRegistry::new();
        let info = reg.by_name("RANDR").expect("RANDR must be present");
        assert_eq!(info.first_event, 89); // RANDR_EVENT_BASE
    }
}

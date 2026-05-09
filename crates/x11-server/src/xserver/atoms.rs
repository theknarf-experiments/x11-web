use std::collections::HashMap;

/// Named atom IDs from the predefined-atom table below. Add an entry here
/// before referencing the atom by number anywhere outside `atoms.rs`.
///
/// The values are wire-stable: the X11 spec defines IDs 1..68 and EWMH /
/// ICCCM extensions follow our own assignment from 69 upward. The
/// `predefined_consts_match_table` test guards every entry against drift.
pub(crate) mod predef {
    /// Per the X11 spec, atom 31 is the predefined `STRING` type used for
    /// Latin-1 textual property values.
    pub(crate) const STRING: u32 = 31;
    pub(crate) const NET_WM_WINDOW_TYPE: u32 = 79;
    pub(crate) const NET_WM_WINDOW_TYPE_NORMAL: u32 = 80;
    pub(crate) const NET_WM_WINDOW_TYPE_DIALOG: u32 = 81;
    pub(crate) const NET_WM_WINDOW_TYPE_TOOLBAR: u32 = 82;
    pub(crate) const NET_WM_WINDOW_TYPE_MENU: u32 = 83;
    pub(crate) const NET_WM_WINDOW_TYPE_UTILITY: u32 = 84;
    pub(crate) const NET_WM_WINDOW_TYPE_SPLASH: u32 = 85;
    pub(crate) const NET_WM_WINDOW_TYPE_DOCK: u32 = 86;
    pub(crate) const NET_WM_WINDOW_TYPE_DESKTOP: u32 = 87;
    pub(crate) const NET_WM_WINDOW_TYPE_DROPDOWN_MENU: u32 = 88;
    pub(crate) const NET_WM_WINDOW_TYPE_POPUP_MENU: u32 = 89;
    pub(crate) const NET_WM_WINDOW_TYPE_TOOLTIP: u32 = 90;
    pub(crate) const NET_WM_WINDOW_TYPE_NOTIFICATION: u32 = 91;
    pub(crate) const NET_WM_STATE: u32 = 92;
    pub(crate) const NET_WM_STATE_ABOVE: u32 = 102;
    pub(crate) const NET_WM_STATE_BELOW: u32 = 103;
    pub(crate) const NET_WM_STRUT: u32 = 129;
    pub(crate) const NET_WM_STRUT_PARTIAL: u32 = 130;
}

pub(crate) struct AtomManager {
    atoms: HashMap<String, u32>,
    reverse: HashMap<u32, String>,
    next_atom: u32,
}

impl AtomManager {
    pub(crate) fn new() -> Self {
        let mut mgr = Self {
            atoms: HashMap::new(),
            reverse: HashMap::new(),
            next_atom: 1,
        };
        for (name, id) in PREDEFINED_ATOMS {
            mgr.atoms.insert(name.to_string(), *id);
            mgr.reverse.insert(*id, name.to_string());
            if *id >= mgr.next_atom {
                mgr.next_atom = *id + 1;
            }
        }
        mgr
    }

    pub(crate) fn intern(&mut self, name: &str, only_if_exists: bool) -> u32 {
        if let Some(&id) = self.atoms.get(name) {
            return id;
        }
        if only_if_exists {
            return 0;
        }
        let id = self.next_atom;
        self.next_atom += 1;
        self.atoms.insert(name.to_string(), id);
        self.reverse.insert(id, name.to_string());
        id
    }

    pub(crate) fn get_name(&self, atom: u32) -> Option<&str> {
        self.reverse.get(&atom).map(|s| s.as_str())
    }
}

pub(crate) const PREDEFINED_ATOMS: &[(&str, u32)] = &[
    ("PRIMARY", 1),
    ("SECONDARY", 2),
    ("ARC", 3),
    ("ATOM", 4),
    ("BITMAP", 5),
    ("CARDINAL", 6),
    ("COLORMAP", 7),
    ("CURSOR", 8),
    ("CUT_BUFFER0", 9),
    ("CUT_BUFFER1", 10),
    ("CUT_BUFFER2", 11),
    ("CUT_BUFFER3", 12),
    ("CUT_BUFFER4", 13),
    ("CUT_BUFFER5", 14),
    ("CUT_BUFFER6", 15),
    ("CUT_BUFFER7", 16),
    ("DRAWABLE", 17),
    ("FONT", 18),
    ("INTEGER", 19),
    ("PIXMAP", 20),
    ("POINT", 21),
    ("RECTANGLE", 22),
    ("RESOURCE_MANAGER", 23),
    ("RGB_COLOR_MAP", 24),
    ("RGB_BEST_MAP", 25),
    ("RGB_BLUE_MAP", 26),
    ("RGB_DEFAULT_MAP", 27),
    ("RGB_GRAY_MAP", 28),
    ("RGB_GREEN_MAP", 29),
    ("RGB_RED_MAP", 30),
    ("STRING", 31),
    ("VISUALID", 32),
    ("WINDOW", 33),
    ("WM_COMMAND", 34),
    ("WM_HINTS", 35),
    ("WM_CLIENT_MACHINE", 36),
    ("WM_ICON_NAME", 37),
    ("WM_ICON_SIZE", 38),
    ("WM_NAME", 39),
    ("WM_NORMAL_HINTS", 40),
    ("WM_SIZE_HINTS", 41),
    ("WM_ZOOM_HINTS", 42),
    ("MIN_SPACE", 43),
    ("NORM_SPACE", 44),
    ("MAX_SPACE", 45),
    ("END_SPACE", 46),
    ("SUPERSCRIPT_X", 47),
    ("SUPERSCRIPT_Y", 48),
    ("SUBSCRIPT_X", 49),
    ("SUBSCRIPT_Y", 50),
    ("UNDERLINE_POSITION", 51),
    ("UNDERLINE_THICKNESS", 52),
    ("STRIKEOUT_ASCENT", 53),
    ("STRIKEOUT_DESCENT", 54),
    ("ITALIC_ANGLE", 55),
    ("X_HEIGHT", 56),
    ("QUAD_WIDTH", 57),
    ("WEIGHT", 58),
    ("POINT_SIZE", 59),
    ("RESOLUTION", 60),
    ("COPYRIGHT", 61),
    ("NOTICE", 62),
    ("FONT_NAME", 63),
    ("FAMILY_NAME", 64),
    ("FULL_NAME", 65),
    ("CAP_HEIGHT", 66),
    ("WM_CLASS", 67),
    ("WM_TRANSIENT_FOR", 68),
    // ICCCM atoms
    ("WM_PROTOCOLS", 69),
    ("WM_DELETE_WINDOW", 70),
    ("WM_TAKE_FOCUS", 71),
    ("WM_STATE", 72),
    ("WM_CHANGE_STATE", 73),
    ("WM_COLORMAP_WINDOWS", 74),
    // EWMH atoms
    ("_NET_SUPPORTED", 75),
    ("_NET_SUPPORTING_WM_CHECK", 76),
    ("_NET_WM_NAME", 77),
    ("_NET_WM_ICON_NAME", 78),
    ("_NET_WM_WINDOW_TYPE", 79),
    ("_NET_WM_WINDOW_TYPE_NORMAL", 80),
    ("_NET_WM_WINDOW_TYPE_DIALOG", 81),
    ("_NET_WM_WINDOW_TYPE_TOOLBAR", 82),
    ("_NET_WM_WINDOW_TYPE_MENU", 83),
    ("_NET_WM_WINDOW_TYPE_UTILITY", 84),
    ("_NET_WM_WINDOW_TYPE_SPLASH", 85),
    ("_NET_WM_WINDOW_TYPE_DOCK", 86),
    ("_NET_WM_WINDOW_TYPE_DESKTOP", 87),
    ("_NET_WM_WINDOW_TYPE_DROPDOWN_MENU", 88),
    ("_NET_WM_WINDOW_TYPE_POPUP_MENU", 89),
    ("_NET_WM_WINDOW_TYPE_TOOLTIP", 90),
    ("_NET_WM_WINDOW_TYPE_NOTIFICATION", 91),
    ("_NET_WM_STATE", 92),
    ("_NET_WM_STATE_MODAL", 93),
    ("_NET_WM_STATE_STICKY", 94),
    ("_NET_WM_STATE_MAXIMIZED_VERT", 95),
    ("_NET_WM_STATE_MAXIMIZED_HORZ", 96),
    ("_NET_WM_STATE_SHADED", 97),
    ("_NET_WM_STATE_SKIP_TASKBAR", 98),
    ("_NET_WM_STATE_SKIP_PAGER", 99),
    ("_NET_WM_STATE_HIDDEN", 100),
    ("_NET_WM_STATE_FULLSCREEN", 101),
    ("_NET_WM_STATE_ABOVE", 102),
    ("_NET_WM_STATE_BELOW", 103),
    ("_NET_WM_STATE_DEMANDS_ATTENTION", 104),
    ("_NET_WM_STATE_FOCUSED", 105),
    ("_NET_WM_ALLOWED_ACTIONS", 106),
    ("_NET_WM_ACTION_MOVE", 107),
    ("_NET_WM_ACTION_RESIZE", 108),
    ("_NET_WM_ACTION_MINIMIZE", 109),
    ("_NET_WM_ACTION_SHADE", 110),
    ("_NET_WM_ACTION_STICK", 111),
    ("_NET_WM_ACTION_MAXIMIZE_HORZ", 112),
    ("_NET_WM_ACTION_MAXIMIZE_VERT", 113),
    ("_NET_WM_ACTION_FULLSCREEN", 114),
    ("_NET_WM_ACTION_CHANGE_DESKTOP", 115),
    ("_NET_WM_ACTION_CLOSE", 116),
    ("_NET_ACTIVE_WINDOW", 117),
    ("_NET_CLIENT_LIST", 118),
    ("_NET_CLIENT_LIST_STACKING", 119),
    ("_NET_NUMBER_OF_DESKTOPS", 120),
    ("_NET_CURRENT_DESKTOP", 121),
    ("_NET_DESKTOP_NAMES", 122),
    ("_NET_WORKAREA", 123),
    ("_NET_DESKTOP_GEOMETRY", 124),
    ("_NET_DESKTOP_VIEWPORT", 125),
    ("_NET_FRAME_EXTENTS", 126),
    ("_NET_WM_PID", 127),
    ("_NET_WM_USER_TIME", 128),
    ("_NET_WM_STRUT", 129),
    ("_NET_WM_STRUT_PARTIAL", 130),
    ("_NET_WM_ICON", 131),
    ("_NET_WM_VISIBLE_NAME", 132),
    ("UTF8_STRING", 133),
    ("CLIPBOARD", 134),
    ("TARGETS", 135),
    ("MULTIPLE", 136),
    ("TIMESTAMP", 137),
    ("INCR", 138),
    ("_NET_WM_PING", 139),
    ("_NET_WM_SYNC_REQUEST", 140),
    ("_MOTIF_WM_HINTS", 141),
    // XDND (X Drag and Drop) atoms
    ("XdndAware", 142),
    ("XdndSelection", 143),
    ("XdndEnter", 144),
    ("XdndLeave", 145),
    ("XdndPosition", 146),
    ("XdndDrop", 147),
    ("XdndFinished", 148),
    ("XdndStatus", 149),
    ("XdndActionCopy", 150),
    ("XdndActionMove", 151),
    ("XdndActionLink", 152),
    ("XdndActionAsk", 153),
    ("XdndActionPrivate", 154),
    ("XdndTypeList", 155),
    ("XdndProxy", 156),
    // XIM (X Input Method) protocol atoms
    ("_XIM_PROTOCOL", 157),
    ("_XIM_XCONNECT", 158),
    ("XIM_SERVERS", 159),
    ("LOCALES", 160),
    ("TRANSPORT", 161),
    ("_XIM_MOREDATA", 162),
    // XSETTINGS (toolkit configuration)
    ("_XSETTINGS_SETTINGS", 163),
    ("_XSETTINGS_S0", 164),
    ("MANAGER", 165),
    // Additional ICCCM/EWMH atoms for broader app compatibility
    ("_NET_WM_WINDOW_OPACITY", 166),
    ("_NET_WM_MOVERESIZE", 167),
    ("_NET_REQUEST_FRAME_EXTENTS", 168),
    ("_NET_WM_FULL_PLACEMENT", 169),
    ("_NET_STARTUP_ID", 170),
    ("_NET_WM_DESKTOP", 171),
    ("_NET_CLOSE_WINDOW", 172),
    ("_NET_MOVERESIZE_WINDOW", 173),
    ("_NET_RESTACK_WINDOW", 174),
    ("_NET_WM_FULLSCREEN_MONITORS", 175),
    ("_NET_WM_CM_S0", 176),
    ("_XEMBED", 177),
    ("_XEMBED_INFO", 178),
    // Clipboard manager atoms
    ("CLIPBOARD_MANAGER", 179),
    ("SAVE_TARGETS", 180),
    // Type atoms used by properties
    ("COMPOUND_TEXT", 181),
    ("TEXT", 182),
    ("text/plain", 183),
    ("text/plain;charset=utf-8", 184),
    // Compose/dead key support
    ("_XKB_RULES_NAMES", 185),
    // System tray (freedesktop.org System Tray Protocol)
    ("_NET_SYSTEM_TRAY_S0", 186),
    ("_NET_SYSTEM_TRAY_OPCODE", 187),
    ("_NET_SYSTEM_TRAY_ORIENTATION", 188),
    ("_NET_SYSTEM_TRAY_VISUAL", 189),
    // ICCCM selection targets
    ("DELETE", 190),
    ("INSERT_SELECTION", 191),
    ("INSERT_PROPERTY", 192),
];

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Predef constants must match the wire IDs in PREDEFINED_ATOMS
    // -----------------------------------------------------------------------

    #[test]
    fn predefined_consts_match_table() {
        let lookup: HashMap<&str, u32> =
            PREDEFINED_ATOMS.iter().map(|&(n, i)| (n, i)).collect();
        let pairs: &[(&str, u32)] = &[
            ("STRING", predef::STRING),
            ("_NET_WM_WINDOW_TYPE", predef::NET_WM_WINDOW_TYPE),
            ("_NET_WM_WINDOW_TYPE_NORMAL", predef::NET_WM_WINDOW_TYPE_NORMAL),
            ("_NET_WM_WINDOW_TYPE_DIALOG", predef::NET_WM_WINDOW_TYPE_DIALOG),
            ("_NET_WM_WINDOW_TYPE_TOOLBAR", predef::NET_WM_WINDOW_TYPE_TOOLBAR),
            ("_NET_WM_WINDOW_TYPE_MENU", predef::NET_WM_WINDOW_TYPE_MENU),
            ("_NET_WM_WINDOW_TYPE_UTILITY", predef::NET_WM_WINDOW_TYPE_UTILITY),
            ("_NET_WM_WINDOW_TYPE_SPLASH", predef::NET_WM_WINDOW_TYPE_SPLASH),
            ("_NET_WM_WINDOW_TYPE_DOCK", predef::NET_WM_WINDOW_TYPE_DOCK),
            ("_NET_WM_WINDOW_TYPE_DESKTOP", predef::NET_WM_WINDOW_TYPE_DESKTOP),
            (
                "_NET_WM_WINDOW_TYPE_DROPDOWN_MENU",
                predef::NET_WM_WINDOW_TYPE_DROPDOWN_MENU,
            ),
            ("_NET_WM_WINDOW_TYPE_POPUP_MENU", predef::NET_WM_WINDOW_TYPE_POPUP_MENU),
            ("_NET_WM_WINDOW_TYPE_TOOLTIP", predef::NET_WM_WINDOW_TYPE_TOOLTIP),
            (
                "_NET_WM_WINDOW_TYPE_NOTIFICATION",
                predef::NET_WM_WINDOW_TYPE_NOTIFICATION,
            ),
            ("_NET_WM_STATE", predef::NET_WM_STATE),
            ("_NET_WM_STATE_ABOVE", predef::NET_WM_STATE_ABOVE),
            ("_NET_WM_STATE_BELOW", predef::NET_WM_STATE_BELOW),
            ("_NET_WM_STRUT", predef::NET_WM_STRUT),
            ("_NET_WM_STRUT_PARTIAL", predef::NET_WM_STRUT_PARTIAL),
        ];
        for &(name, value) in pairs {
            assert_eq!(
                lookup.get(name).copied(),
                Some(value),
                "predef::{name} must equal PREDEFINED_ATOMS entry",
            );
        }
    }

    // -----------------------------------------------------------------------
    // Basic intern / lookup
    // -----------------------------------------------------------------------

    #[test]
    fn intern_new_atom_returns_nonzero_id() {
        let mut mgr = AtomManager::new();
        let id = mgr.intern("MY_CUSTOM_ATOM", false);
        assert_ne!(id, 0, "intern of a new atom must return a non-zero ID");
    }

    #[test]
    fn intern_same_name_returns_same_id() {
        let mut mgr = AtomManager::new();
        let id1 = mgr.intern("DUPLICATE_NAME", false);
        let id2 = mgr.intern("DUPLICATE_NAME", false);
        assert_eq!(
            id1, id2,
            "interning the same name twice must return the same ID"
        );
    }

    #[test]
    fn intern_different_names_return_different_ids() {
        let mut mgr = AtomManager::new();
        let id1 = mgr.intern("FIRST_ATOM", false);
        let id2 = mgr.intern("SECOND_ATOM", false);
        assert_ne!(id1, id2, "distinct names must get distinct IDs");
    }

    #[test]
    fn intern_incrementing_ids() {
        let mut mgr = AtomManager::new();
        // The pre-defined atoms go up to 185, so new IDs start at 186
        let id1 = mgr.intern("NEW_A", false);
        let id2 = mgr.intern("NEW_B", false);
        let id3 = mgr.intern("NEW_C", false);
        // IDs must be strictly increasing
        assert!(id1 < id2, "IDs must be monotonically increasing");
        assert!(id2 < id3, "IDs must be monotonically increasing");
    }

    #[test]
    fn get_name_returns_correct_name() {
        let mut mgr = AtomManager::new();
        let id = mgr.intern("HELLO_WORLD", false);
        assert_eq!(mgr.get_name(id), Some("HELLO_WORLD"));
    }

    #[test]
    fn get_name_unknown_id_returns_none() {
        let mgr = AtomManager::new();
        // 0 is never a valid atom ID in X11
        assert_eq!(mgr.get_name(0), None);
        // A very large ID that was never interned
        assert_eq!(mgr.get_name(999_999), None);
    }

    // -----------------------------------------------------------------------
    // only_if_exists flag
    // -----------------------------------------------------------------------

    #[test]
    fn intern_only_if_exists_returns_zero_for_missing_atom() {
        let mut mgr = AtomManager::new();
        let id = mgr.intern("NONEXISTENT_ATOM", true);
        assert_eq!(
            id, 0,
            "only_if_exists=true must return 0 when atom does not exist"
        );
    }

    #[test]
    fn intern_only_if_exists_returns_id_for_existing_atom() {
        let mut mgr = AtomManager::new();
        let created = mgr.intern("PREEXISTING", false);
        let found = mgr.intern("PREEXISTING", true);
        assert_eq!(
            created, found,
            "only_if_exists=true must return the existing ID"
        );
    }

    // -----------------------------------------------------------------------
    // Pre-registered standard atoms
    // -----------------------------------------------------------------------

    #[test]
    fn primary_atom_has_id_1() {
        let mgr = AtomManager::new();
        assert_eq!(mgr.get_name(1), Some("PRIMARY"));
    }

    #[test]
    fn secondary_atom_has_id_2() {
        let mgr = AtomManager::new();
        assert_eq!(mgr.get_name(2), Some("SECONDARY"));
    }

    #[test]
    fn atom_atom_has_id_4() {
        let mgr = AtomManager::new();
        assert_eq!(mgr.get_name(4), Some("ATOM"));
    }

    #[test]
    fn string_atom_has_id_31() {
        let mgr = AtomManager::new();
        assert_eq!(mgr.get_name(31), Some("STRING"));
    }

    #[test]
    fn wm_name_atom_has_id_39() {
        let mgr = AtomManager::new();
        assert_eq!(mgr.get_name(39), Some("WM_NAME"));
    }

    #[test]
    fn wm_class_atom_has_id_67() {
        let mgr = AtomManager::new();
        assert_eq!(mgr.get_name(67), Some("WM_CLASS"));
    }

    #[test]
    fn wm_protocols_atom_has_id_69() {
        let mgr = AtomManager::new();
        assert_eq!(mgr.get_name(69), Some("WM_PROTOCOLS"));
    }

    #[test]
    fn wm_delete_window_atom_has_id_70() {
        let mgr = AtomManager::new();
        assert_eq!(mgr.get_name(70), Some("WM_DELETE_WINDOW"));
    }

    #[test]
    fn utf8_string_atom_has_id_133() {
        let mgr = AtomManager::new();
        assert_eq!(mgr.get_name(133), Some("UTF8_STRING"));
    }

    #[test]
    fn clipboard_atom_has_id_134() {
        let mgr = AtomManager::new();
        assert_eq!(mgr.get_name(134), Some("CLIPBOARD"));
    }

    #[test]
    fn net_wm_name_atom_has_id_77() {
        let mgr = AtomManager::new();
        assert_eq!(mgr.get_name(77), Some("_NET_WM_NAME"));
    }

    #[test]
    fn intern_predefined_atom_returns_correct_id() {
        let mut mgr = AtomManager::new();
        // Interning a predefined atom must return its fixed ID, not a new one
        assert_eq!(mgr.intern("PRIMARY", false), 1);
        assert_eq!(mgr.intern("STRING", false), 31);
        assert_eq!(mgr.intern("WM_NAME", false), 39);
        assert_eq!(mgr.intern("UTF8_STRING", false), 133);
    }

    // -----------------------------------------------------------------------
    // Case sensitivity
    // -----------------------------------------------------------------------

    #[test]
    fn atom_names_are_case_sensitive() {
        let mut mgr = AtomManager::new();
        let upper = mgr.intern("MYATOM", false);
        let lower = mgr.intern("myatom", false);
        let mixed = mgr.intern("MyAtom", false);
        // All three must be distinct atoms
        assert_ne!(
            upper, lower,
            "atom names must be case-sensitive: MYATOM vs myatom"
        );
        assert_ne!(
            upper, mixed,
            "atom names must be case-sensitive: MYATOM vs MyAtom"
        );
        assert_ne!(
            lower, mixed,
            "atom names must be case-sensitive: myatom vs MyAtom"
        );
    }

    #[test]
    fn predefined_atom_lookup_is_case_sensitive() {
        let mut mgr = AtomManager::new();
        // "string" (lowercase) is NOT the same as predefined "STRING"
        let lowercase_id = mgr.intern("string", false);
        let uppercase_id = mgr.intern("STRING", false);
        assert_ne!(
            lowercase_id, uppercase_id,
            "'string' and 'STRING' must be different atoms"
        );
        assert_eq!(
            uppercase_id, 31,
            "predefined 'STRING' must still have ID 31"
        );
    }

    // -----------------------------------------------------------------------
    // All predefined atoms are registered correctly
    // -----------------------------------------------------------------------

    #[test]
    fn all_predefined_atoms_are_retrievable() {
        let mgr = AtomManager::new();
        for &(name, id) in PREDEFINED_ATOMS {
            assert_eq!(
                mgr.get_name(id),
                Some(name),
                "predefined atom '{name}' (ID {id}) not retrievable by ID"
            );
        }
    }

    #[test]
    fn all_predefined_atoms_are_interneable() {
        let mut mgr = AtomManager::new();
        for &(name, expected_id) in PREDEFINED_ATOMS {
            assert_eq!(
                mgr.intern(name, false),
                expected_id,
                "interning predefined atom '{name}' must return ID {expected_id}"
            );
        }
    }

    #[test]
    fn predefined_atoms_have_unique_ids() {
        let mut seen_ids = std::collections::HashSet::new();
        let mut seen_names = std::collections::HashSet::new();
        for &(name, id) in PREDEFINED_ATOMS {
            assert!(
                seen_ids.insert(id),
                "predefined atom ID {id} ('{name}') is duplicated"
            );
            assert!(
                seen_names.insert(name),
                "predefined atom name '{name}' is duplicated"
            );
        }
    }

    #[test]
    fn new_atoms_do_not_collide_with_predefined() {
        let mut mgr = AtomManager::new();
        let custom_id = mgr.intern("Z_BRAND_NEW_ATOM", false);
        // Must not coincide with any predefined atom ID
        let predefined_ids: std::collections::HashSet<u32> =
            PREDEFINED_ATOMS.iter().map(|&(_, id)| id).collect();
        assert!(
            !predefined_ids.contains(&custom_id),
            "new atom ID {custom_id} collides with a predefined atom"
        );
    }
}

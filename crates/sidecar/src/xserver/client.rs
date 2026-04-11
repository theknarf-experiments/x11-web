//! Per-connection client state for X11.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use uuid::Uuid;
use x11_web_protocol::DisplayUpdate;

use crate::framebuffer::Framebuffer;
use crate::fonts::FontManager;

use super::atoms::AtomManager;
use super::grab::GrabState;
use super::types::*;

/// Per-connection state for an X11 client.
pub(crate) struct ClientState {
    pub(crate) client_id: String,
    pub(crate) sequence: u16,
    pub(crate) windows: HashMap<u32, WindowState>,
    pub(crate) shared_windows: SharedWindows,
    pub(crate) pixmaps: HashMap<u32, PixmapState>,
    pub(crate) gcs: HashMap<u32, GcState>,
    pub(crate) atoms: Arc<Mutex<AtomManager>>,
    pub(crate) update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    pub(crate) root_window: u32,
    pub(crate) pointer_x: i16,
    pub(crate) pointer_y: i16,
    pub(crate) focus_window: u32,
    pub(crate) font_manager: FontManager,
    pub(crate) render: crate::render::RenderState,
    pub(crate) selections: HashMap<u32, u32>,
    pub(crate) shm_segments: HashMap<u32, ShmSegment>,
    pub(crate) wm_state: SharedWmState,
    pub(crate) wm_events_tx: mpsc::UnboundedSender<Vec<u8>>,
    pub(crate) damage_regions: HashMap<u32, DamageInfo>,
    pub(crate) present_subscriptions: HashMap<u32, PresentSubscription>,
    pub(crate) pending_events: Vec<Vec<u8>>,
    pub(crate) window_router: WindowRouter,
    pub(crate) message_tx: mpsc::UnboundedSender<(u32, WindowMessage)>,
    pub(crate) x11_to_uuid: HashMap<u32, String>,
    pub(crate) cursors: HashMap<u32, String>,
    pub(crate) xi: crate::xinput2::XiState,
    pub(crate) menu_tracker: crate::menus::MenuTracker,
    pub(crate) gtk_menu_paths: HashMap<u32, crate::menus::GtkMenuPaths>,
    /// Grab state: pointer grabs, keyboard grabs, passive grabs.
    pub(crate) grabs: GrabState,
    /// Close-down mode for this client (0=Destroy, 1=RetainPermanent, 2=RetainTemporary).
    pub(crate) close_down_mode: u8,
    /// Currently pressed keys (for QueryKeymap).
    pub(crate) pressed_keys: [u8; 32],
    /// Keyboard control settings.
    pub(crate) keyboard_control: KeyboardControl,
    /// Pointer control settings.
    pub(crate) pointer_control: PointerControl,
    /// Screen saver settings.
    pub(crate) screen_saver: ScreenSaverSettings,
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
}

impl ClientState {
    /// Get or create a UUID for a top-level X11 window.
    pub(crate) fn get_or_create_window_uuid(&mut self, x11_wid: u32) -> String {
        if let Some(uuid) = self.x11_to_uuid.get(&x11_wid) {
            return uuid.clone();
        }
        let uuid = Uuid::new_v4().to_string();
        self.x11_to_uuid.insert(x11_wid, uuid.clone());
        self.window_router.register(&uuid, x11_wid, &self.message_tx);
        self.menu_tracker
            .window_index()
            .register(x11_wid, uuid.clone(), self.client_id.clone());
        uuid
    }

    /// Get the UUID for a window. Returns None if the window was never registered.
    pub(crate) fn window_uuid(&self, x11_wid: u32) -> Option<String> {
        self.x11_to_uuid.get(&x11_wid).cloned()
    }

    /// Walk the parent chain from `x11_wid` to find the nearest top-level
    /// window that has a registered UUID.
    pub(crate) fn top_level_uuid_for(&self, x11_wid: u32) -> Option<String> {
        if x11_wid == 0 || x11_wid == self.root_window {
            return None;
        }
        let mut current = x11_wid;
        for _ in 0..32 {
            if let Some(uuid) = self.x11_to_uuid.get(&current) {
                return Some(uuid.clone());
            }
            match self.windows.get(&current) {
                Some(w) if w.parent != self.root_window && w.parent != 0 => {
                    current = w.parent;
                }
                _ => return None,
            }
        }
        None
    }

    /// Update the focus window and broadcast if changed.
    pub(crate) fn set_focus_window(&mut self, new_focus: u32) {
        let prev_uuid = self.top_level_uuid_for(self.focus_window);
        self.focus_window = new_focus;
        let next_uuid = self.top_level_uuid_for(new_focus);
        if prev_uuid != next_uuid {
            self.broadcast_focus(next_uuid);
        }
    }

    /// Send a `WindowFocused` update to the frontend.
    pub(crate) fn broadcast_focus(&self, window_id: Option<String>) {
        let _ = self.update_tx.send((
            self.client_id.clone(),
            DisplayUpdate::WindowFocused { window_id },
        ));
    }

    /// Intern an atom name, returning its global ID.
    pub(crate) fn intern_atom(&self, name: &str, only_if_exists: bool) -> u32 {
        self.atoms.lock().unwrap().intern(name, only_if_exists)
    }

    /// Get the name of an atom by its global ID.
    pub(crate) fn get_atom_name(&self, atom: u32) -> Option<String> {
        self.atoms.lock().unwrap().get_name(atom).map(|s| s.to_string())
    }

    /// Map a color value for the drawable's depth.
    pub(crate) fn map_color_for_drawable(&self, drawable: u32, color: u32) -> u32 {
        let depth = self.pixmaps.get(&drawable).map(|p| p.depth).unwrap_or(24);
        if depth <= 1 {
            if color != 0 { 0xFFFFFF } else { 0x000000 }
        } else {
            color
        }
    }

    /// Get a mutable reference to the framebuffer for a drawable.
    pub(crate) fn get_framebuffer_mut(&mut self, drawable: u32) -> Option<&mut Framebuffer> {
        let target = self.resolve_drawable(drawable);
        if let Some(win) = self.windows.get_mut(&target) {
            return Some(&mut win.framebuffer);
        }
        if let Some(pix) = self.pixmaps.get_mut(&target) {
            return Some(&mut pix.framebuffer);
        }
        None
    }

    /// Queue DamageNotify events for subscriptions on the given drawable.
    pub(crate) fn notify_damage(&mut self, drawable: u32, x: i16, y: i16, width: u16, height: u16) {
        let resolved = self.resolve_drawable(drawable);
        let matches: Vec<(u32, u8)> = self
            .damage_regions
            .iter()
            .filter(|(_, info)| info.drawable == resolved)
            .map(|(&did, info)| (did, info.level))
            .collect();

        for (damage_id, level) in matches {
            let mut event = [0u8; 32];
            event[0] = 91;
            event[1] = level;
            event[2..4].copy_from_slice(&self.sequence.to_le_bytes());
            event[4..8].copy_from_slice(&resolved.to_le_bytes());
            event[8..12].copy_from_slice(&damage_id.to_le_bytes());
            event[14..16].copy_from_slice(&(x as u16).to_le_bytes());
            event[16..18].copy_from_slice(&(y as u16).to_le_bytes());
            event[18..20].copy_from_slice(&width.to_le_bytes());
            event[20..22].copy_from_slice(&height.to_le_bytes());
            event[22..24].copy_from_slice(&(x as u16).to_le_bytes());
            event[24..26].copy_from_slice(&(y as u16).to_le_bytes());
            event[26..28].copy_from_slice(&width.to_le_bytes());
            event[28..30].copy_from_slice(&height.to_le_bytes());
            self.pending_events.push(event.to_vec());
        }
    }

    /// Resolve a drawable ID to the actual drawable ID (following aliases).
    pub(crate) fn resolve_drawable(&self, drawable: u32) -> u32 {
        if let Some(pix) = self.pixmaps.get(&drawable) {
            if let Some(alias_wid) = pix.alias_window {
                return alias_wid;
            }
        }
        drawable
    }

    /// Sync SHM-backed pixmap data before reading.
    pub(crate) fn sync_shm_pixmap(&mut self, drawable: u32) {
        let target = self.resolve_drawable(drawable);
        let (shmseg, offset, width, height) = {
            let pix = match self.pixmaps.get(&target) {
                Some(p) => p,
                None => return,
            };
            let backing = match &pix.shm_backing {
                Some(b) => b,
                None => return,
            };
            (backing.shmseg, backing.offset, pix.width as usize, pix.height as usize)
        };

        let seg = match self.shm_segments.get(&shmseg) {
            Some(s) => s,
            None => {
                tracing::warn!("sync_shm_pixmap: SHM segment {shmseg} not found for pixmap {target:#x}");
                return;
            }
        };

        let bpp = 4usize;
        let stride = width * bpp;
        let total_bytes = stride * height;

        if offset + total_bytes > seg.size {
            tracing::warn!(
                "sync_shm_pixmap: out of bounds (offset={offset} + size={total_bytes} > seg.size={})",
                seg.size
            );
            return;
        }

        let pix = self.pixmaps.get_mut(&target).unwrap();
        let fb = &mut pix.framebuffer;
        let fb_data = fb.data_mut();
        unsafe {
            let src_ptr = seg.addr.add(offset);
            let copy_len = total_bytes.min(fb_data.len());
            std::ptr::copy_nonoverlapping(src_ptr, fb_data.as_mut_ptr(), copy_len);
        }
    }

    /// Sync local windows with the shared store.
    pub(crate) fn sync_windows(&mut self) {
        if let Ok(mut shared) = self.shared_windows.lock() {
            for (&wid, shared_win) in shared.iter() {
                if let Some(local_win) = self.windows.get_mut(&wid) {
                    if shared_win.mapped && !local_win.mapped {
                        local_win.mapped = true;
                    }
                    if shared_win.redirected {
                        local_win.redirected = true;
                    }
                    for (&atom, val) in shared_win.properties.iter() {
                        if !local_win.properties.contains_key(&atom) {
                            local_win.properties.insert(atom, val.clone());
                        }
                    }
                } else {
                    self.windows.insert(wid, shared_win.clone());
                }
            }

            for (&wid, local_win) in self.windows.iter() {
                if let Some(shared_win) = shared.get_mut(&wid) {
                    if local_win.mapped {
                        shared_win.mapped = true;
                    }
                    if local_win.redirected {
                        shared_win.redirected = true;
                    }
                    for (&atom, val) in local_win.properties.iter() {
                        shared_win.properties.insert(atom, val.clone());
                    }
                    shared_win.x = local_win.x;
                    shared_win.y = local_win.y;
                    shared_win.width = local_win.width;
                    shared_win.height = local_win.height;
                } else {
                    shared.insert(wid, local_win.clone());
                }
            }

            let shared_ids: Vec<u32> = shared.keys().copied().collect();
            for wid in shared_ids {
                if !self.windows.contains_key(&wid) {
                    shared.remove(&wid);
                }
            }
        }
    }

    /// Send dirty framebuffer regions for all mapped windows as PutImage updates.
    pub(crate) fn flush_dirty_windows(&mut self) {
        let children: Vec<(u32, u16, u16)> = self
            .windows
            .iter()
            .filter(|(_, w)| {
                w.mapped
                    && w.framebuffer.is_dirty()
                    && w.parent != self.root_window
                    && w.parent != 0
                    && w.class == 1
            })
            .map(|(_, w)| (w.id, w.width, w.height))
            .collect();

        for (child_id, cw, ch) in &children {
            let mut target = *child_id;
            let mut off_x: i32 = 0;
            let mut off_y: i32 = 0;
            for _ in 0..10 {
                let (parent, wx, wy) = match self.windows.get(&target) {
                    Some(w) if w.parent != self.root_window && w.parent != 0 => {
                        (w.parent, w.x as i32, w.y as i32)
                    }
                    _ => break,
                };
                off_x += wx;
                off_y += wy;
                target = parent;
            }

            if target == *child_id {
                continue;
            }

            let pixels = if let Some(child) = self.windows.get(child_id) {
                Some(child.framebuffer.extract_pixels(0, 0, *cw, *ch))
            } else {
                None
            };

            if let Some(pixels) = pixels {
                if let Some(ancestor) = self.windows.get_mut(&target) {
                    ancestor.framebuffer.put_image(off_x as i16, off_y as i16, *cw, *ch, &pixels);
                }
            }

            if let Some(child) = self.windows.get_mut(child_id) {
                child.framebuffer.clear_dirty();
            }
        }

        let window_ids: Vec<u32> = self
            .windows
            .iter()
            .filter(|(_, w)| {
                w.mapped
                    && w.framebuffer.is_dirty()
                    && w.parent == self.root_window
                    && w.class == 1
            })
            .map(|(id, _)| *id)
            .collect();

        for wid in window_ids {
            let Some(wid_str) = self.window_uuid(wid) else { continue };
            if let Some(win) = self.windows.get_mut(&wid) {
                if let Some((x, y, w, h, pixels)) = win.framebuffer.take_dirty_pixels() {
                    let owner = if win.owner_client_id.is_empty() {
                        self.client_id.clone()
                    } else {
                        win.owner_client_id.clone()
                    };
                    let _ = self.update_tx.send((
                        owner,
                        DisplayUpdate::PutImage {
                            window_id: wid_str.clone(),
                            x,
                            y,
                            width: w,
                            height: h,
                            data: pixels,
                        },
                    ));

                    let win_width = win.width;
                    let win_height = win.height;
                    let damage_matches: Vec<(u32, u8)> = self
                        .damage_regions
                        .iter()
                        .filter(|(_, info)| info.drawable == wid)
                        .map(|(&did, info)| (did, info.level))
                        .collect();

                    for (damage_id, level) in damage_matches {
                        let mut event = [0u8; 32];
                        event[0] = 91;
                        event[1] = level;
                        event[2..4].copy_from_slice(&self.sequence.to_le_bytes());
                        event[4..8].copy_from_slice(&wid.to_le_bytes());
                        event[8..12].copy_from_slice(&damage_id.to_le_bytes());
                        event[14..16].copy_from_slice(&(x as u16).to_le_bytes());
                        event[16..18].copy_from_slice(&(y as u16).to_le_bytes());
                        event[18..20].copy_from_slice(&w.to_le_bytes());
                        event[20..22].copy_from_slice(&h.to_le_bytes());
                        event[26..28].copy_from_slice(&win_width.to_le_bytes());
                        event[28..30].copy_from_slice(&win_height.to_le_bytes());
                        self.pending_events.push(event.to_vec());
                    }
                }
            }
        }
    }
}

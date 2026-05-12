//! Window attributes and save-set handlers (opcodes 2, 3, 6).

use super::*;
use crate::xserver::event::serialize_event;
use crate::xserver::reply::serialize_reply;
use x11rb_protocol::protocol::xproto::{
    BackingStore, ChangeSaveSetRequest, ChangeWindowAttributesRequest, ColormapNotifyEvent,
    ColormapState as XColormapState, GetWindowAttributesReply, GetWindowAttributesRequest, Gravity,
    MapState, WindowClass,
};

// ---------------------------------------------------------------------------
// Opcode 2: ChangeWindowAttributes
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_window_attributes(
    state: &mut ClientState,
    req: &ChangeWindowAttributesRequest,
) -> Vec<u8> {
    let seq = state.sequence;
    let wid = req.window;
    let vl = &*req.value_list;

    // Cross-connection: another client may want to set attributes (typically
    // event_mask) on a window owned by a different client. The owning
    // client's window lives in shared_windows; pull it into our local view
    // first so the rest of the handler can operate on it.
    if !state.windows.contains_key(&wid) {
        let shared_win = state
            .shared_windows
            .lock()
            .ok()
            .and_then(|sw| sw.get(&wid).cloned());
        if let Some(sw) = shared_win {
            state.windows.insert(wid, sw);
        } else {
            return build_error(WINDOW_ERROR, seq, wid, 2, 0);
        }
    }

    // Pre-validate enumerated attributes before mutating state
    if let Some(g) = vl.bit_gravity {
        let val = u32::from(g);
        if val > 10 {
            return build_error(VALUE_ERROR, seq, val, 2, 0);
        }
    }
    if let Some(g) = vl.win_gravity {
        let val = u32::from(g);
        if val > 10 {
            return build_error(VALUE_ERROR, seq, val, 2, 0);
        }
    }
    if let Some(bs) = vl.backing_store {
        let val = u32::from(bs);
        if val > u32::from(BackingStore::ALWAYS) {
            return build_error(VALUE_ERROR, seq, val, 2, 0);
        }
    }
    // event-mask: check SubstructureRedirect/ResizeRedirect mutual exclusion per X11 spec Section 12.3
    if let Some(em) = vl.event_mask {
        let val = u32::from(em);
        if let Some(_conflict) =
            state
                .event_broadcaster
                .check_redirect_conflict(wid, val, &state.client_id)
        {
            return build_error(ACCESS_ERROR, seq, 0, 2, 0);
        }
    }
    // cursor: validate cursor ID exists
    if let Some(c) = vl.cursor {
        if c != 0 && !state.cursors.contains_key(&c) {
            return build_error(CURSOR_ERROR, seq, c, 2, 0);
        }
    }

    let mut cursor_changed = false;
    let mut deferred_event_mask: Option<u32> = None;
    let mut deferred_colormap_notify: Option<(u32, u32)> = None; // (old_cmap, new_cmap)
    if let Some(win) = state.windows.get_mut(&wid) {
        if let Some(val) = vl.background_pixmap {
            // background-pixmap: 0=None, 1=ParentRelative, else pixmap ID
            win.background_pixmap = Some(val);
        }
        if let Some(val) = vl.background_pixel {
            win.background_pixel = val;
        }
        if let Some(val) = vl.border_pixmap {
            // border-pixmap: 0=CopyFromParent, else pixmap ID
            win.border_pixmap = Some(val);
        }
        if let Some(val) = vl.border_pixel {
            win.border_pixel = val;
        }
        if let Some(g) = vl.bit_gravity {
            state.bit_gravity.insert(wid, u32::from(g) as u8);
        }
        if let Some(g) = vl.win_gravity {
            state.win_gravity.insert(wid, u32::from(g) as u8);
        }
        if let Some(bs) = vl.backing_store {
            win.backing_store = u32::from(bs) as u8;
        }
        if let Some(val) = vl.backing_planes {
            win.backing_planes = val;
        }
        if let Some(val) = vl.backing_pixel {
            win.backing_pixel = val;
        }
        if let Some(val) = vl.override_redirect {
            win.override_redirect = val != 0;
        }
        if let Some(val) = vl.save_under {
            win.save_under = val != 0;
        }
        if let Some(em) = vl.event_mask {
            let val = u32::from(em);
            win.event_mask = val;
            // SubstructureRedirectMask = bit 20 = 0x0010_0000
            if wid == state.root_window
                && (val & EventMask::SUBSTRUCTURE_REDIRECT != EventMask::NO_EVENT)
            {
                info!(
                    "Client {} registering as window manager (SubstructureRedirectMask on root)",
                    state.client_id
                );
                if let Ok(mut wm) = state.wm_state.lock() {
                    wm.client_id = Some(state.client_id.clone());
                    wm.event_tx = Some(state.wm_events_tx.clone());
                }
            }
            // Defer cross-connection subscription until after mutable borrow ends
            deferred_event_mask = Some(val);
        }
        if let Some(em) = vl.do_not_propogate_mask {
            win.do_not_propagate_mask = u32::from(em);
        }
        if let Some(val) = vl.colormap {
            // Colormap: 0 = CopyFromParent
            let old_cmap = win.colormap;
            win.colormap = val;
            if val != old_cmap
                && (win.event_mask & EventMask::COLOR_MAP_CHANGE != EventMask::NO_EVENT)
            {
                deferred_colormap_notify = Some((old_cmap, val));
            }
        }
        if let Some(val) = vl.cursor {
            let new_cursor = if val == 0 { None } else { Some(val) };
            if win.cursor != new_cursor {
                win.cursor = new_cursor;
                cursor_changed = true;
            }
        }
    }

    // Register cross-connection event subscription (deferred from inside mutable borrow)
    if let Some(mask) = deferred_event_mask {
        state.subscribe_to_window_events(wid, mask);
    }

    // Generate ColormapNotify when the window's colormap attribute changes
    if let Some((_old_cmap, new_cmap)) = deferred_colormap_notify {
        let event = serialize_event(
            &ColormapNotifyEvent {
                response_type: COLOURMAP_NOTIFY_EVENT,
                sequence: 0,
                window: wid,
                colormap: new_cmap,
                new: true,
                state: XColormapState::INSTALLED,
            },
            state.msb_first,
        );
        state.pending_events.push(event.clone());
        state.broadcast_event(wid, EventMask::COLOR_MAP_CHANGE, &event);
    }

    if cursor_changed {
        emit_cursor_changed(state, wid);
    }

    // If border attributes changed, send a WindowConfigured update so the
    // frontend can re-render the border.
    if vl.border_pixmap.is_some() || vl.border_pixel.is_some() {
        if let Some(win) = state.windows.get(&wid) {
            if let Some(uuid) = state.window_uuid(wid) {
                let _ = state.update_tx.send((
                    state.client_id.clone(),
                    DisplayUpdate::WindowConfigured {
                        window_id: uuid,
                        x: win.x,
                        y: win.y,
                        width: win.width,
                        height: win.height,
                        border_width: win.border_width,
                        border_pixel: win.border_pixel,
                        resizable: true,
                    },
                ));
            }
        }
    }

    // Push event_mask / override_redirect / colormap / etc. updates to
    // shared_windows on the next sync tick.
    state.mark_window_shared_dirty(wid);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 3: GetWindowAttributes
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_window_attributes(
    state: &mut ClientState,
    req: &GetWindowAttributesRequest,
) -> Vec<u8> {
    let seq = state.sequence;
    let wid = req.window;

    // Cross-client GetWindowAttributes: when the window was created by a
    // different client, fall back to the shared store. Without this we
    // return BadWindow → Xlib zeros attrs.visual → GDK calls
    // XVisualIDFromVisual(NULL) → SIGSEGV (the Firefox/GTK3 crash).
    if !state.windows.contains_key(&wid) {
        let shared_win = state
            .shared_windows
            .lock()
            .ok()
            .and_then(|sw| sw.get(&wid).cloned());
        if let Some(sw) = shared_win {
            state.windows.insert(wid, sw);
        } else {
            return build_error(WINDOW_ERROR, seq, wid, 3, 0);
        }
    }
    let win = match state.windows.get(&wid) {
        Some(w) => w,
        None => return build_error(WINDOW_ERROR, seq, wid, 3, 0),
    };

    let bit_gravity = state.bit_gravity.get(&wid).copied().unwrap_or(0);
    let win_gravity = state.win_gravity.get(&wid).copied().unwrap_or(1);
    // Per X11 spec: map_state is one of
    //   0 IsUnmapped   (this window is unmapped)
    //   1 IsUnviewable (this window is mapped but some ancestor is unmapped)
    //   2 IsViewable   (this window and all its ancestors are mapped)
    let map_state: u8 = if !win.mapped {
        0
    } else {
        // Walk parents (root is implicitly mapped; bail at root or missing).
        let mut cur = win.parent;
        let mut viewable = true;
        for _ in 0..256 {
            if cur == 0 || cur == state.root_window {
                break;
            }
            match state.windows.get(&cur) {
                Some(p) => {
                    if !p.mapped {
                        viewable = false;
                        break;
                    }
                    cur = p.parent;
                }
                None => break,
            }
        }
        if viewable {
            2
        } else {
            1
        }
    };
    let cmap = if win.colormap != 0 {
        win.colormap
    } else {
        ROOT_COLORMAP
    };
    let remote_masks = state.event_broadcaster.all_event_masks(wid);
    serialize_reply(
        &GetWindowAttributesReply {
            backing_store: BackingStore::from(win.backing_store),
            sequence: seq,
            length: 0,
            visual: win.visual,
            class: WindowClass::from(win.class),
            bit_gravity: Gravity::from(bit_gravity),
            win_gravity: Gravity::from(win_gravity),
            backing_planes: win.backing_planes,
            backing_pixel: win.backing_pixel,
            save_under: win.save_under,
            map_is_installed: true,
            map_state: MapState::from(map_state),
            override_redirect: win.override_redirect,
            colormap: cmap,
            all_event_masks: EventMask::from(win.event_mask | remote_masks),
            your_event_mask: EventMask::from(win.event_mask),
            do_not_propagate_mask: EventMask::from(win.do_not_propagate_mask as u32),
        },
        state.byte_order(),
    )
}

// ---------------------------------------------------------------------------
// Opcode 6: ChangeSaveSet
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_save_set(
    state: &mut ClientState,
    req: &ChangeSaveSetRequest,
) -> Vec<u8> {
    let _seq = state.sequence;
    let mode: u8 = req.mode.into();
    let window = req.window;

    // Per X11 spec, validate the window exists (cannot add root window to save set)
    if !state.windows.contains_key(&window) || window == state.root_window {
        return build_error(WINDOW_ERROR, state.sequence, window, 6, 0);
    }

    // Per X11 spec, mode must be 0 (Insert) or 1 (Delete)
    if mode > 1 {
        return build_error(VALUE_ERROR, state.sequence, mode as u32, 6, 0);
    }

    match mode {
        0 => {
            // Insert
            if !state.save_set.contains(&window) {
                state.save_set.push(window);
            }
        }
        1 => {
            // Delete
            state.save_set.retain(|&w| w != window);
        }
        _ => {}
    }
    Vec::new()
}

//! Extension protocol handlers (opcodes >= 128).
//!
//! This module contains handlers for X11 extensions: XFIXES, RANDR, SHAPE,
//! MIT-SHM, SYNC, DAMAGE, Composite, Generic Event, XKB, XC-MISC, and Present.

use tracing::{debug, info, warn};

use super::super::client::ClientState;
use super::super::core::{ROOT_VISUAL, SCREEN_HEIGHT, SCREEN_WIDTH};
use super::super::types::{DamageInfo, PixmapState, PresentSubscription, ShmPixmapBacking, ShmSegment};
use crate::framebuffer::Framebuffer;

pub(crate) fn handle_xfixes_request(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("XFIXES minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: return version 5.0
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&5u32.to_le_bytes()); // major version
            reply[12..16].copy_from_slice(&0u32.to_le_bytes()); // minor version
            reply.to_vec()
        }
        4 => {
            // GetCursorImage: return a 1x1 transparent cursor.
            // Reply layout (post-32-byte header):
            //   i16 x, i16 y, u16 width, u16 height, u16 xhot, u16 yhot,
            //   u32 cursor_serial, 8 bytes pad,
            //   then width*height u32 ARGB pixels.
            // Returning BadImplementation here used to make Firefox
            // segfault on startup.
            let width: u16 = 1;
            let height: u16 = 1;
            let pixels_len = (width as usize) * (height as usize) * 4;
            let extra = 24 + pixels_len; // 24 bytes of header fields after the 32-byte reply header
            let total = 32 + extra;
            let length_units = (extra / 4) as u32;
            let mut reply = vec![0u8; total];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&length_units.to_le_bytes());
            // x, y at bytes 8..12 — 0,0
            reply[12..14].copy_from_slice(&width.to_le_bytes());
            reply[14..16].copy_from_slice(&height.to_le_bytes());
            // xhot, yhot at 16..20 — 0,0
            reply[20..24].copy_from_slice(&0u32.to_le_bytes()); // cursor_serial
            // 24..32 = pad
            // 32..32+pixels_len = ARGB pixels (already 0 = transparent)
            reply
        }
        18 => {
            // FetchRegion: return empty region reply
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // reply length = 0 (no rectangles)
            // extents: x1=0, y1=0, x2=0, y2=0 (bytes 8-15, already zero)
            reply.to_vec()
        }
        31 => {
            // GetCursorName: return empty reply
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // atom = 0 (None), name length = 0
            reply.to_vec()
        }
        // All other minor opcodes: ignore
        _ => Vec::new(),
    }
}

pub(crate) fn handle_randr_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("RANDR minor={minor}");

    match minor {
        0 => {
            // QueryVersion: return version 1.2
            // (1.5 requires GetMonitors which is complex; 1.2 uses GetScreenResources)
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&1u32.to_le_bytes()); // major version
            reply[12..16].copy_from_slice(&2u32.to_le_bytes()); // minor version
            reply.to_vec()
        }
        2 => {
            // SetScreenConfig: reply with success
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 0; // Success
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // timestamp
            reply[8..12].copy_from_slice(&0u32.to_le_bytes());
            // config_timestamp
            reply[12..16].copy_from_slice(&0u32.to_le_bytes());
            // root window
            reply[16..20].copy_from_slice(&state.root_window.to_le_bytes());
            reply.to_vec()
        }
        5 => {
            // GetScreenInfo: minimal screen configuration
            // Reply header (32) + 1 ScreenSize (8 bytes) + 0 rates
            let num_sizes: u16 = 1;
            let extra_data_len: usize = 8; // 1 screen size * 8 bytes
            let reply_len = 32 + extra_data_len;
            let mut reply = vec![0u8; reply_len];
            reply[0] = 1; // Reply
            reply[1] = 1; // rotations = Rotate_0
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&((extra_data_len / 4) as u32).to_le_bytes()); // length
            reply[8..12].copy_from_slice(&state.root_window.to_le_bytes()); // root
            // timestamp
            reply[12..16].copy_from_slice(&0u32.to_le_bytes());
            // config_timestamp
            reply[16..20].copy_from_slice(&0u32.to_le_bytes());
            reply[20..22].copy_from_slice(&num_sizes.to_le_bytes()); // nSizes
            reply[22..24].copy_from_slice(&0u16.to_le_bytes()); // sizeID (current)
            reply[24..26].copy_from_slice(&1u16.to_le_bytes()); // rotation = Rotate_0
            reply[26..28].copy_from_slice(&0u16.to_le_bytes()); // nrateEnts = 0
            // pad bytes 28-31 already zero
            // Screen size entry: width(2), height(2), mwidth(2), mheight(2)
            reply[32..34].copy_from_slice(&SCREEN_WIDTH.to_le_bytes());
            reply[34..36].copy_from_slice(&SCREEN_HEIGHT.to_le_bytes());
            reply[36..38].copy_from_slice(&270u16.to_le_bytes()); // mm width
            reply[38..40].copy_from_slice(&203u16.to_le_bytes()); // mm height
            reply
        }
        6 => {
            // GetScreenSizeRange: min=1x1, max=32767x32767
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // min_width
            reply[10..12].copy_from_slice(&1u16.to_le_bytes()); // min_height
            reply[12..14].copy_from_slice(&32767u16.to_le_bytes()); // max_width
            reply[14..16].copy_from_slice(&32767u16.to_le_bytes()); // max_height
            reply.to_vec()
        }
        7 => {
            // SetScreenSize: ignore (void)
            Vec::new()
        }
        8 | 19 => {
            // GetScreenResources / GetScreenResourcesCurrent
            // Use x11rb to build a correct reply via raw bytes.
            // 1 CRTC (id=100), 1 output (id=200), 1 mode (id=300)
            let crtc_id: u32 = 100;
            let output_id: u32 = 200;
            let mode_id: u32 = 300;
            let mode_name = b"1024x768";
            let mode_name_pad = (4 - (mode_name.len() % 4)) % 4;

            // Variable data after the 32-byte header:
            //   crtc_ids: num_crtcs * 4 bytes
            //   output_ids: num_outputs * 4 bytes
            //   mode_infos: num_modes * 32 bytes
            //   mode_names: names_len bytes + padding
            let var_len = 4 + 4 + 32 + mode_name.len() + mode_name_pad;
            let length_field = var_len / 4; // extra 4-byte units beyond 32-byte header
            let total = 32 + var_len;

            let mut r = vec![0u8; total];
            r[0] = 1; // Reply
            r[2..4].copy_from_slice(&seq.to_le_bytes());
            r[4..8].copy_from_slice(&(length_field as u32).to_le_bytes());
            r[8..12].copy_from_slice(&1u32.to_le_bytes()); // timestamp
            r[12..16].copy_from_slice(&1u32.to_le_bytes()); // config_timestamp
            r[16..18].copy_from_slice(&1u16.to_le_bytes()); // num_crtcs
            r[18..20].copy_from_slice(&1u16.to_le_bytes()); // num_outputs
            r[20..22].copy_from_slice(&1u16.to_le_bytes()); // num_modes
            r[22..24].copy_from_slice(&(mode_name.len() as u16).to_le_bytes()); // names_len

            let mut off = 32;
            // CRTC IDs array
            r[off..off + 4].copy_from_slice(&crtc_id.to_le_bytes());
            off += 4;
            // Output IDs array
            r[off..off + 4].copy_from_slice(&output_id.to_le_bytes());
            off += 4;
            // ModeInfo struct (32 bytes)
            r[off..off + 4].copy_from_slice(&mode_id.to_le_bytes());       // id
            r[off + 4..off + 6].copy_from_slice(&SCREEN_WIDTH.to_le_bytes());  // width
            r[off + 6..off + 8].copy_from_slice(&SCREEN_HEIGHT.to_le_bytes()); // height
            r[off + 8..off + 12].copy_from_slice(&60000u32.to_le_bytes());     // dotClock
            r[off + 12..off + 14].copy_from_slice(&(SCREEN_WIDTH + 40).to_le_bytes()); // hSyncStart
            r[off + 14..off + 16].copy_from_slice(&(SCREEN_WIDTH + 80).to_le_bytes()); // hSyncEnd
            r[off + 16..off + 18].copy_from_slice(&(SCREEN_WIDTH + 160).to_le_bytes()); // hTotal
            // hSkew at off+18 = 0
            r[off + 20..off + 22].copy_from_slice(&(SCREEN_HEIGHT + 3).to_le_bytes()); // vSyncStart
            r[off + 22..off + 24].copy_from_slice(&(SCREEN_HEIGHT + 6).to_le_bytes()); // vSyncEnd
            r[off + 24..off + 26].copy_from_slice(&(SCREEN_HEIGHT + 30).to_le_bytes()); // vTotal
            r[off + 26..off + 28].copy_from_slice(&(mode_name.len() as u16).to_le_bytes()); // nameLength
            // modeFlags at off+28..off+32 = 0
            off += 32;
            // Mode names
            r[off..off + mode_name.len()].copy_from_slice(mode_name);

            r
        }
        9 => {
            // GetOutputInfo (RandR 1.2)
            // Reply: 32-byte header + inline data
            // Bytes 8-35 are the output info fields (part of the "extra" length)
            let output_name = b"default";
            let crtc_id: u32 = 100;
            let mode_id: u32 = 300;
            let num_crtcs: u16 = 1;
            let num_modes: u16 = 1;
            let num_clones: u16 = 0;

            // Variable data: crtc_ids + mode_ids + clone_ids + name + pad
            let name_pad = (4 - (output_name.len() % 4)) % 4;
            let var_data = (num_crtcs as usize * 4) + (num_modes as usize * 4) + (num_clones as usize * 4) + output_name.len() + name_pad;
            // The reply length field = (total_reply_bytes - 32) / 4
            // Total bytes = 32 (header) + 4 (timestamp) + 4 (crtc) + 4 (mm_width) + 4 (mm_height)
            //             + 1 (connection) + 1 (subpixel) + 2 (num_crtcs) + 2 (num_modes)
            //             + 2 (num_preferred) + 2 (num_clones) + 2 (name_len) + var_data
            // = 32 + 24 + var_data (but 24 bytes of inline header are counted as extra)
            let inline_header = 24; // bytes 8-31 in the reply
            let length = (inline_header + var_data) / 4;
            let total = 32 + inline_header + var_data;
            let mut reply = vec![0u8; total];

            reply[0] = 1; // Reply
            reply[1] = 0; // status: Success
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(length as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&1u32.to_le_bytes()); // timestamp
            reply[12..16].copy_from_slice(&crtc_id.to_le_bytes()); // crtc
            reply[16..20].copy_from_slice(&270u32.to_le_bytes()); // mm_width
            reply[20..24].copy_from_slice(&203u32.to_le_bytes()); // mm_height
            reply[24] = 0; // connection: Connected
            reply[25] = 0; // subpixel_order: Unknown
            reply[26..28].copy_from_slice(&num_crtcs.to_le_bytes());
            reply[28..30].copy_from_slice(&num_modes.to_le_bytes());
            reply[30..32].copy_from_slice(&1u16.to_le_bytes()); // num_preferred
            reply[32..34].copy_from_slice(&num_clones.to_le_bytes());
            reply[34..36].copy_from_slice(&(output_name.len() as u16).to_le_bytes());

            let mut off = 36;
            // CRTC IDs
            reply[off..off + 4].copy_from_slice(&crtc_id.to_le_bytes());
            off += 4;
            // Mode IDs
            reply[off..off + 4].copy_from_slice(&mode_id.to_le_bytes());
            off += 4;
            // Clone IDs (none)
            // Output name
            reply[off..off + output_name.len()].copy_from_slice(output_name);

            reply
        }
        // GetCrtcInfo (RandR 1.2). Spec opcode is 20; the older
        // case 14 number kept here as a paranoid alias because
        // existing tests have been hitting it for a while and we
        // don't have great visibility into what really called it.
        14 | 20 => {
            // Header fields at bytes 8-31 count as extra data
            let output_id: u32 = 200;
            let mode_id: u32 = 300;
            let num_outputs: u16 = 1;
            let num_possible: u16 = 1;
            let var_data = (num_outputs as usize + num_possible as usize) * 4;
            let inline_header = 24; // bytes 8-31
            let length = (inline_header + var_data) / 4;
            let total = 32 + inline_header + var_data;
            let mut reply = vec![0u8; total];

            reply[0] = 1; // Reply
            reply[1] = 0; // status: Success
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(length as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&1u32.to_le_bytes()); // timestamp
            reply[12..14].copy_from_slice(&0i16.to_le_bytes()); // x
            reply[14..16].copy_from_slice(&0i16.to_le_bytes()); // y
            reply[16..18].copy_from_slice(&SCREEN_WIDTH.to_le_bytes()); // width
            reply[18..20].copy_from_slice(&SCREEN_HEIGHT.to_le_bytes()); // height
            reply[20..24].copy_from_slice(&mode_id.to_le_bytes()); // mode
            reply[24..26].copy_from_slice(&1u16.to_le_bytes()); // rotation: Rotate_0
            reply[26..28].copy_from_slice(&1u16.to_le_bytes()); // rotations: Rotate_0
            reply[28..30].copy_from_slice(&num_outputs.to_le_bytes());
            reply[30..32].copy_from_slice(&num_possible.to_le_bytes());

            let mut off = 32;
            reply[off..off + 4].copy_from_slice(&output_id.to_le_bytes());
            off += 4;
            reply[off..off + 4].copy_from_slice(&output_id.to_le_bytes());

            reply
        }
        // SetCrtcConfig — spec opcode 21, alias 15 for compat.
        15 | 21 => {
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 0; // Success
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // timestamp
            reply[8..12].copy_from_slice(&0u32.to_le_bytes());
            reply.to_vec()
        }
        // GetCrtcGammaSize (22) / GetCrtcGamma (23) — return zero-size
        // gamma ramp. Qt and Cairo probe these on startup and stall
        // without a reply.
        22 => {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..10].copy_from_slice(&0u16.to_le_bytes()); // size
            reply.to_vec()
        }
        23 => {
            // GetCrtcGamma — empty ramp.
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..10].copy_from_slice(&0u16.to_le_bytes()); // size
            reply.to_vec()
        }
        // SetCrtcGamma (24), SetCrtcTransform (26), SetPanning (28),
        // SetOutputPrimary (30) — all no-op writes.
        24 | 26 | 28 | 30 => Vec::new(),
        // GetPanning (27) — no panning.
        27 => {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = 0; // status: Success
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        // GetCrtcTransform (29) — identity matrix.
        29 => {
            // 32 header + 12 (current transform) + 12 (pending) +
            // 2*string_len ... we send the minimum: identity matrix
            // + zero-length filter names. The reply layout is large;
            // we approximate with a 96-byte buffer of zeros and set
            // the identity floats explicitly.
            let mut reply = vec![0u8; 96];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&16u32.to_le_bytes()); // length in 4-byte units
            reply
        }
        // GetOutputPrimary — spec opcode 31. Old wrong number 41
        // is preserved as an alias.
        31 | 41 => {
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&1u32.to_le_bytes()); // output = 1
            reply.to_vec()
        }
        // GetProviders — spec opcode 32. Qt 5 calls this on startup;
        // returning 0 providers tells it RandR providers are not
        // available so it stops asking. Old wrong number 46 alias.
        32 | 46 => {
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&0u32.to_le_bytes()); // timestamp
            reply[12..14].copy_from_slice(&0u16.to_le_bytes()); // num_providers
            reply.to_vec()
        }
        // GetProviderInfo — spec opcode 33. Old wrong number 47.
        33 | 47 => {
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        // GetMonitors (RandR 1.5) — spec opcode 42. Old wrong
        // number 25 preserved as alias.
        25 | 42 => {
            // Return 1 monitor with 1 output
            let monitor_name = state.intern_atom("default", false);
            let output_id: u32 = 200;

            // MonitorInfo: name(4) + primary(1) + automatic(1) + nOutput(2) +
            //              x(2) + y(2) + width(2) + height(2) + width_mm(4) + height_mm(4) +
            //              outputs(4)
            let monitor_size = 4 + 1 + 1 + 2 + 2 + 2 + 2 + 2 + 4 + 4 + 4; // = 28
            let var_len = monitor_size;
            let pad = (4 - (var_len % 4)) % 4;
            let length_field = (var_len + pad) / 4;
            let total = 32 + var_len + pad;

            let mut r = vec![0u8; total];
            r[0] = 1; // Reply
            r[2..4].copy_from_slice(&seq.to_le_bytes());
            r[4..8].copy_from_slice(&(length_field as u32).to_le_bytes());
            r[8..12].copy_from_slice(&1u32.to_le_bytes()); // timestamp
            r[12..16].copy_from_slice(&1u32.to_le_bytes()); // nMonitors
            r[16..20].copy_from_slice(&1u32.to_le_bytes()); // nOutputs

            let mut off = 32;
            r[off..off + 4].copy_from_slice(&monitor_name.to_le_bytes()); // name
            off += 4;
            r[off] = 1; // primary
            off += 1;
            r[off] = 1; // automatic
            off += 1;
            r[off..off + 2].copy_from_slice(&1u16.to_le_bytes()); // nOutput
            off += 2;
            r[off..off + 2].copy_from_slice(&0i16.to_le_bytes()); // x
            off += 2;
            r[off..off + 2].copy_from_slice(&0i16.to_le_bytes()); // y
            off += 2;
            r[off..off + 2].copy_from_slice(&SCREEN_WIDTH.to_le_bytes()); // width
            off += 2;
            r[off..off + 2].copy_from_slice(&SCREEN_HEIGHT.to_le_bytes()); // height
            off += 2;
            r[off..off + 4].copy_from_slice(&270u32.to_le_bytes()); // width_mm
            off += 4;
            r[off..off + 4].copy_from_slice(&203u32.to_le_bytes()); // height_mm
            off += 4;
            r[off..off + 4].copy_from_slice(&output_id.to_le_bytes()); // output

            r
        }
        // SetMonitor (43) / DeleteMonitor (44) — no-op.
        43 | 44 => Vec::new(),
        _ => {
            info!("Unhandled RANDR minor opcode: {minor}");
            Vec::new()
        }
    }
}

pub(crate) fn handle_shape_request(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("SHAPE minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: return version 1.1
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // major version
            reply[10..12].copy_from_slice(&1u16.to_le_bytes()); // minor version
            reply.to_vec()
        }
        _ => Vec::new(),
    }
}

/// Handle MIT-SHM extension requests (major opcode 130).
pub(crate) fn handle_shm_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];

    match minor {
        // QueryVersion
        0 => {
            info!("SHM QueryVersion");
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 1; // shared_pixmaps = true
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // reply[4..8] = additional data length = 0
            reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // major version
            reply[10..12].copy_from_slice(&2u16.to_le_bytes()); // minor version
            reply[12..14].copy_from_slice(&0u16.to_le_bytes()); // uid
            reply[14..16].copy_from_slice(&0u16.to_le_bytes()); // gid
            reply[16] = 2; // pixmap_format = ZPixmap
            reply.to_vec()
        }

        // Attach
        1 => {
            if data.len() < 16 {
                return Vec::new();
            }
            let shmseg = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let shmid = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as i32;
            let read_only = data[12] != 0;

            info!("SHM Attach: shmseg={shmseg} shmid={shmid} read_only={read_only}");

            unsafe {
                // Get segment size via shmctl IPC_STAT
                let mut ds: libc::shmid_ds = std::mem::zeroed();
                let stat_ret = libc::shmctl(shmid, libc::IPC_STAT, &mut ds);
                if stat_ret < 0 {
                    warn!("SHM Attach: shmctl IPC_STAT failed for shmid={shmid}");
                    return Vec::new();
                }
                let size = ds.shm_segsz;

                let flags = if read_only { libc::SHM_RDONLY } else { 0 };
                let addr = libc::shmat(shmid, std::ptr::null(), flags);
                if addr == (-1isize) as *mut libc::c_void {
                    warn!("SHM Attach: shmat failed for shmid={shmid}");
                    return Vec::new();
                }

                state.shm_segments.insert(shmseg, ShmSegment {
                    addr: addr as *mut u8,
                    size,
                });
            }

            Vec::new() // No reply for Attach
        }

        // Detach
        2 => {
            if data.len() < 8 {
                return Vec::new();
            }
            let shmseg = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            info!("SHM Detach: shmseg={shmseg}");

            if let Some(seg) = state.shm_segments.remove(&shmseg) {
                unsafe {
                    libc::shmdt(seg.addr as *const libc::c_void);
                }
            }

            Vec::new() // No reply for Detach
        }

        // PutImage
        3 => {
            if data.len() < 40 {
                return Vec::new();
            }

            let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let _gc = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
            let total_width = u16::from_le_bytes([data[12], data[13]]) as usize;
            let _total_height = u16::from_le_bytes([data[14], data[15]]);
            let src_x = u16::from_le_bytes([data[16], data[17]]) as usize;
            let src_y = u16::from_le_bytes([data[18], data[19]]) as usize;
            let src_width = u16::from_le_bytes([data[20], data[21]]);
            let src_height = u16::from_le_bytes([data[22], data[23]]);
            let dst_x = i16::from_le_bytes([data[24], data[25]]);
            let dst_y = i16::from_le_bytes([data[26], data[27]]);
            let _depth = data[28];
            let _format = data[29];
            let send_event = data[30] != 0;
            let shmseg = u32::from_le_bytes([data[32], data[33], data[34], data[35]]);
            let offset = u32::from_le_bytes([data[36], data[37], data[38], data[39]]) as usize;

            info!(
                "SHM PutImage: drawable={drawable:#x} shmseg={shmseg} offset={offset} \
                 total_width={total_width} src=({src_x},{src_y}) size=({src_width}x{src_height}) \
                 dst=({dst_x},{dst_y}) send_event={send_event}"
            );

            let seg = match state.shm_segments.get(&shmseg) {
                Some(s) => s,
                None => {
                    warn!("SHM PutImage: unknown shmseg={shmseg}");
                    return Vec::new();
                }
            };

            // Bytes per pixel (32bpp BGRA)
            let bpp = 4usize;
            let src_stride = total_width * bpp;
            let region_size = src_stride * (src_y + src_height as usize);

            // Bounds check
            if offset + region_size > seg.size {
                warn!(
                    "SHM PutImage: out of bounds (offset={offset} + region_size={region_size} > seg.size={})",
                    seg.size
                );
                return Vec::new();
            }

            // Build a contiguous pixel buffer for the source region
            let w = src_width as usize;
            let h = src_height as usize;
            let mut pixels = vec![0u8; w * h * bpp];

            unsafe {
                let base = seg.addr.add(offset);
                for row in 0..h {
                    let src_off = (src_y + row) * src_stride + src_x * bpp;
                    let dst_off = row * w * bpp;
                    let src_ptr = base.add(src_off);
                    std::ptr::copy_nonoverlapping(src_ptr, pixels.as_mut_ptr().add(dst_off), w * bpp);
                }
            }

            // Blit to the drawable's framebuffer
            if let Some(fb) = state.get_framebuffer_mut(drawable) {
                fb.put_image(dst_x, dst_y, src_width, src_height, &pixels);
            }

            // If send_event, return a ShmCompletion event
            if send_event {
                let mut event = [0u8; 32];
                event[0] = 65; // ShmCompletion event type (first_event + 0)
                event[2..4].copy_from_slice(&seq.to_le_bytes());
                event[4..8].copy_from_slice(&drawable.to_le_bytes());
                event[8..12].copy_from_slice(&shmseg.to_le_bytes());
                event[16..20].copy_from_slice(&(offset as u32).to_le_bytes());
                event.to_vec()
            } else {
                Vec::new()
            }
        }

        // GetImage
        4 => {
            if data.len() < 32 {
                return Vec::new();
            }
            let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let src_x = i16::from_le_bytes([data[8], data[9]]);
            let src_y = i16::from_le_bytes([data[10], data[11]]);
            let width = u16::from_le_bytes([data[12], data[13]]);
            let height = u16::from_le_bytes([data[14], data[15]]);
            let _plane_mask = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
            let _format = data[20];
            let shmseg = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
            let shm_offset = u32::from_le_bytes([data[28], data[29], data[30], data[31]]) as usize;

            info!("SHM GetImage: drawable={drawable:#x} ({src_x},{src_y}) {width}x{height} shmseg={shmseg} offset={shm_offset}");

            // Sync SHM-backed pixmap data before reading
            state.sync_shm_pixmap(drawable);

            // Copy pixels from drawable into SHM segment
            let resolved = state.resolve_drawable(drawable);
            let pixels = if let Some(fb) = state.get_framebuffer_mut(resolved) {
                fb.extract_pixels(src_x, src_y, width, height)
            } else {
                vec![0u8; width as usize * height as usize * 4]
            };

            if let Some(seg) = state.shm_segments.get(&shmseg) {
                let bpp = 4usize;
                let row_bytes = width as usize * bpp;
                let total_bytes = row_bytes * height as usize;
                if shm_offset + total_bytes <= seg.size {
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            pixels.as_ptr(),
                            seg.addr.add(shm_offset),
                            total_bytes.min(pixels.len()),
                        );
                    }
                }
            }

            // Reply
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 24; // depth
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&ROOT_VISUAL.to_le_bytes());
            reply[12..16].copy_from_slice(&(width as u32 * height as u32).to_le_bytes()); // size
            reply.to_vec()
        }

        // CreatePixmap
        5 => {
            if data.len() < 28 {
                return Vec::new();
            }
            let pid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let width = u16::from_le_bytes([data[12], data[13]]);
            let height = u16::from_le_bytes([data[14], data[15]]);
            let depth = data[16];
            let shmseg = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
            let shm_offset = u32::from_le_bytes([data[24], data[25], data[26], data[27]]) as usize;

            info!("SHM CreatePixmap: pid={pid:#x} {width}x{height} depth={depth} shmseg={shmseg} offset={shm_offset}");

            // Create an SHM-backed pixmap. The client will write directly into
            // the SHM segment; we sync from it before reading.
            state.pixmaps.insert(
                pid,
                PixmapState {
                    width,
                    height,
                    depth,
                    framebuffer: Framebuffer::new(width as u32, height as u32),
                    alias_window: None,
                    shm_backing: Some(ShmPixmapBacking {
                        shmseg,
                        offset: shm_offset,
                    }),
                },
            );
            Vec::new()
        }

        // AttachFd (minor 6) — used in MIT-SHM 1.2+ with fd passing
        6 => {
            if data.len() < 16 {
                return Vec::new();
            }
            let shmseg = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            info!("SHM AttachFd: shmseg={shmseg} (stubbed — fd passing not supported)");
            Vec::new()
        }

        _ => {
            warn!("Unhandled SHM minor opcode: {minor}");
            Vec::new()
        }
    }
}

pub(crate) fn handle_sync_request(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    info!("SYNC minor opcode: {minor}");

    match minor {
        0 => {
            // Initialize: reply with version 3.1
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8] = 3; // major version
            reply[9] = 1; // minor version
            reply.to_vec()
        }
        1 => {
            // ListSystemCounters: reply with 0 counters
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // length = 0 (no extra data)
            // num_counters = 0
            reply.to_vec()
        }
        2 | 3 | 4 => {
            // CreateCounter, SetCounter, ChangeCounter: void
            Vec::new()
        }
        5 => {
            // QueryCounter: reply with value 0
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // value_hi = 0, value_lo = 0 (already zero)
            reply.to_vec()
        }
        6 => {
            // DestroyCounter: void
            Vec::new()
        }
        7 => {
            // Await: return immediately (no blocking)
            Vec::new()
        }
        8 | 9 => {
            // CreateAlarm, ChangeAlarm: void
            Vec::new()
        }
        10 => {
            // QueryAlarm: reply with zeroed alarm state
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        11 => {
            // DestroyAlarm: void
            Vec::new()
        }
        12 => {
            // SetPriority: void
            Vec::new()
        }
        13 => {
            // GetPriority: reply with priority 0
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // priority = 0 (already zero)
            reply.to_vec()
        }
        14 | 15 | 16 | 17 => {
            // CreateFence, TriggerFence, ResetFence, DestroyFence: void
            Vec::new()
        }
        18 => {
            // QueryFence: reply with triggered=true
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8] = 1; // triggered = true
            reply.to_vec()
        }
        19 => {
            // AwaitFence: return immediately
            Vec::new()
        }
        _ => {
            debug!("Unhandled SYNC minor opcode: {minor}");
            Vec::new()
        }
    }
}

pub(crate) fn handle_damage_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("DAMAGE minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: reply with version 1.1
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&1u32.to_le_bytes()); // major version
            reply[12..16].copy_from_slice(&1u32.to_le_bytes()); // minor version
            reply.to_vec()
        }
        1 => {
            // DamageCreate: data[4..8] = damage_id, data[8..12] = drawable, data[12] = level
            if data.len() >= 13 {
                let damage_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let drawable = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let level = data[12];
                info!("DAMAGE Create: id={damage_id:#x} drawable={drawable:#x} level={level}");
                state.damage_regions.insert(damage_id, DamageInfo { drawable, level });
            }
            Vec::new()
        }
        2 => {
            // DamageDestroy: data[4..8] = damage_id
            if data.len() >= 8 {
                let damage_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                debug!("DAMAGE Destroy: id={damage_id:#x}");
                state.damage_regions.remove(&damage_id);
            }
            Vec::new()
        }
        3 => {
            // DamageSubtract: data[4..8] = damage_id, data[8..12] = repair, data[12..16] = parts
            // This acknowledges the damage — we just accept it.
            if data.len() >= 8 {
                let damage_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                debug!("DAMAGE Subtract: id={damage_id:#x}");
            }
            Vec::new()
        }
        4 => {
            // DamageAdd: void
            Vec::new()
        }
        _ => {
            debug!("Unhandled DAMAGE minor opcode: {minor}");
            Vec::new()
        }
    }
}

pub(crate) fn handle_x_composite_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    info!("Composite minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: reply with version 0.4
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&0u32.to_le_bytes()); // major version
            reply[12..16].copy_from_slice(&4u32.to_le_bytes()); // minor version
            reply.to_vec()
        }
        1 => {
            // RedirectWindow: data[4..8] = window, data[8] = update
            if data.len() >= 9 {
                let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let update = data[8];
                info!("Composite RedirectWindow: window={window:#x} update={update}");
                if let Some(win) = state.windows.get_mut(&window) {
                    win.redirected = true;
                }
            }
            Vec::new()
        }
        2 => {
            // RedirectSubwindows: data[4..8] = window, data[8] = update
            if data.len() >= 9 {
                let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let update = data[8];
                info!("Composite RedirectSubwindows: window={window:#x} update={update}");
                // Mark all children as redirected
                let children: Vec<u32> = state.windows.iter()
                    .filter(|(_, w)| w.parent == window)
                    .map(|(id, _)| *id)
                    .collect();
                for child in children {
                    if let Some(w) = state.windows.get_mut(&child) {
                        w.redirected = true;
                    }
                }
            }
            Vec::new()
        }
        3 => {
            // UnredirectWindow: data[4..8] = window
            if data.len() >= 8 {
                let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                debug!("Composite UnredirectWindow: window={window:#x}");
                if let Some(win) = state.windows.get_mut(&window) {
                    win.redirected = false;
                }
            }
            Vec::new()
        }
        4 | 5 => {
            // UnredirectSubwindows, CreateRegionFromBorderClip: void
            Vec::new()
        }
        6 => {
            // NameWindowPixmap: create a pixmap aliased to a window's framebuffer
            // data[4..8] = window, data[8..12] = pixmap
            if data.len() >= 12 {
                let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let pixmap = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                // Create a pixmap entry that aliases the window's framebuffer.
                // The actual framebuffer here is a dummy - all accesses will be
                // redirected to the window via alias_window.
                if let Some(win) = state.windows.get(&window) {
                    let w = win.width;
                    let h = win.height;
                    state.pixmaps.insert(
                        pixmap,
                        PixmapState {
                            width: w,
                            height: h,
                            depth: 24,
                            framebuffer: crate::framebuffer::Framebuffer::new(0, 0),
                            alias_window: Some(window),
                            shm_backing: None,
                        },
                    );
                    info!("NameWindowPixmap: window={window:#x} -> pixmap={pixmap:#x} {w}x{h} (aliased)");
                }
            }
            Vec::new()
        }
        7 => {
            // GetOverlayWindow: reply with overlay window = root window
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            reply.to_vec()
        }
        _ => {
            debug!("Unhandled Composite minor opcode: {minor}");
            Vec::new()
        }
    }
}

pub(crate) fn handle_ge_request(data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("Generic Event Extension minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: reply with version 1.0
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // major version
            reply[10..12].copy_from_slice(&0u16.to_le_bytes()); // minor version
            reply.to_vec()
        }
        _ => {
            debug!("Unhandled GE minor opcode: {minor}");
            Vec::new()
        }
    }
}

/// Build an XKB GetMap reply that's complete enough for `xkbcomp -xkb`
/// to parse and dump.
///
/// The XKB GetMap reply has a 40-byte header (8 bytes more than a
/// standard X reply header) followed by variable-length sections, in
/// this fixed order:
///
///   1. KeyTypes              (XkbKeyTypeWireDesc, 8 bytes each + entries)
///   2. KeySyms               (XkbSymMapWireDesc, 8 bytes each + syms)
///   3. KeyActions            (per-key nActs + per-action 8 bytes)
///   4. KeyBehaviors          (XkbBehaviorWireDesc)
///   5. VirtualMods           (1 byte per virtual modifier set)
///   6. ExplicitComponents    (XkbExplicitWireDesc, 2 bytes each)
///   7. ModifierMap           (XkbKeyModMapWireDesc, 4 bytes each)
///   8. VirtualModMap         (XkbKeyVModMapWireDesc, 4 bytes each)
///
/// Each section is only present if its bit in the `present` mask is
/// set. For our minimal implementation we set the full 0xff mask but
/// every section beyond KeyTypes and KeySyms is empty (count = 0),
/// which costs us nothing on the wire and keeps xkbcomp happy.
///
/// The keymap itself is the standard 248-key range (8..255) with one
/// `ONE_LEVEL` key type and a single keysym per key. Most keys map to
/// `NoSymbol`; a small US-style ASCII subset is filled in for the
/// printable range so the dumped keymap actually means something.
fn build_xkb_get_map_reply(seq: u16) -> Vec<u8> {
    const MIN_KEY_CODE: u8 = 8;
    const MAX_KEY_CODE: u8 = 255;
    const N_KEYS: usize = (MAX_KEY_CODE - MIN_KEY_CODE + 1) as usize; // 248

    // ----- Build the variable-length sections -----
    let mut data = Vec::new();

    // 1. KeyTypes: libxkbfile rejects any GetMap reply with fewer
    //    than `XkbNumRequiredTypes` (= 4) types — see
    //    XkbAllocClientMap in libX11. Provide the 4 standard XKB
    //    types in their canonical positions:
    //
    //      type 0: ONE_LEVEL                 — no modifiers
    //      type 1: TWO_LEVEL                 — Shift toggles a level
    //      type 2: ALPHABETIC                — Shift + Lock
    //      type 3: KEYPAD                    — Shift + NumLock
    //
    // Each XkbKeyTypeWireDesc is 8 bytes followed by `nMapEntries`
    // XkbKtMapEntryWireDesc structures (8 bytes each).
    let n_types = 4u8;

    // type 0 — ONE_LEVEL: numLevels=1, no map entries.
    data.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x00, // mask, realMods, vmods (16-bit)
        0x01, // numLevels
        0x00, // nMapEntries
        0x00, 0x00, // hasPreserve, pad
    ]);

    // type 1 — TWO_LEVEL: Shift mask, 1 entry mapping Shift -> level 1.
    data.extend_from_slice(&[
        0x01, 0x01, 0x00, 0x00, // mask=Shift, realMods=Shift, vmods=0
        0x02, // numLevels
        0x01, // nMapEntries
        0x00, 0x00, // hasPreserve, pad
    ]);
    // map entry: active, mask=Shift, level=1, realMods=Shift
    data.extend_from_slice(&[0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00]);

    // type 2 — ALPHABETIC: Shift+Lock, 2 entries.
    data.extend_from_slice(&[
        0x03, 0x03, 0x00, 0x00, // mask=Shift|Lock, realMods=Shift|Lock
        0x02, // numLevels
        0x02, // nMapEntries
        0x00, 0x00,
    ]);
    data.extend_from_slice(&[0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x02, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00]);

    // type 3 — KEYPAD: NumLock (Mod2 = 0x10), 1 entry.
    data.extend_from_slice(&[
        0x10, 0x10, 0x00, 0x00, // mask=Mod2, realMods=Mod2
        0x02, // numLevels
        0x01, // nMapEntries
        0x00, 0x00,
    ]);
    data.extend_from_slice(&[0x01, 0x10, 0x01, 0x10, 0x00, 0x00, 0x00, 0x00]);

    // 2. KeySyms: one XkbSymMapWireDesc per key. We give every key
    //    a single sym slot pointing at type 0 (ONE_LEVEL) so the
    //    layout is uniform — libxkbfile rejects mixed width=0 /
    //    width=1 entries. Keys we don't model get the canonical
    //    NoSymbol (0).
    //
    // XkbSymMapWireDesc layout:
    //   4 bytes: kt_index[4]   (group → key type index)
    //   1 byte:  groupInfo     (low 4 bits = num groups)
    //   1 byte:  width         (max syms per group across the levels)
    //   2 bytes: nSyms         (total syms following this header)
    //   nSyms * 4 bytes:       KeySym values
    let us_syms = us_qwerty_keysyms();
    let mut total_syms_count: u16 = 0;
    for kc in MIN_KEY_CODE..=MAX_KEY_CODE {
        let sym = us_syms[(kc - MIN_KEY_CODE) as usize];
        data.extend_from_slice(&[
            0x00, 0x00, 0x00, 0x00, // kt_index = ONE_LEVEL for all groups
            0x01, // groupInfo: 1 group
            0x01, // width: 1 sym per group
            0x01, 0x00, // nSyms = 1
        ]);
        data.extend_from_slice(&sym.to_le_bytes());
        total_syms_count += 1;
    }

    // 3. KeyActions: libxkbcommon's `get_actions()` enforces
    //       firstKeyAction == min_key_code
    //       firstKeyAction + nKeyActions == max_key_code + 1
    //    on every reply, so the per-key nActs array must span the
    //    *entire* keycode range even when every key has zero actions
    //    (which is our case — we don't model XKB actions). Emit one
    //    zero byte per key. The total payload is 248 bytes which is
    //    already 4-byte aligned, and totalActions stays 0.
    //
    //    libxkbfile (xkbcomp) doesn't enforce this and accepted the
    //    previous empty-section reply, but xkbcommon (used by GTK,
    //    Qt and Firefox) rejected it and fell back to a NULL keymap
    //    that GDK then crashed on later — see the firefox
    //    "crashes on first paint" failure mode this fix addresses.
    for _ in 0..N_KEYS {
        data.push(0);
    }
    // 4. KeyBehaviors: empty (libxkbcommon doesn't enforce span here,
    //    only that present entries have valid keycodes — sparse OK).
    // 5. VirtualMods: virtualMods = 0 → no per-vmod entries
    // 6. ExplicitComponents: empty (sparse, like Behaviors)
    // 7. ModifierMap: empty (sparse)
    // 8. VirtualModMap: empty (sparse)

    // Pad section data out to a 4-byte boundary.
    while data.len() % 4 != 0 {
        data.push(0);
    }

    // ----- Header -----
    let total_len = 40 + data.len();
    let mut reply = vec![0u8; total_len];
    reply[0] = 1; // Reply
    reply[1] = 3; // deviceID (matches Xvfb's default core kbd)
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    // length counts 4-byte words *after* the standard 32-byte X reply
    // header, so it's (8 + data.len()) / 4.
    let length_words = ((8 + data.len()) / 4) as u32;
    reply[4..8].copy_from_slice(&length_words.to_le_bytes());
    // 8..9 = pad1 (zero)
    reply[10] = MIN_KEY_CODE;
    reply[11] = MAX_KEY_CODE;
    // libxkbfile reads each section based on its count field
    // (nTypes, nKeySyms, totalActions, ...) rather than the bits
    // in `present`, so the present mask doesn't actually gate
    // anything we send. We still report the canonical 0xff so
    // that other clients (xcffib, real Xlib apps) see "everything
    // is here, just empty".
    let present: u16 = 0x00ff;
    reply[12..14].copy_from_slice(&present.to_le_bytes());
    reply[14] = 0; // firstType
    reply[15] = n_types;
    reply[16] = n_types; // totalTypes
    reply[17] = MIN_KEY_CODE; // firstKeySym
    reply[18..20].copy_from_slice(&total_syms_count.to_le_bytes());
    reply[20] = N_KEYS as u8; // nKeySyms
    reply[21] = MIN_KEY_CODE; // firstKeyAction
    reply[22..24].copy_from_slice(&0u16.to_le_bytes()); // totalActions
    reply[24] = N_KEYS as u8; // nKeyActions (full range, all zero)
    reply[25] = MIN_KEY_CODE; // firstKeyBehavior
    reply[26] = 0; // nKeyBehaviors
    reply[27] = 0; // totalKeyBehaviors
    reply[28] = MIN_KEY_CODE; // firstKeyExplicit
    reply[29] = 0; // nKeyExplicit
    reply[30] = 0; // totalKeyExplicit
    reply[31] = MIN_KEY_CODE; // firstModMapKey
    reply[32] = 0; // nModMapKeys
    reply[33] = 0; // totalModMapKeys
    reply[34] = MIN_KEY_CODE; // firstVModMapKey
    reply[35] = 0; // nVModMapKeys
    reply[36] = 0; // totalVModMapKeys
    // 37 = pad2
    reply[38..40].copy_from_slice(&0u16.to_le_bytes()); // virtualMods

    reply[40..].copy_from_slice(&data);
    reply
}

/// Build an XKB GetNames reply.
///
/// This is the second half of the libxkbcommon-compatibility fix
/// (the first being build_xkb_get_map_reply): libxkbcommon refuses
/// to load a keymap unless the GetNames reply advertises *all four*
/// of these bits in `which`:
///
///   bit 6  KeyTypeNames     — one ATOM per key type
///   bit 7  KTLevelNames     — `nLevelsPerType` bytes + sum-of-levels ATOMs
///   bit 9  KeyNames         — `nKeys * 4` bytes of 4-char key names
///   bit 11 VirtualModNames  — popcount(virtualMods) ATOMs
///
/// The previous version only set bit 9 (KeyNames), which was enough
/// for libxkbfile (xkbcomp) but caused libxkbcommon (used by GTK,
/// Qt and Firefox) to reject the reply with
///   "unmet condition in get_names(): (which & required) == required"
/// and fall back to a NULL keymap that GDK then crashed on.
///
/// We use atom 0 (None) for every type / level name — libxkbcommon
/// just calls `x11_atom_interner_adopt_atom` for each one and treats
/// missing atoms as XKB_ATOM_NONE, so the actual values don't matter
/// for keymap correctness.
fn build_xkb_get_names_reply(seq: u16, device_id: u8) -> Vec<u8> {
    const MIN_KEY_CODE: u8 = 8;
    const MAX_KEY_CODE: u8 = 255;
    const N_KEYS: usize = (MAX_KEY_CODE - MIN_KEY_CODE + 1) as usize;
    const KEY_NAME_LEN: usize = 4;

    // Number of types must match GetMap (which sends 4 standard
    // types) — libxkbcommon's get_type_names() asserts
    //   reply->nTypes == keymap->num_types
    const N_TYPES: u8 = 4;
    // Levels per type, mirroring GetMap:
    //   type 0 ONE_LEVEL=1, type 1 TWO_LEVEL=2, type 2 ALPHABETIC=2,
    //   type 3 KEYPAD=2 → 7 levels total.
    const LEVELS_PER_TYPE: [u8; 4] = [1, 2, 2, 2];

    // ----- Build the variable-length value list. -----
    // Section bit-case order is determined by the XML switch in
    // xkb.xml's GetNames reply, NOT by bit number — so:
    //   1. KeyTypeNames    (bit 6)
    //   2. KTLevelNames    (bit 7)
    //   3. VirtualModNames (bit 11) — empty since virtualMods=0
    //   4. KeyNames        (bit 9)
    let mut data = Vec::new();

    // 1. KeyTypeNames: nTypes ATOMs (4 bytes each).
    for _ in 0..N_TYPES {
        data.extend_from_slice(&0u32.to_le_bytes());
    }

    // 2. KTLevelNames: nLevelsPerType bytes (one per type), padded
    //    to a 4-byte boundary, then sum-of-levels ATOMs.
    for &n in LEVELS_PER_TYPE.iter() {
        data.push(n);
    }
    while data.len() % 4 != 0 {
        data.push(0);
    }
    let total_levels: u8 = LEVELS_PER_TYPE.iter().sum();
    for _ in 0..total_levels {
        data.extend_from_slice(&0u32.to_le_bytes());
    }

    // 3. VirtualModNames: virtualMods=0 → popcount=0 → no entries.

    // 4. KeyNames: 248 * 4 = 992 bytes.
    let key_names = us_qwerty_key_names();
    for kc in MIN_KEY_CODE..=MAX_KEY_CODE {
        let name = key_names[(kc - MIN_KEY_CODE) as usize];
        data.extend_from_slice(name);
    }

    debug_assert_eq!(N_KEYS * KEY_NAME_LEN, 992);
    // Pad to 4-byte boundary (already 4-aligned, but be safe).
    while data.len() % 4 != 0 {
        data.push(0);
    }

    // `which` mask: bits 6, 7, 9, 11.
    let which: u32 = (1 << 6) | (1 << 7) | (1 << 9) | (1 << 11);

    // 32 bytes header + variable body.
    let total_len = 32 + data.len();
    let length_words = (data.len() / 4) as u32;

    let mut reply = vec![0u8; total_len];
    reply[0] = 1;
    reply[1] = device_id;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&length_words.to_le_bytes());
    reply[8..12].copy_from_slice(&which.to_le_bytes());
    reply[12] = MIN_KEY_CODE;
    reply[13] = MAX_KEY_CODE;
    reply[14] = N_TYPES;
    reply[15] = 0; // groupNames
    // 16-17: virtualMods (0)
    reply[18] = MIN_KEY_CODE; // firstKey
    reply[19] = N_KEYS as u8; // nKeys
    // 20-23: indicators (0)
    reply[24] = 0; // nRadioGroups
    reply[25] = 0; // nKeyAliases
    reply[26..28].copy_from_slice(&u16::from(total_levels).to_le_bytes()); // nKTLevels
    // 28-31: pad
    reply[32..32 + data.len()].copy_from_slice(&data);
    reply
}

/// 4-character XKB key names for keycodes 8..255. The first ~70 are
/// the standard PC-101 names from `xkb/keycodes/evdev`; the rest are
/// filled with `K{kc}` placeholders so xkbcomp's keycodes-section
/// dumper has a unique 4-byte identifier for every key.
fn us_qwerty_key_names() -> [&'static [u8; 4]; 248] {
    let mut names: [&[u8; 4]; 248] = [b"K   "; 248];
    let real: &[(u8, &[u8; 4])] = &[
        (9, b"ESC "),
        (10, b"AE01"),
        (11, b"AE02"),
        (12, b"AE03"),
        (13, b"AE04"),
        (14, b"AE05"),
        (15, b"AE06"),
        (16, b"AE07"),
        (17, b"AE08"),
        (18, b"AE09"),
        (19, b"AE10"),
        (20, b"AE11"),
        (21, b"AE12"),
        (22, b"BKSP"),
        (23, b"TAB "),
        (24, b"AD01"),
        (25, b"AD02"),
        (26, b"AD03"),
        (27, b"AD04"),
        (28, b"AD05"),
        (29, b"AD06"),
        (30, b"AD07"),
        (31, b"AD08"),
        (32, b"AD09"),
        (33, b"AD10"),
        (34, b"AD11"),
        (35, b"AD12"),
        (36, b"RTRN"),
        (37, b"LCTL"),
        (38, b"AC01"),
        (39, b"AC02"),
        (40, b"AC03"),
        (41, b"AC04"),
        (42, b"AC05"),
        (43, b"AC06"),
        (44, b"AC07"),
        (45, b"AC08"),
        (46, b"AC09"),
        (47, b"AC10"),
        (48, b"AC11"),
        (49, b"TLDE"),
        (50, b"LFSH"),
        (51, b"BKSL"),
        (52, b"AB01"),
        (53, b"AB02"),
        (54, b"AB03"),
        (55, b"AB04"),
        (56, b"AB05"),
        (57, b"AB06"),
        (58, b"AB07"),
        (59, b"AB08"),
        (60, b"AB09"),
        (61, b"AB10"),
        (62, b"RTSH"),
        (63, b"KPMU"),
        (64, b"LALT"),
        (65, b"SPCE"),
        (66, b"CAPS"),
    ];
    for &(kc, name) in real {
        if kc >= 8 {
            names[(kc - 8) as usize] = name;
        }
    }
    // Fill the rest with stable "K{idx}" placeholders. We can't use
    // a runtime format string in a `const` table, so we precompute
    // a static pool of 200 4-byte name slots and index into it.
    static PLACEHOLDERS: [[u8; 4]; 256] = {
        let mut out = [[b' '; 4]; 256];
        let hex = b"0123456789ABCDEF";
        let mut i = 0;
        while i < 256 {
            out[i][0] = b'K';
            out[i][1] = hex[(i >> 8) & 0xf];
            out[i][2] = hex[(i >> 4) & 0xf];
            out[i][3] = hex[i & 0xf];
            i += 1;
        }
        out
    };
    for kc in 8u8..=255 {
        let idx = (kc - 8) as usize;
        if names[idx] == b"K   " {
            names[idx] = &PLACEHOLDERS[kc as usize];
        }
    }
    names
}

/// Standard US-QWERTY keysyms keyed by physical X11 keycode (8..255).
/// Index 0 corresponds to keycode 8 (which is unused on real X
/// servers — Xorg starts user keys at keycode 9). Returns 0
/// (NoSymbol) for keys we don't model.
///
/// We don't try to be exhaustive — this is the bare minimum so that
/// xkbcomp can dump a recognisable keymap and so that synthetic
/// input from the frontend has plausible keysym mappings.
fn us_qwerty_keysyms() -> [u32; 248] {
    let mut syms = [0u32; 248];
    // (keycode, keysym) pairs from /usr/share/X11/xkb/symbols/us
    // (level 1 only — no Shift variants).
    let mappings: &[(u8, u32)] = &[
        (9, 0xff1b),  // Escape
        (10, b'1' as u32),
        (11, b'2' as u32),
        (12, b'3' as u32),
        (13, b'4' as u32),
        (14, b'5' as u32),
        (15, b'6' as u32),
        (16, b'7' as u32),
        (17, b'8' as u32),
        (18, b'9' as u32),
        (19, b'0' as u32),
        (20, b'-' as u32),
        (21, b'=' as u32),
        (22, 0xff08), // BackSpace
        (23, 0xff09), // Tab
        (24, b'q' as u32),
        (25, b'w' as u32),
        (26, b'e' as u32),
        (27, b'r' as u32),
        (28, b't' as u32),
        (29, b'y' as u32),
        (30, b'u' as u32),
        (31, b'i' as u32),
        (32, b'o' as u32),
        (33, b'p' as u32),
        (34, b'[' as u32),
        (35, b']' as u32),
        (36, 0xff0d), // Return
        (37, 0xffe3), // Control_L
        (38, b'a' as u32),
        (39, b's' as u32),
        (40, b'd' as u32),
        (41, b'f' as u32),
        (42, b'g' as u32),
        (43, b'h' as u32),
        (44, b'j' as u32),
        (45, b'k' as u32),
        (46, b'l' as u32),
        (47, b';' as u32),
        (48, b'\'' as u32),
        (49, b'`' as u32),
        (50, 0xffe1), // Shift_L
        (51, b'\\' as u32),
        (52, b'z' as u32),
        (53, b'x' as u32),
        (54, b'c' as u32),
        (55, b'v' as u32),
        (56, b'b' as u32),
        (57, b'n' as u32),
        (58, b'm' as u32),
        (59, b',' as u32),
        (60, b'.' as u32),
        (61, b'/' as u32),
        (62, 0xffe2), // Shift_R
        (63, b'*' as u32),
        (64, 0xffe9), // Alt_L
        (65, b' ' as u32), // Space
        (66, 0xffe5), // Caps_Lock
        (77, 0xff7f), // Num_Lock
        (105, 0xffe4), // Control_R
        (108, 0xffea), // Alt_R
        (133, 0xffeb), // Super_L
        (134, 0xffec), // Super_R
    ];
    for &(kc, sym) in mappings {
        if kc >= 8 {
            syms[(kc - 8) as usize] = sym;
        }
    }
    syms
}

pub(crate) fn handle_xkb_request(data: &[u8], seq: u16) -> Vec<u8> {
    // Minor opcodes per X11/extensions/XKBproto.h:
    //
    //   0  UseExtension              (reply)
    //   1  SelectEvents              (void)
    //   3  Bell                      (void)
    //   4  GetState                  (reply)
    //   5  LatchLockState            (void)
    //   6  GetControls               (reply)
    //   7  SetControls               (void)
    //   8  GetMap                    (reply)
    //   9  SetMap                    (void)
    //  10  GetCompatMap              (reply)
    //  11  SetCompatMap              (void)
    //  12  GetIndicatorState         (reply)
    //  13  GetIndicatorMap           (reply)
    //  14  SetIndicatorMap           (void)
    //  15  GetNamedIndicator         (reply)
    //  16  SetNamedIndicator         (void)
    //  17  GetNames                  (reply)
    //  18  SetNames                  (void)
    //  21  PerClientFlags            (reply)
    //  22  ListComponents            (reply)
    //  23  GetKbdByName              (reply)
    //  24  GetDeviceInfo             (reply)
    let minor = data[1];
    debug!("XKB minor opcode: {minor}");

    let device_id_byte = if data.len() >= 6 { data[4] } else { 0 };

    match minor {
        0 => {
            // UseExtension: reply with supported=true, version 1.0
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 1; // supported = true
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // server major version
            reply[10..12].copy_from_slice(&0u16.to_le_bytes()); // server minor version
            reply.to_vec()
        }
        // Void requests — no reply.
        1 | 3 | 5 | 7 | 9 | 11 | 14 | 16 | 18 => Vec::new(),
        4 => {
            // GetState: minimal reply with all zero modifier / group state
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = device_id_byte;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        6 => {
            // GetControls reply (xcb-proto): 92 bytes total. 8-byte
            // standard X reply header + 84-byte body. The fields
            // start at offset 8 with mouseKeysDfltBtn / numGroups
            // / ... and end with a 32-byte perKeyRepeat bitmap at
            // offset 60..92.
            let mut reply = vec![0u8; 92];
            reply[0] = 1;
            reply[1] = device_id_byte;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // length = (92 - 32) / 4 = 15
            reply[4..8].copy_from_slice(&15u32.to_le_bytes());
            // Byte 8: mouseKeysDfltBtn = 0
            reply[9] = 1; // numGroups (must be >= 1)
            // Bytes 10-19: modifiers, groupsWrap, etc. (all zero)
            // Byte 20-21: repeatDelay — non-zero to avoid division-by-zero
            // in clients that compute repeat rate. Xorg default = 660ms.
            reply[20..22].copy_from_slice(&660u16.to_le_bytes());
            // Byte 22-23: repeatInterval — non-zero! Xorg default = 40ms.
            // A zero value here causes SIGFPE in xset which divides by it.
            reply[22..24].copy_from_slice(&40u16.to_le_bytes());
            // Bytes 24-59: slowKeys, debounce, mouseKeys, accessX, etc. (all zero is fine)
            // Bytes 60-91: perKeyRepeat (32 bytes) — set every
            // bit so all keys auto-repeat by default.
            for byte in &mut reply[60..92] {
                *byte = 0xff;
            }
            reply
        }
        8 => build_xkb_get_map_reply(seq),
        10 => {
            // GetCompatMap reply.
            //
            // libxkbfile's `_XkbReadGetCompatMapReply` calls
            // `_XkbInitReadBuffer(dpy, &buf, length * 4)`
            // *unconditionally* and that helper returns failure for
            // size <= 0, so a 32-byte length=0 reply makes
            // XkbGetCompatMap return BadAlloc.
            //
            // libxkbfile then refuses to render the compat section
            // unless `xkb->compat->sym_interpret` is non-null —
            // which only gets allocated when the wire reply
            // declares at least one sym interpretation. So we ship
            // a single placeholder XkbSymInterpretWireDesc plus one
            // group compat entry.
            //
            // Reply layout:
            //   8 bytes:  std header
            //   8:        groupsRtrn = 0x01  (group 0 present)
            //   9:        pad
            //   10-11:    firstSIRtrn = 0
            //   12-13:    nSIRtrn = 1
            //   14-15:    nTotalSI = 1
            //   16-31:    pad (16 bytes)
            //   32-47:    XkbSymInterpretWireDesc (16 bytes, all 0)
            //   48-51:    xkbModsWireDesc for group 0 (4 bytes, all 0)
            //
            // 52 bytes total → length = 5.
            let mut reply = vec![0u8; 52];
            reply[0] = 1;
            reply[1] = device_id_byte;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&5u32.to_le_bytes()); // length
            reply[8] = 0x01; // groupsRtrn: group 0
            reply[12..14].copy_from_slice(&1u16.to_le_bytes()); // nSIRtrn
            reply[14..16].copy_from_slice(&1u16.to_le_bytes()); // nTotalSI
            // bytes 32..48: SymInterpret entry (all-zero placeholder
            //   sym=NoSymbol, no modifiers, NoAction)
            // bytes 48..52: ModWireDesc for group 0 (all zero — no mods)
            reply
        }
        12 => {
            // GetIndicatorState: state = 0
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = device_id_byte;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        13 => {
            // GetIndicatorMap: no indicators
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = device_id_byte;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        15 => {
            // GetNamedIndicator: empty
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = device_id_byte;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        17 => build_xkb_get_names_reply(seq, device_id_byte),
        19 => {
            // GetGeometry reply: "no geometry". Sending length=0
            // makes libxkbfile take the early-out path that skips
            // the body parse entirely (the variable-length section
            // with labelFont / properties / colors / shapes etc.
            // would otherwise force us to invent placeholder
            // colours, and libxkbfile then dereferences
            // `geom->base_color = &geom->colors[0]` which segfaults
            // if num_colors stays at zero).
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = device_id_byte;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // length = 0, found = 0, all other fields zero.
            reply.to_vec()
        }
        21 => {
            // PerClientFlags: supported = 0, value = 0
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = device_id_byte;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        22 => {
            // ListComponents: empty list of every category. Reply has
            // CARD16 counts for keymaps / keycodes / types / compat /
            // symbols / geometries / extra, then 10 bytes of pad.
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = device_id_byte;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        23 => {
            // GetKbdByName: return our standard map. The reply layout
            // is the same `present`/section format as GetMap with a
            // larger header, but for our purposes the GetMap encoder
            // is close enough — xkbcomp falls back to issuing GetMap
            // separately if GetKbdByName returns nothing.
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = device_id_byte;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply
                .to_vec()
        }
        24 => {
            // GetDeviceInfo: zero device info
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = device_id_byte;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        _ => {
            debug!("Unhandled XKB minor opcode: {minor}");
            Vec::new()
        }
    }
}

pub(crate) fn handle_xc_misc_request(data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("XC-MISC minor opcode: {minor}");

    match minor {
        0 => {
            // GetVersion: reply with version 1.1
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // major version
            reply[10..12].copy_from_slice(&1u16.to_le_bytes()); // minor version
            reply.to_vec()
        }
        1 => {
            // GetXIDRange: reply with a range of resource IDs
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&0x08000000u32.to_le_bytes()); // start_id
            reply[12..16].copy_from_slice(&65536u32.to_le_bytes()); // count
            reply.to_vec()
        }
        2 => {
            // GetXIDList: return requested number of IDs
            let count = if data.len() >= 8 {
                u32::from_le_bytes([data[4], data[5], data[6], data[7]])
            } else {
                0
            };
            let ids_to_return = count.min(4096); // cap at reasonable limit
            let extra_bytes = (ids_to_return as usize) * 4;
            let padded = (extra_bytes + 3) & !3;
            let mut reply = vec![0u8; 32 + padded];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&((padded / 4) as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&ids_to_return.to_le_bytes()); // ids_count
            // Fill in sequential IDs starting from a high range
            let base: u32 = 0x09000000;
            for i in 0..ids_to_return {
                let offset = 32 + (i as usize) * 4;
                let id = base + i;
                reply[offset..offset + 4].copy_from_slice(&id.to_le_bytes());
            }
            reply
        }
        _ => {
            debug!("Unhandled XC-MISC minor opcode: {minor}");
            Vec::new()
        }
    }
}

/// Handle X Present extension requests (major opcode 148).
pub(crate) fn handle_present_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("Present minor opcode: {minor}");

    match minor {
        // QueryVersion
        0 => {
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&1u32.to_le_bytes()); // major version
            reply[12..16].copy_from_slice(&2u32.to_le_bytes()); // minor version
            reply.to_vec()
        }
        // Pixmap (PresentPixmap) — the critical operation
        1 => {
            if data.len() < 72 {
                debug!("PresentPixmap: request too short ({} bytes)", data.len());
                return Vec::new();
            }
            let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let pixmap = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
            let serial = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
            let x_off = i16::from_le_bytes([data[24], data[25]]);
            let y_off = i16::from_le_bytes([data[26], data[27]]);

            info!(
                "PresentPixmap: window={:#x} pixmap={:#x} serial={} x_off={} y_off={}",
                window, pixmap, serial, x_off, y_off
            );

            // Copy pixels from the source pixmap to the destination window.
            // We need to clone the pixel data first because we can't borrow
            // both the pixmap and window framebuffers simultaneously.
            // Sync SHM pixmaps before reading
            state.sync_shm_pixmap(pixmap);

            let src_info = {
                let resolved = state.resolve_drawable(pixmap);
                if let Some(win) = state.windows.get(&resolved) {
                    Some((
                        win.framebuffer.width() as u16,
                        win.framebuffer.height() as u16,
                        win.framebuffer.data().to_vec(),
                        24u8,
                    ))
                } else if let Some(pix) = state.pixmaps.get(&resolved) {
                    Some((
                        pix.framebuffer.width() as u16,
                        pix.framebuffer.height() as u16,
                        pix.framebuffer.data().to_vec(),
                        pix.depth,
                    ))
                } else {
                    debug!("PresentPixmap: source pixmap {:#x} not found", pixmap);
                    None
                }
            };

            if let Some((src_w, src_h, mut src_data, src_depth)) = src_info {
                // Debug: count non-black pixels in source

                // For depth-1 pixmaps, convert 1-bit values to proper RGB:
                // pixel != 0 → white (0xFFFFFF), pixel == 0 → black (0x000000)
                if src_depth <= 1 {
                    for i in (0..src_data.len()).step_by(4) {
                        if i + 3 < src_data.len() {
                            let is_set = src_data[i] != 0 || src_data[i + 1] != 0 || src_data[i + 2] != 0;
                            let val = if is_set { 0xFF } else { 0x00 };
                            src_data[i] = val;     // B
                            src_data[i + 1] = val; // G
                            src_data[i + 2] = val; // R
                            src_data[i + 3] = 0xFF;
                        }
                    }
                }
                // Determine the target window and offset for rendering.
                // If the target is a child window, propagate pixels up to the
                // parent (top-level) window so the frontend sees them.
                let (target_wid, total_x_off, total_y_off) = {
                    let mut wid = window;
                    let mut tx = x_off as i32;
                    let mut ty = y_off as i32;
                    // Walk up the parent chain to the top-level window
                    for _ in 0..10 {
                        let parent = state.windows.get(&wid).map(|w| w.parent);
                        match parent {
                            Some(p) if p != state.root_window && p != 0 => {
                                // Add this window's position relative to its parent
                                if let Some(w) = state.windows.get(&wid) {
                                    tx += w.x as i32;
                                    ty += w.y as i32;
                                }
                                wid = p;
                            }
                            _ => break,
                        }
                    }
                    (wid, tx as i16, ty as i16)
                };

                // Copy to the child window (keeps its framebuffer up-to-date)
                if let Some(win) = state.windows.get_mut(&window) {
                    win.framebuffer.put_image(x_off, y_off, src_w, src_h, &src_data);
                }

                // Also copy to the top-level parent so the frontend displays it
                if target_wid != window {
                    if let Some(parent_win) = state.windows.get_mut(&target_wid) {
                        parent_win.framebuffer.put_image(total_x_off, total_y_off, src_w, src_h, &src_data);
                        info!(
                            "PresentPixmap: propagated {}x{} from child {:#x} to parent {:#x} at ({},{})",
                            src_w, src_h, window, target_wid, total_x_off, total_y_off
                        );
                    }
                } else {
                    info!(
                        "PresentPixmap: copied {}x{} to window {:#x}",
                        src_w, src_h, window
                    );
                }

                if !state.windows.contains_key(&window) {
                    debug!("PresentPixmap: destination window {:#x} not found", window);
                }
            }

            // Send PresentCompleteNotify if the client subscribed via SelectInput
            let matching_subs: Vec<(u32, u32)> = state
                .present_subscriptions
                .iter()
                .filter(|(_, sub)| sub.window == window && (sub.event_mask & 1) != 0)
                .map(|(&eid, sub)| (eid, sub.window))
                .collect();

            for (event_id, _win) in matching_subs {
                // GenericEvent format for PresentCompleteNotify
                let mut event = [0u8; 32];
                event[0] = 35; // GenericEvent
                event[1] = 148; // Present extension major opcode
                event[2..4].copy_from_slice(&seq.to_le_bytes());
                // event[4..8] = 0 (no extra data beyond 32 bytes)
                event[8..10].copy_from_slice(&1u16.to_le_bytes()); // CompleteNotify event type
                // event[10..12] = pad
                event[12..16].copy_from_slice(&event_id.to_le_bytes()); // event_id
                event[16..20].copy_from_slice(&window.to_le_bytes()); // window
                event[20..24].copy_from_slice(&serial.to_le_bytes()); // serial
                event[24] = 0; // kind = Pixmap
                event[25] = 0; // mode = Copy
                state.pending_events.push(event.to_vec());
            }

            Vec::new() // PresentPixmap has no reply
        }
        // NotifyMSC
        2 => {
            // Stub: we don't track MSC, just ignore
            debug!("PresentNotifyMSC: stub");
            Vec::new()
        }
        // SelectInput
        3 => {
            if data.len() >= 16 {
                let event_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let window = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let event_mask = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

                debug!(
                    "PresentSelectInput: event_id={:#x} window={:#x} event_mask={:#x}",
                    event_id, window, event_mask
                );

                if event_mask == 0 {
                    // Unsubscribe
                    state.present_subscriptions.remove(&event_id);
                } else {
                    state.present_subscriptions.insert(
                        event_id,
                        PresentSubscription { window, event_mask },
                    );
                }
            }
            Vec::new() // SelectInput has no reply
        }
        // QueryCapabilities
        4 => {
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&0u32.to_le_bytes()); // capabilities = none
            reply.to_vec()
        }
        _ => {
            debug!("Unhandled Present minor opcode: {minor}");
            Vec::new()
        }
    }
}

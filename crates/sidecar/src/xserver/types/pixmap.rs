//! Pixmap state, shared memory segments, and shared resource registries.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::gc::GcState;
use crate::framebuffer::Framebuffer;

pub(crate) struct PixmapState {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) depth: u8,
    pub(crate) framebuffer: Framebuffer,
    pub(crate) alias_window: Option<u32>,
    pub(crate) shm_backing: Option<ShmPixmapBacking>,
}

/// Shared pixmap metadata for cross-connection access.
/// Stores the pixmap's geometry and owner so other connections can
/// validate references. The actual framebuffer data is proxied through
/// the owning connection via SharedPixmapFbs.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct SharedPixmapMeta {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) depth: u8,
    pub(crate) owner_client_id: String,
}

/// Shared pixmap registry for cross-connection drawable access.
/// Per X11 spec, resource IDs are global within a display—any client
/// may reference a pixmap created by another client.
pub(crate) type SharedPixmaps = Arc<Mutex<HashMap<u32, SharedPixmapMeta>>>;

/// Shared pixmap framebuffers for cross-connection drawing.
/// Maps pixmap ID → Framebuffer, protected by Arc<Mutex<..>>.
/// This is separate from SharedPixmapMeta to allow mutable borrow
/// of the framebuffer without blocking metadata reads.
pub(crate) type SharedPixmapFbs = Arc<Mutex<HashMap<u32, Framebuffer>>>;

/// Shared GC registry for cross-connection GC access.
/// Per X11 spec, GC resource IDs are global within a display.
pub(crate) type SharedGcs = Arc<Mutex<HashMap<u32, GcState>>>;

#[derive(Clone)]
pub(crate) struct ShmPixmapBacking {
    pub(crate) shmseg: u32,
    pub(crate) offset: usize,
}

/// A shared memory segment attached via MIT-SHM.
pub(crate) struct ShmSegment {
    pub(crate) addr: *mut u8,
    pub(crate) size: usize,
}

unsafe impl Send for ShmSegment {}

/// Damage subscription info for DAMAGE extension.
///
/// Per the DAMAGE spec, damage accumulates as a region of rectangles
/// until the client acknowledges it via DamageSubtract.  The `level`
/// field controls granularity:
///   0 = RawRectangles — report each individual damaged rect
///   1 = DeltaRectangles — coalesce into bounding box since last
///   2 = BoundingBox — single bounding rect of all damage
///   3 = NonEmpty — just notify that damage exists
#[derive(Clone)]
pub(crate) struct DamageInfo {
    pub(crate) drawable: u32,
    pub(crate) level: u8,
    /// Accumulated damage region since last DamageSubtract.
    pub(crate) accumulated: super::region::XFixesRegion,
}

/// Present extension event subscription.
#[derive(Clone)]
pub(crate) struct PresentSubscription {
    pub(crate) window: u32,
    pub(crate) event_mask: u32,
}

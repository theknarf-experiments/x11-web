//! DRI3 pixmap operations: PixmapFromBuffer, BufferFromPixmap,
//! PixmapFromBuffers, BuffersFromPixmap.

use tracing::{debug, warn};

use super::super::super::client::ClientState;
use super::super::super::core::*;
use super::super::super::types::PixmapState;
use super::super::parse_minor;
use crate::framebuffer::Framebuffer;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

// DRM fourcc codes for YUV formats

// -----------------------------------------------------------------
// 2: PixmapFromBuffer
// -----------------------------------------------------------------

// -----------------------------------------------------------------
// 3: BufferFromPixmap
// -----------------------------------------------------------------

// -----------------------------------------------------------------
// 7: PixmapFromBuffers (DRI3 1.2, multi-plane)
// -----------------------------------------------------------------

// -----------------------------------------------------------------
// 8: BuffersFromPixmap (DRI3 1.2, multi-plane)
// -----------------------------------------------------------------

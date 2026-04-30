//! DRI3 extension handler.
//!
//! DRI3 enables zero-copy buffer sharing between the X server and GPU clients
//! via DMA-BUF file descriptors. Our implementation provides version negotiation
//! and basic fd-backed pixmap import so Mesa's software fallback path works.

// DRI3 minor opcodes:
// 0 = QueryVersion
// 1 = Open
// 2 = PixmapFromBuffer
// 3 = BufferFromPixmap
// 4 = FenceFromFD
// 5 = FDFromFence
// (DRI3 1.2+)
// 6 = GetSupportedModifiers
// 7 = PixmapFromBuffers
// 8 = BuffersFromPixmap
// (DRI3 1.4+)
// 9 = SetDRMDeviceInUse

mod device;
mod fence;
mod pixmap;

// Supported DRI3 version

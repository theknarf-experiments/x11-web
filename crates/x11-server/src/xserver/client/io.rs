//! Byte-order I/O helpers and error building for ClientState.

use super::ClientState;
use crate::xserver::core::RESOURCE_ID_MASK;

impl ClientState {
    /// Validate that a resource ID belongs to this client's allocated range.
    /// Returns true if valid, false if the ID is outside this client's range.
    pub(crate) fn validate_resource_id(&self, id: u32) -> bool {
        (id & !RESOURCE_ID_MASK) == self.resource_id_base
    }

    /// Recycle a freed resource ID so it can be reused via XC-MISC GetXIDList.
    /// Only recycles IDs that belong to this client's range.
    pub(crate) fn recycle_xid(&mut self, id: u32) {
        if self.validate_resource_id(id) {
            self.freed_xids.push(id);
        }
    }

    /// Get the current server timestamp (milliseconds since server start).
    pub(crate) fn timestamp(&self) -> u32 {
        self.server_start.elapsed().as_millis() as u32
    }

    // -----------------------------------------------------------------------
    // Byte-order-aware read helpers for parsing client requests.
    // -----------------------------------------------------------------------

    /// Read a u16 from request data respecting client byte order.
    #[inline]
    pub(crate) fn read_u16(&self, data: &[u8], offset: usize) -> u16 {
        let bytes: [u8; 2] = data[offset..offset + 2].try_into().unwrap();
        if self.msb_first {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        }
    }

    /// Read a u32 from request data respecting client byte order.
    #[inline]
    pub(crate) fn read_u32(&self, data: &[u8], offset: usize) -> u32 {
        let bytes: [u8; 4] = data[offset..offset + 4].try_into().unwrap();
        if self.msb_first {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        }
    }

    /// Read a u32 from arbitrary data respecting client byte order.
    /// Same as `read_u32` but with a distinct name for clarity on non-request data.
    #[inline]
    pub(crate) fn read_u32_from(&self, data: &[u8], offset: usize) -> u32 {
        self.read_u32(data, offset)
    }

    /// Write a u16 into a reply buffer in the client's byte order.
    #[inline]
    pub(crate) fn write_u16(&self, buf: &mut [u8], offset: usize, val: u16) {
        let bytes = if self.msb_first {
            val.to_be_bytes()
        } else {
            val.to_le_bytes()
        };
        buf[offset..offset + 2].copy_from_slice(&bytes);
    }

    /// Write a u32 into a reply buffer in the client's byte order.
    #[inline]
    pub(crate) fn write_u32(&self, buf: &mut [u8], offset: usize, val: u32) {
        let bytes = if self.msb_first {
            val.to_be_bytes()
        } else {
            val.to_le_bytes()
        };
        buf[offset..offset + 4].copy_from_slice(&bytes);
    }

    /// Write an i16 into a reply buffer in the client's byte order.
    #[inline]
    pub(crate) fn write_i16(&self, buf: &mut [u8], offset: usize, val: i16) {
        let bytes = if self.msb_first {
            val.to_be_bytes()
        } else {
            val.to_le_bytes()
        };
        buf[offset..offset + 2].copy_from_slice(&bytes);
    }
}

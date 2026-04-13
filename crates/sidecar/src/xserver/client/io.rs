//! Byte-order I/O helpers and error building for ClientState.

use super::ClientState;

impl ClientState {
    /// Validate that a resource ID belongs to this client's allocated range.
    /// Returns true if valid, false if the ID is outside this client's range.
    pub(crate) fn validate_resource_id(&self, id: u32) -> bool {
        (id & !0x003FFFFF) == self.resource_id_base
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

    /// Build an error reply in the client's byte order.
    #[allow(dead_code)]
    pub(crate) fn error(&self, error_code: u8, bad_value: u32, major_opcode: u8, minor_opcode: u16) -> Vec<u8> {
        super::super::core::build_error_bo(error_code, self.sequence, bad_value, major_opcode, minor_opcode, self.msb_first)
    }

    // -----------------------------------------------------------------------
    // Byte-order-aware read helpers for parsing client requests.
    // -----------------------------------------------------------------------

    /// Read a u16 from request data respecting client byte order.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn read_u16(&self, data: &[u8], offset: usize) -> u16 {
        if self.msb_first {
            u16::from_be_bytes([data[offset], data[offset + 1]])
        } else {
            u16::from_le_bytes([data[offset], data[offset + 1]])
        }
    }

    /// Read a u32 from request data respecting client byte order.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn read_u32(&self, data: &[u8], offset: usize) -> u32 {
        if self.msb_first {
            u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
        } else {
            u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
        }
    }

    /// Read a u32 from arbitrary data respecting client byte order.
    /// Same as `read_u32` but with a distinct name for clarity on non-request data.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn read_u32_from(&self, data: &[u8], offset: usize) -> u32 {
        self.read_u32(data, offset)
    }

    /// Read an i16 from request data respecting client byte order.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn read_i16(&self, data: &[u8], offset: usize) -> i16 {
        if self.msb_first {
            i16::from_be_bytes([data[offset], data[offset + 1]])
        } else {
            i16::from_le_bytes([data[offset], data[offset + 1]])
        }
    }

    /// Write a u16 into a reply buffer in the client's byte order.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn write_u16(&self, buf: &mut [u8], offset: usize, val: u16) {
        let bytes = if self.msb_first { val.to_be_bytes() } else { val.to_le_bytes() };
        buf[offset..offset + 2].copy_from_slice(&bytes);
    }

    /// Write a u32 into a reply buffer in the client's byte order.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn write_u32(&self, buf: &mut [u8], offset: usize, val: u32) {
        let bytes = if self.msb_first { val.to_be_bytes() } else { val.to_le_bytes() };
        buf[offset..offset + 4].copy_from_slice(&bytes);
    }

    /// Write an i16 into a reply buffer in the client's byte order.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn write_i16(&self, buf: &mut [u8], offset: usize, val: i16) {
        let bytes = if self.msb_first { val.to_be_bytes() } else { val.to_le_bytes() };
        buf[offset..offset + 2].copy_from_slice(&bytes);
    }
}

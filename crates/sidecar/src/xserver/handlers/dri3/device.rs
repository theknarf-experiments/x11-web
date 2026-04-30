//! DRI3 device/modifier operations: GetSupportedModifiers, SetDRMDeviceInUse.

use tracing::debug;

use super::super::super::client::ClientState;
use super::super::parse_minor;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

// -----------------------------------------------------------------
// 6: GetSupportedModifiers (DRI3 1.2)
// -----------------------------------------------------------------

// -----------------------------------------------------------------
// 9: SetDRMDeviceInUse (DRI3 1.4, void request)
// -----------------------------------------------------------------

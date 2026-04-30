//! DRI3 fence operations: FenceFromFD, FDFromFence.

use tracing::{debug, warn};

use super::super::super::client::ClientState;
use super::super::super::core::*;
use super::super::parse_minor;
use crate::xserver::reply::ReplyBuf;

// -----------------------------------------------------------------
// 4: FenceFromFD — create a SYNC fence backed by an fd
// -----------------------------------------------------------------

// -----------------------------------------------------------------
// 5: FDFromFence — export a SYNC fence as a file descriptor
// -----------------------------------------------------------------

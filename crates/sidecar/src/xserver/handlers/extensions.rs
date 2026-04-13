//! Extension protocol handlers (opcodes >= 128).
//!
//! Each extension lives in its own submodule. This file re-exports all
//! handler functions so that call sites can continue using
//! `handlers::extensions::handle_*`.

// Extension submodules (declared in the parent mod.rs)
pub(crate) use super::composite::{handle_damage_request, handle_x_composite_request};
pub(crate) use super::dbe::handle_dbe_request;
pub(crate) use super::dpms::handle_dpms_request;
pub(crate) use super::dri3::handle_dri3_request;
pub(crate) use super::glx::handle_glx_request;
pub(crate) use super::present::{
    handle_present_request, handle_xc_misc_request, send_present_config_notify,
};
pub(crate) use super::randr::handle_randr_request;
pub(crate) use super::screensaver::handle_screen_saver_request;
pub(crate) use super::security::handle_security_request;
pub(crate) use super::shape::handle_shape_request;
pub(crate) use super::shm::handle_shm_request;
pub(crate) use super::sync::handle_sync_request;
pub(crate) use super::vidmode::handle_vidmode_request;
pub(crate) use super::xfixes::handle_xfixes_request;
pub(crate) use super::xinerama::handle_xinerama_request;
pub(crate) use super::xkb::{handle_ge_request, handle_xkb_request};
pub(crate) use super::xresource::handle_xresource_request;
pub(crate) use super::xtest::handle_xtest_request;
pub(crate) use super::xvideo::handle_xvideo_request;

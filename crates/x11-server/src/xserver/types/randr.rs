//! RandR data model: CRTCs, outputs, modes, providers, monitors, and EDID generation.

use super::window::PropertyValue;
use std::collections::HashMap;

/// RandR CRTC (a display controller that drives an output).
#[derive(Clone, Debug)]
pub(crate) struct RandrCrtc {
    pub(crate) id: u32,
    pub(crate) x: i16,
    pub(crate) y: i16,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) mode_id: u32,
    pub(crate) rotation: u16,
    pub(crate) outputs: Vec<u32>,
    /// 256-entry gamma ramp per channel (R, G, B).
    pub(crate) gamma_red: Vec<u16>,
    pub(crate) gamma_green: Vec<u16>,
    pub(crate) gamma_blue: Vec<u16>,
    /// CRTC transform matrix (3x3, each element is a 16.16 fixed-point value).
    /// Stored as raw i32 values in row-major order. Identity = [65536,0,0, 0,65536,0, 0,0,65536].
    pub(crate) transform: [i32; 9],
}

/// Gamma ramp full-scale value: gamma stops are u16, so the maximum entry
/// is `u16::MAX = 65535`.
pub(crate) const GAMMA_RAMP_MAX: u32 = u16::MAX as u32;
/// Identity scale for RandR's 16.16 fixed-point transform matrix.
pub(crate) const FP_16_16_ONE: i32 = 1 << 16;

impl RandrCrtc {
    /// Create a default CRTC with a linear gamma ramp.
    pub(crate) fn new(id: u32, width: u16, height: u16, mode_id: u32, output_id: u32) -> Self {
        let gamma: Vec<u16> = (0..256)
            .map(|i| ((i as u32 * GAMMA_RAMP_MAX) / 255) as u16)
            .collect();
        Self {
            id,
            x: 0,
            y: 0,
            width,
            height,
            mode_id,
            rotation: 1, // Rotate_0
            outputs: vec![output_id],
            gamma_red: gamma.clone(),
            gamma_green: gamma.clone(),
            gamma_blue: gamma,
            // Identity transform in 16.16 fixed-point.
            transform: [FP_16_16_ONE, 0, 0, 0, FP_16_16_ONE, 0, 0, 0, FP_16_16_ONE],
        }
    }
}

/// RandR output (a physical or virtual display connector).
#[derive(Clone, Debug)]
pub(crate) struct RandrOutput {
    pub(crate) id: u32,
    pub(crate) name: String,
    /// 0 = Connected, 1 = Disconnected, 2 = Unknown.
    pub(crate) connection_status: u8,
    pub(crate) crtc_id: u32,
    pub(crate) modes: Vec<u32>,
    pub(crate) mm_width: u32,
    pub(crate) mm_height: u32,
    /// Output properties (property atom -> PropertyValue).
    pub(crate) properties: HashMap<u32, PropertyValue>,
    /// Output property configurations (property atom -> config).
    pub(crate) property_configs: HashMap<u32, OutputPropertyConfig>,
    /// CRTCs that can drive this output.
    pub(crate) possible_crtcs: Vec<u32>,
}

/// RandR mode (a video timing / resolution).
#[derive(Clone, Debug)]
pub(crate) struct RandrMode {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) dot_clock: u32,
    pub(crate) h_sync_start: u16,
    pub(crate) h_sync_end: u16,
    pub(crate) h_total: u16,
    pub(crate) v_sync_start: u16,
    pub(crate) v_sync_end: u16,
    pub(crate) v_total: u16,
    pub(crate) flags: u32,
}

impl RandrMode {
    /// Create a mode with sensible sync timings for the given resolution.
    pub(crate) fn new(id: u32, width: u16, height: u16) -> Self {
        let name = format!("{}x{}", width, height);
        Self {
            id,
            name,
            width,
            height,
            dot_clock: 60000,
            h_sync_start: width + 40,
            h_sync_end: width + 80,
            h_total: width + 160,
            v_sync_start: height + 3,
            v_sync_end: height + 6,
            v_total: height + 30,
            flags: 0,
        }
    }
}

/// RandR provider (GPU or software renderer).
#[derive(Clone, Debug)]
pub(crate) struct RandrProvider {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) capabilities: u32,
    pub(crate) crtcs: Vec<u32>,
    pub(crate) outputs: Vec<u32>,
}

/// RandR monitor definition (RandR 1.5).
#[derive(Clone, Debug)]
pub(crate) struct RandrMonitor {
    pub(crate) name_atom: u32,
    pub(crate) primary: bool,
    pub(crate) automatic: bool,
    pub(crate) x: i16,
    pub(crate) y: i16,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) output_ids: Vec<u32>,
}

/// RandR output property configuration (stored by ConfigureOutputProperty).
#[derive(Clone, Debug)]
pub(crate) struct OutputPropertyConfig {
    pub(crate) pending: bool,
    pub(crate) range: bool,
    pub(crate) values: Vec<u32>,
}

/// Event base for RandR (assigned in QueryExtension).
pub(crate) const RANDR_EVENT_BASE: u8 = 89;

/// Default RandR resource IDs we hand out for the single built-in
/// monitor / CRTC / mode / provider. Spread across distinct ranges
/// (100s/200s/…) so they don't collide with a client's resource_id_base.
pub(crate) const DEFAULT_RANDR_CRTC_ID: u32 = 100;
pub(crate) const DEFAULT_RANDR_OUTPUT_ID: u32 = 200;
pub(crate) const DEFAULT_RANDR_MODE_ID: u32 = 300;
pub(crate) const DEFAULT_RANDR_PROVIDER_ID: u32 = 400;

/// Convert a pixel measurement to millimetres assuming the standard 96 DPI
/// (≈ 25.4 mm/inch * 10 → 254/960 with `+ 480` rounding for half-up).
#[inline]
pub(crate) fn pixels_to_mm_at_96dpi(pixels: u32) -> u32 {
    (pixels * 254 + 480) / 960
}

/// RandR event select mask bits.
pub(crate) const RR_SCREEN_CHANGE_NOTIFY_MASK: u32 = 1 << 0;
pub(crate) const RR_CRTC_CHANGE_NOTIFY_MASK: u32 = 1 << 1;

// EDID 1.3 wire-format constants. Byte values defined by the VESA EDID
// specification — names follow the spec's terminology so a reader can
// cross-reference. We only define the bytes that carry semantic meaning;
// dimensional fields (sync widths, blanking intervals) stay inline.

/// Mandatory EDID header (8 bytes) that identifies the blob.
const EDID_HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

/// Video-input descriptor: bit 7 set = digital input, lower bits = depth code.
const EDID_VIDEO_INPUT_DIGITAL: u8 = 0x80;

/// Feature-support byte: standard sRGB color space, preferred timing in DTD1.
const EDID_FEATURE_SUPPORT: u8 = 0x0A;

/// 10-byte chromaticity block — standard sRGB-ish primaries.
const EDID_CHROMATICITY: [u8; 10] = [0xEE, 0x91, 0xA3, 0x54, 0x4C, 0x99, 0x26, 0x0F, 0x50, 0x54];

/// Detailed Timing Descriptor flag byte: non-interlaced, normal display.
const EDID_DTD_FLAGS: u8 = 0x18;

/// Monitor-name descriptor tag (used in DTD#2 to declare a monitor name string).
const EDID_DESC_TAG_MONITOR_NAME: u8 = 0xFC;

/// Field byte offsets within a 128-byte EDID 1.3 blob. Names follow the
/// VESA EDID 1.3 spec so a reader can cross-reference.
mod edid_offset {
    pub(super) const HEADER: std::ops::Range<usize> = 0..8;
    pub(super) const MANUFACTURER_ID: std::ops::Range<usize> = 8..10;
    pub(super) const PRODUCT_CODE: std::ops::Range<usize> = 10..12;
    pub(super) const SERIAL: std::ops::Range<usize> = 12..16;
    pub(super) const WEEK_OF_MANUFACTURE: usize = 16;
    pub(super) const YEAR_OF_MANUFACTURE: usize = 17; // year - 1990
    pub(super) const VERSION: usize = 18;
    pub(super) const REVISION: usize = 19;
    pub(super) const VIDEO_INPUT: usize = 20;
    pub(super) const MAX_H_IMAGE_SIZE_CM: usize = 21;
    pub(super) const MAX_V_IMAGE_SIZE_CM: usize = 22;
    pub(super) const DISPLAY_GAMMA: usize = 23;
    pub(super) const FEATURE_SUPPORT: usize = 24;
    pub(super) const CHROMATICITY: std::ops::Range<usize> = 25..35;
    pub(super) const ESTABLISHED_TIMINGS: std::ops::Range<usize> = 35..38;
    pub(super) const STANDARD_TIMINGS: std::ops::Range<usize> = 38..54;
    pub(super) const DTD1: std::ops::Range<usize> = 54..72;
    pub(super) const DTD2: std::ops::Range<usize> = 72..90;
    // DTD3 + DTD4 (90..126) left zero — unused.
    pub(super) const EXTENSION_FLAG: usize = 126;
    pub(super) const CHECKSUM: usize = 127;
}

/// Field offsets within an EDID Detailed Timing Descriptor (18 bytes).
mod dtd_offset {
    pub(super) const PIXEL_CLOCK_LO: usize = 0;
    pub(super) const PIXEL_CLOCK_HI: usize = 1;
    pub(super) const H_ACTIVE_LO: usize = 2;
    pub(super) const H_BLANKING_LO: usize = 3;
    pub(super) const H_ACTIVE_BLANKING_HI: usize = 4;
    pub(super) const V_ACTIVE_LO: usize = 5;
    pub(super) const V_BLANKING_LO: usize = 6;
    pub(super) const V_ACTIVE_BLANKING_HI: usize = 7;
    pub(super) const H_SYNC_OFFSET: usize = 8;
    pub(super) const H_SYNC_WIDTH: usize = 9;
    pub(super) const V_SYNC_OFFSET_WIDTH: usize = 10;
    pub(super) const SYNC_HI: usize = 11;
    pub(super) const H_IMAGE_SIZE_MM: usize = 12;
    pub(super) const V_IMAGE_SIZE_MM: usize = 13;
    pub(super) const IMAGE_SIZE_MM_HI: usize = 14;
    pub(super) const H_BORDER: usize = 15;
    pub(super) const V_BORDER: usize = 16;
    pub(super) const FLAGS: usize = 17;
}

/// Field offsets within an EDID Monitor-Name descriptor (DTD#2, 18 bytes).
mod monitor_name_offset {
    pub(super) const ZERO_PIXEL_CLOCK: std::ops::Range<usize> = 0..3;
    pub(super) const TAG: usize = 3;
    pub(super) const FLAG: usize = 4;
    pub(super) const NAME: std::ops::Range<usize> = 5..18;
}

/// Manufacturer-ID bytes for "XWB" (X11-Web), encoded as 3 × 5-bit chars
/// per the EDID spec: X=24, W=23, B=2 → 0b11000_10111_00010 = 0xC5C2.
const MANUFACTURER_ID_XWB: [u8; 2] = [0xC5, 0xC2];
/// Product / serial dummy values.
const PRODUCT_CODE: [u8; 2] = [0x01, 0x00];
const SERIAL_NUMBER: [u8; 4] = [0x01, 0x00, 0x00, 0x00];
/// Manufacturing date stamp embedded in the EDID.
const MFG_WEEK: u8 = 1;
const MFG_YEAR_MINUS_1990: u8 = 34; // 2024
/// EDID version we advertise: 1.3.
const EDID_VERSION_MAJOR: u8 = 1;
const EDID_VERSION_MINOR: u8 = 3;
/// Display gamma encoded as `(gamma * 100) - 100` → 2.2 maps to 120.
const DISPLAY_GAMMA_22: u8 = 120;
/// Standard-timing entry meaning "unused" — both bytes 0x01.
const STANDARD_TIMING_UNUSED: u8 = 0x01;
/// Horizontal blanking pixels reserved in our generated mode.
const H_BLANKING_PIXELS: u16 = 160;
/// Vertical blanking lines reserved in our generated mode.
const V_BLANKING_LINES: u16 = 30;
/// Horizontal sync offset / pulse width (in pixels).
const H_SYNC_OFFSET: u8 = 40;
const H_SYNC_WIDTH: u8 = 40;
/// Packed v-sync field: offset=3 (high nibble), width=6 (low nibble).
const V_SYNC_OFFSET_WIDTH: u8 = 0x36;
/// Refresh rate (Hz) used to derive pixel clock.
const REFRESH_HZ: u32 = 60;
/// Pixel-clock divisor: EDID stores pixel clock in 10 kHz units.
const PIXEL_CLOCK_DIVISOR: u32 = 10_000;

/// Generate a minimal valid EDID blob (128 bytes).
pub(crate) fn generate_edid(
    width_mm: u16,
    height_mm: u16,
    width_px: u16,
    height_px: u16,
) -> Vec<u8> {
    use edid_offset as eo;

    let mut edid = vec![0u8; 128];
    edid[eo::HEADER].copy_from_slice(&EDID_HEADER);
    edid[eo::MANUFACTURER_ID].copy_from_slice(&MANUFACTURER_ID_XWB);
    edid[eo::PRODUCT_CODE].copy_from_slice(&PRODUCT_CODE);
    edid[eo::SERIAL].copy_from_slice(&SERIAL_NUMBER);
    edid[eo::WEEK_OF_MANUFACTURE] = MFG_WEEK;
    edid[eo::YEAR_OF_MANUFACTURE] = MFG_YEAR_MINUS_1990;
    edid[eo::VERSION] = EDID_VERSION_MAJOR;
    edid[eo::REVISION] = EDID_VERSION_MINOR;
    edid[eo::VIDEO_INPUT] = EDID_VIDEO_INPUT_DIGITAL;
    edid[eo::MAX_H_IMAGE_SIZE_CM] = (width_mm / 10) as u8;
    edid[eo::MAX_V_IMAGE_SIZE_CM] = (height_mm / 10) as u8;
    edid[eo::DISPLAY_GAMMA] = DISPLAY_GAMMA_22;
    edid[eo::FEATURE_SUPPORT] = EDID_FEATURE_SUPPORT;
    edid[eo::CHROMATICITY].copy_from_slice(&EDID_CHROMATICITY);
    // Established timings: leave zero (no preset modes asserted).
    for byte in &mut edid[eo::ESTABLISHED_TIMINGS] {
        *byte = 0;
    }
    // Standard timings: 8 entries of 0x0101 (unused).
    for byte in &mut edid[eo::STANDARD_TIMINGS] {
        *byte = STANDARD_TIMING_UNUSED;
    }

    // DTD#1: preferred timing for our actual screen mode.
    {
        use dtd_offset as d;
        let dtd = &mut edid[eo::DTD1];
        let pixel_clock: u16 = ((width_px as u32 + H_BLANKING_PIXELS as u32)
            * (height_px as u32 + V_BLANKING_LINES as u32)
            * REFRESH_HZ
            / PIXEL_CLOCK_DIVISOR) as u16;
        dtd[d::PIXEL_CLOCK_LO] = (pixel_clock & 0xFF) as u8;
        dtd[d::PIXEL_CLOCK_HI] = ((pixel_clock >> 8) & 0xFF) as u8;
        dtd[d::H_ACTIVE_LO] = (width_px & 0xFF) as u8;
        dtd[d::H_BLANKING_LO] = H_BLANKING_PIXELS as u8;
        dtd[d::H_ACTIVE_BLANKING_HI] = (((width_px >> 8) & 0x0F) << 4) as u8;
        dtd[d::V_ACTIVE_LO] = (height_px & 0xFF) as u8;
        dtd[d::V_BLANKING_LO] = V_BLANKING_LINES as u8;
        dtd[d::V_ACTIVE_BLANKING_HI] = (((height_px >> 8) & 0x0F) << 4) as u8;
        dtd[d::H_SYNC_OFFSET] = H_SYNC_OFFSET;
        dtd[d::H_SYNC_WIDTH] = H_SYNC_WIDTH;
        dtd[d::V_SYNC_OFFSET_WIDTH] = V_SYNC_OFFSET_WIDTH;
        dtd[d::SYNC_HI] = 0;
        dtd[d::H_IMAGE_SIZE_MM] = (width_mm & 0xFF) as u8;
        dtd[d::V_IMAGE_SIZE_MM] = (height_mm & 0xFF) as u8;
        dtd[d::IMAGE_SIZE_MM_HI] =
            (((width_mm >> 8) & 0x0F) << 4 | ((height_mm >> 8) & 0x0F)) as u8;
        dtd[d::H_BORDER] = 0;
        dtd[d::V_BORDER] = 0;
        dtd[d::FLAGS] = EDID_DTD_FLAGS;
    }

    // DTD#2: monitor-name descriptor.
    {
        use monitor_name_offset as m;
        let dtd = &mut edid[eo::DTD2];
        for byte in &mut dtd[m::ZERO_PIXEL_CLOCK] {
            *byte = 0;
        }
        dtd[m::TAG] = EDID_DESC_TAG_MONITOR_NAME;
        dtd[m::FLAG] = 0;
        let name = b"X11-Web\n";
        let copy_len = name.len().min(m::NAME.len());
        dtd[m::NAME.start..m::NAME.start + copy_len].copy_from_slice(&name[..copy_len]);
        for byte in &mut dtd[m::NAME.start + copy_len..m::NAME.end] {
            *byte = 0x20; // pad with spaces per EDID spec
        }
    }

    // DTD#3, DTD#4: unused (zeroed).
    edid[eo::EXTENSION_FLAG] = 0;

    // Checksum: make all 128 bytes sum to 0 mod 256.
    let sum: u32 = edid[..eo::CHECKSUM].iter().map(|&b| b as u32).sum();
    edid[eo::CHECKSUM] = (256 - (sum % 256)) as u8;

    edid
}

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

/// Generate a minimal valid EDID blob (128 bytes).
pub(crate) fn generate_edid(
    width_mm: u16,
    height_mm: u16,
    width_px: u16,
    height_px: u16,
) -> Vec<u8> {
    let mut edid = vec![0u8; 128];
    // Header
    edid[0..8].copy_from_slice(&EDID_HEADER);
    // Manufacturer ID: "XWB" (X11-Web) encoded as 3 5-bit chars
    // X=24, W=23, B=2 -> 0b11000_10111_00010 = 0xC5C2
    edid[8] = 0xC5;
    edid[9] = 0xC2;
    // Product code
    edid[10] = 0x01;
    edid[11] = 0x00;
    // Serial
    edid[12..16].copy_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    // Week 1, Year 2024 (year - 1990 = 34)
    edid[16] = 1;
    edid[17] = 34;
    // EDID version 1.3
    edid[18] = 1;
    edid[19] = 3;
    // Digital input, 8-bit color
    edid[20] = EDID_VIDEO_INPUT_DIGITAL;
    // Max image size (cm)
    edid[21] = (width_mm / 10) as u8;
    edid[22] = (height_mm / 10) as u8;
    // Gamma 2.2 (value = (gamma * 100) - 100 = 120)
    edid[23] = 120;
    // Supported features: RGB color, preferred timing in DTD1
    edid[24] = EDID_FEATURE_SUPPORT;
    // Chromaticity (standard sRGB-ish values)
    edid[25..35].copy_from_slice(&EDID_CHROMATICITY);
    // Established timings
    edid[35] = 0x00;
    edid[36] = 0x00;
    edid[37] = 0x00;
    // Standard timings (8 entries of 0x0101 = unused)
    for i in 0..8 {
        edid[38 + i * 2] = 0x01;
        edid[38 + i * 2 + 1] = 0x01;
    }
    // Detailed Timing Descriptor #1 (bytes 54-71)
    // Pixel clock in 10kHz units
    let pixel_clock: u16 = ((width_px as u32 + 160) * (height_px as u32 + 30) * 60 / 10000) as u16;
    edid[54] = (pixel_clock & 0xFF) as u8;
    edid[55] = ((pixel_clock >> 8) & 0xFF) as u8;
    // H active lower 8 bits
    edid[56] = (width_px & 0xFF) as u8;
    // H blanking lower 8 bits
    edid[57] = 160u8;
    // H active upper 4 : H blanking upper 4
    edid[58] = (((width_px >> 8) & 0x0F) << 4) as u8;
    // V active lower 8 bits
    edid[59] = (height_px & 0xFF) as u8;
    // V blanking lower 8 bits
    edid[60] = 30;
    // V active upper 4 : V blanking upper 4
    edid[61] = (((height_px >> 8) & 0x0F) << 4) as u8;
    // H sync offset, width
    edid[62] = 40;
    edid[63] = 40;
    // V sync offset (4 bits) : V sync width (4 bits)
    edid[64] = 0x36; // offset=3, width=6
                     // Upper bits of sync
    edid[65] = 0x00;
    // Image size mm
    edid[66] = (width_mm & 0xFF) as u8;
    edid[67] = (height_mm & 0xFF) as u8;
    edid[68] = (((width_mm >> 8) & 0x0F) << 4 | ((height_mm >> 8) & 0x0F)) as u8;
    // Border
    edid[69] = 0;
    edid[70] = 0;
    // Flags: non-interlaced, normal display
    edid[71] = EDID_DTD_FLAGS;

    // DTD#2 (bytes 72-89): Monitor name descriptor
    edid[72] = 0x00;
    edid[73] = 0x00;
    edid[74] = 0x00;
    edid[75] = EDID_DESC_TAG_MONITOR_NAME;
    edid[76] = 0x00;
    let name = b"X11-Web\n";
    let end = 77 + name.len().min(13);
    edid[77..end].copy_from_slice(&name[..name.len().min(13)]);
    // Pad with spaces
    for b in &mut edid[end..90] {
        *b = 0x20;
    }

    // DTD#3, DTD#4: unused (zeroed)
    // edid[90..126] already zero

    // Extension count
    edid[126] = 0;

    // Checksum: make all 128 bytes sum to 0 mod 256
    let sum: u32 = edid[..127].iter().map(|&b| b as u32).sum();
    edid[127] = (256 - (sum % 256)) as u8;

    edid
}

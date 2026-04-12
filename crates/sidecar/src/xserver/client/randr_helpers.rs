//! RandR model helpers for ClientState.

use super::super::core::{write_u16_bo, write_u32_bo};
use super::super::types::*;
use super::ClientState;

impl ClientState {
    /// Initialize the default single-monitor RandR model.
    pub(crate) fn randr_init_default(&mut self) {
        use super::super::types::*;

        let crtc_id: u32 = 100;
        let output_id: u32 = 200;
        let mode_id: u32 = 300;
        let provider_id: u32 = 400;

        let mode = RandrMode::new(mode_id, self.screen_width, self.screen_height);
        let crtc = RandrCrtc::new(crtc_id, self.screen_width, self.screen_height, mode_id, output_id);

        // Pre-populate EDID property.
        let edid_atom = self.intern_atom("EDID", false);
        let edid_data = generate_edid(270, 203, self.screen_width, self.screen_height);
        let mut output_props = std::collections::HashMap::new();
        output_props.insert(edid_atom, PropertyValue {
            prop_type: edid_atom,
            format: 8,
            data: edid_data,
        });

        let output = RandrOutput {
            id: output_id,
            name: "default".to_string(),
            connection_status: 0, // Connected
            crtc_id,
            modes: vec![mode_id],
            mm_width: 270,
            mm_height: 203,
            properties: output_props,
            property_configs: std::collections::HashMap::new(),
            possible_crtcs: vec![crtc_id],
        };

        let provider = RandrProvider {
            id: provider_id,
            name: "x11-web".to_string(),
            capabilities: 0x0F,
            crtcs: vec![crtc_id],
            outputs: vec![output_id],
        };

        self.randr_crtcs = vec![crtc];
        self.randr_outputs = vec![output];
        self.randr_modes = vec![mode];
        self.randr_providers = vec![provider];
    }

    /// Find a RandR CRTC by ID.
    pub(crate) fn randr_find_crtc(&self, id: u32) -> Option<&RandrCrtc> {
        self.randr_crtcs.iter().find(|c| c.id == id)
    }

    /// Find a RandR CRTC by ID (mutable).
    pub(crate) fn randr_find_crtc_mut(&mut self, id: u32) -> Option<&mut RandrCrtc> {
        self.randr_crtcs.iter_mut().find(|c| c.id == id)
    }

    /// Find a RandR output by ID.
    pub(crate) fn randr_find_output(&self, id: u32) -> Option<&RandrOutput> {
        self.randr_outputs.iter().find(|o| o.id == id)
    }

    /// Find a RandR output by ID (mutable).
    pub(crate) fn randr_find_output_mut(&mut self, id: u32) -> Option<&mut RandrOutput> {
        self.randr_outputs.iter_mut().find(|o| o.id == id)
    }

    /// Find a RandR mode by ID.
    pub(crate) fn randr_find_mode(&self, id: u32) -> Option<&RandrMode> {
        self.randr_modes.iter().find(|m| m.id == id)
    }

    /// Find a RandR provider by ID.
    #[allow(dead_code)]
    pub(crate) fn randr_find_provider(&self, id: u32) -> Option<&RandrProvider> {
        self.randr_providers.iter().find(|p| p.id == id)
    }

    /// Queue an RRScreenChangeNotify event if the client selected that mask.
    pub(crate) fn randr_queue_screen_change_notify(&mut self) {
        use super::super::types::RANDR_EVENT_BASE;

        if self.randr_event_mask & super::super::types::RR_SCREEN_CHANGE_NOTIFY_MASK == 0 {
            return;
        }

        let bo = self.msb_first;
        let seq = self.sequence;
        let ts = self.timestamp();
        let mut event = [0u8; 32];
        event[0] = RANDR_EVENT_BASE; // RRScreenChangeNotify
        event[1] = 1; // rotation = Rotate_0
        write_u16_bo(&mut event, 2, seq, bo);
        write_u32_bo(&mut event, 4, ts, bo);
        write_u32_bo(&mut event, 8, ts, bo);
        write_u32_bo(&mut event, 12, self.root_window, bo);
        write_u32_bo(&mut event, 16, 0, bo);
        write_u16_bo(&mut event, 20, 0, bo);
        write_u16_bo(&mut event, 22, 0, bo);
        write_u16_bo(&mut event, 24, self.screen_width, bo);
        write_u16_bo(&mut event, 26, self.screen_height, bo);
        write_u16_bo(&mut event, 28, 270, bo);
        write_u16_bo(&mut event, 30, 203, bo);
        self.pending_events.push(event.to_vec());
    }

    /// Queue an RRCrtcChangeNotify event if the client selected that mask.
    pub(crate) fn randr_queue_crtc_change_notify(&mut self, crtc_id: u32) {
        use super::super::core::write_i16_bo;
        use super::super::types::{RANDR_EVENT_BASE, RR_CRTC_CHANGE_NOTIFY_MASK};

        if self.randr_event_mask & RR_CRTC_CHANGE_NOTIFY_MASK == 0 {
            return;
        }

        let crtc = match self.randr_find_crtc(crtc_id) {
            Some(c) => c.clone(),
            None => return,
        };

        let bo = self.msb_first;
        let seq = self.sequence;
        let ts = self.timestamp();

        let mut event = [0u8; 32];
        event[0] = RANDR_EVENT_BASE + 1; // RRNotify
        event[1] = 0; // subtype: CrtcChange
        write_u16_bo(&mut event, 2, seq, bo);
        write_u32_bo(&mut event, 4, ts, bo);
        write_u32_bo(&mut event, 8, self.root_window, bo);
        write_u32_bo(&mut event, 12, crtc.id, bo);
        write_u32_bo(&mut event, 16, crtc.mode_id, bo);
        write_u16_bo(&mut event, 20, crtc.rotation, bo);
        write_i16_bo(&mut event, 24, crtc.x, bo);
        write_i16_bo(&mut event, 26, crtc.y, bo);
        write_u16_bo(&mut event, 28, crtc.width, bo);
        write_u16_bo(&mut event, 30, crtc.height, bo);
        self.pending_events.push(event.to_vec());
    }
}

//! RandR model helpers for ClientState.

use super::super::types::*;
use super::ClientState;
use crate::xserver::event::serialize_event;
use x11rb_protocol::protocol::randr::{
    CrtcChange, Notify, NotifyData, NotifyEvent, Rotation, ScreenChangeNotifyEvent,
};
use x11rb_protocol::protocol::render::SubPixel;

impl ClientState {
    /// Initialize the default single-monitor RandR model.
    pub(crate) fn randr_init_default(&mut self) {
        use super::super::types::*;

        let crtc_id = DEFAULT_RANDR_CRTC_ID;
        let output_id = DEFAULT_RANDR_OUTPUT_ID;
        let mode_id = DEFAULT_RANDR_MODE_ID;
        let provider_id = DEFAULT_RANDR_PROVIDER_ID;

        let mode = RandrMode::new(mode_id, self.screen_width, self.screen_height);
        let crtc = RandrCrtc::new(
            crtc_id,
            self.screen_width,
            self.screen_height,
            mode_id,
            output_id,
        );

        // Pre-populate EDID property.
        let edid_atom = self.intern_atom("EDID", false);
        let edid_data = generate_edid(270, 203, self.screen_width, self.screen_height);
        let mut output_props = std::collections::HashMap::new();
        output_props.insert(
            edid_atom,
            PropertyValue {
                prop_type: edid_atom,
                format: 8,
                data: edid_data,
            },
        );

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

        self.randr.crtcs = vec![crtc];
        self.randr.outputs = vec![output];
        self.randr.modes = vec![mode];
        self.randr.providers = vec![provider];
    }

    /// Find a RandR CRTC by ID.
    pub(crate) fn randr_find_crtc(&self, id: u32) -> Option<&RandrCrtc> {
        self.randr.crtcs.iter().find(|c| c.id == id)
    }

    /// Find a RandR CRTC by ID (mutable).
    pub(crate) fn randr_find_crtc_mut(&mut self, id: u32) -> Option<&mut RandrCrtc> {
        self.randr.crtcs.iter_mut().find(|c| c.id == id)
    }

    /// Find a RandR output by ID.
    pub(crate) fn randr_find_output(&self, id: u32) -> Option<&RandrOutput> {
        self.randr.outputs.iter().find(|o| o.id == id)
    }

    /// Find a RandR output by ID (mutable).
    pub(crate) fn randr_find_output_mut(&mut self, id: u32) -> Option<&mut RandrOutput> {
        self.randr.outputs.iter_mut().find(|o| o.id == id)
    }

    /// Find a RandR mode by ID.
    pub(crate) fn randr_find_mode(&self, id: u32) -> Option<&RandrMode> {
        self.randr.modes.iter().find(|m| m.id == id)
    }

    /// Queue an RRScreenChangeNotify event if the client selected that mask.
    pub(crate) fn randr_queue_screen_change_notify(&mut self) {
        use super::super::types::RANDR_EVENT_BASE;

        if self.randr.event_mask & super::super::types::RR_SCREEN_CHANGE_NOTIFY_MASK == 0 {
            return;
        }

        let bo = self.msb_first;
        let seq = self.sequence;
        let ts = self.timestamp();
        let event = serialize_event(
            &ScreenChangeNotifyEvent {
                response_type: RANDR_EVENT_BASE,
                rotation: Rotation::ROTATE0,
                sequence: seq,
                timestamp: ts,
                config_timestamp: ts,
                root: self.root_window,
                request_window: 0,
                size_id: 0,
                subpixel_order: SubPixel::from(0u8),
                width: self.screen_width,
                height: self.screen_height,
                mwidth: 270,
                mheight: 203,
            },
            bo,
        );
        self.pending_events.push(event);
    }

    /// Queue an RRCrtcChangeNotify event if the client selected that mask.
    pub(crate) fn randr_queue_crtc_change_notify(&mut self, crtc_id: u32) {
        use super::super::types::{RANDR_EVENT_BASE, RR_CRTC_CHANGE_NOTIFY_MASK};

        if self.randr.event_mask & RR_CRTC_CHANGE_NOTIFY_MASK == 0 {
            return;
        }

        let crtc = match self.randr_find_crtc(crtc_id) {
            Some(c) => c.clone(),
            None => return,
        };

        let bo = self.msb_first;
        let seq = self.sequence;
        let ts = self.timestamp();

        let event = serialize_event(
            &NotifyEvent {
                response_type: RANDR_EVENT_BASE + 1,
                sub_code: Notify::CRTC_CHANGE,
                sequence: seq,
                u: NotifyData::from(CrtcChange {
                    timestamp: ts,
                    window: self.root_window,
                    crtc: crtc.id,
                    mode: crtc.mode_id,
                    rotation: Rotation::from(crtc.rotation),
                    x: crtc.x,
                    y: crtc.y,
                    width: crtc.width,
                    height: crtc.height,
                }),
            },
            bo,
        );
        self.pending_events.push(event);
    }
}

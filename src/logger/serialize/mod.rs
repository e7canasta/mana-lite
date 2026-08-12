use super::event::Event;

mod basic;
mod control;
mod detection;
mod writers;

use basic::{
    write_frame_event, write_health_event, write_meta_event, write_metrics_event,
    write_scan_deadline_event,
};
use control::{
    write_entity_event, write_face_dwell_event, write_fsm_event, write_presence_event,
    write_scene_signals_event, write_zone_event,
};
use detection::{
    write_consolidated_detection_event, write_depth_event, write_depth_region_event,
    write_detection_event,
};

pub fn write_event(event: &Event, ts: &str, buf: &mut Vec<u8>) {
    buf.clear();
    buf.extend_from_slice(b"{\"t\":\"");
    buf.extend_from_slice(ts.as_bytes());
    buf.extend_from_slice(b"\",");
    match event {
        Event::Meta { .. } => write_meta_event(event, buf),
        Event::Health { .. } => write_health_event(event, buf),
        Event::ScanDeadline { .. } => write_scan_deadline_event(event, buf),
        Event::Frame { .. } => write_frame_event(event, buf),
        Event::Detection { .. } => write_detection_event(event, buf),
        Event::Depth { .. } => write_depth_event(event, buf),
        Event::DepthRegion { .. } => write_depth_region_event(event, buf),
        Event::ConsolidatedDetection { .. } => write_consolidated_detection_event(event, buf),
        Event::Entity { .. } => write_entity_event(event, buf),
        Event::Zone { .. } => write_zone_event(event, buf),
        Event::Fsm { .. } => write_fsm_event(event, buf),
        Event::Presence { .. } => write_presence_event(event, buf),
        Event::SceneSignals { .. } => write_scene_signals_event(event, buf),
        Event::FaceDwell { .. } => write_face_dwell_event(event, buf),
        Event::Metrics(_) => write_metrics_event(event, buf),
    }
    buf.extend_from_slice(b"}\n");
}

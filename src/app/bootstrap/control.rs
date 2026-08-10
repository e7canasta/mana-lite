//! Stage 4: build control state (clocks, health, FSM engine, policy).

use std::time::Instant;

use crate::config::AppConfig;
use crate::fsm::{FsmEngine, FsmProgram, FsmSceneContext};
use crate::health::Health;
use crate::scan::{ControlPolicy, ControlState};
use mana_control::domain::LoopId;
use mana_control::scan::ScanTimeline;

use super::perception::PerceptionEngines;

pub(super) struct ControlBundle {
    pub(super) control: ControlState,
    pub(super) scan_timeline: ScanTimeline,
    pub(super) boot_wall: chrono::DateTime<chrono::Utc>,
    pub(super) boot_instant: Instant,
}

/// Anchor boot clocks and assemble [`ControlState`] + scan timeline.
pub(super) fn build_control_state(
    config: &AppConfig,
    fsm_program: Option<FsmProgram>,
    perception: &mut PerceptionEngines,
) -> ControlBundle {
    // Ancla del reloj: una sola lectura de pared y una de monotónico, en
    // el mismo punto. Todo timestamp del proceso sale de aqui + delta del
    // monotónico; un salto de NTP no puede desordenar ni duplicar el JSONL.
    // Re-anclar en cada rotación horaria del log queda deliberadamente en
    // pendiente: la deriva de ppm del monotónico es despreciable frente a
    // la rotación del sistema, y un re-anclaje mal hecho reabriría el salto.
    let boot_wall = chrono::Utc::now();
    let boot_instant = Instant::now();
    let loop_id = LoopId::default_loop();
    let health = Health::new_at(
        config.health.data_stale_ms,
        config.health.stale_warn_ms,
        boot_instant,
    );
    let fsm_engine = config
        .pipeline
        .fsm
        .then(|| fsm_program.map(|program| FsmEngine::from_program_at(program, boot_instant)))
        .flatten();
    let scan_timeline = ScanTimeline::new(loop_id.clone(), boot_instant, config.scan.period_ms);

    let control = ControlState {
        loop_id,
        tracker: perception.tracker.take(),
        zone_engine: perception.zone_engine.take(),
        fsm_engine,
        health,
        presence: crate::presence::PresenceFilter::new(
            config.presence.enabled,
            config.presence.class.clone(),
            mana_control::config::PresencePoiPolicy {
                on_ms: config.presence.poi.on_ms,
                off_ms: config.presence.poi.off_ms,
            },
        ),
        occupancy: crate::occupancy::OccupancyStateMachine::new(
            mana_control::config::OccupancyPolicy {
                single_confirm_ms: config.presence.occupancy.single_confirm_ms,
                empty_confirm_ms: config.presence.occupancy.empty_confirm_ms,
                multiple_confirm_ms: config.presence.occupancy.multiple_confirm_ms,
                multiple_exit_ms: config.presence.occupancy.multiple_exit_ms,
                require_confirmed_tracks: config.presence.occupancy.require_confirmed_tracks,
            },
        ),
        fsm_context: FsmSceneContext::default(),
        last_scan_at: boot_instant,
        scan_seq: 0,
        policy: ControlPolicy {
            person_class: config.presence.class.as_str().into(),
            presence_enabled: config.presence.enabled,
            data_stale_ms: config.health.data_stale_ms,
            scan_period_ms: config.scan.period_ms,
            face_dwell_roi: perception.face_dwell_roi.map(|x| x.to_array()),
            person_detection_roi: perception.person_detection_roi.map(|x| x.to_array()),
            face_edge_margin_px: config.detection.face_edge_margin_px,
        },
    };

    ControlBundle {
        control,
        scan_timeline,
        boot_wall,
        boot_instant,
    }
}

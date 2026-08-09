use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct FsmCatalog {
    pub fsm: FsmRoot,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FsmRoot {
    pub initial: String,
    pub states: HashMap<String, FsmState>,
    pub roles: FsmRoles,
    #[serde(default)]
    pub transitions: Vec<FsmTransition>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FsmRoles {
    pub safe: String,
    pub reset: String,
    #[serde(default)]
    pub latch_set: Vec<String>,
    #[serde(default)]
    pub latch_maybe: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FsmState {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub dwell_min_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FsmTransition {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub guards: Vec<FsmGuard>,
    #[serde(default)]
    pub dwell: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum FsmGuard {
    /// A persistent zone condition. Unlike `zone_occupied` this does not
    /// depend on the one-frame edge event emitted when a zone changes state.
    #[serde(rename = "zone_present")]
    ZonePresent { zone: String },
    #[serde(rename = "zone_occupied")]
    ZoneOccupied {
        zone: String,
        #[serde(default = "default_guard_confidence")]
        min_confidence: f32,
        #[serde(default)]
        min_duration_ms: Option<u64>,
    },
    #[serde(rename = "zone_vacated")]
    ZoneVacated {
        zone: String,
        #[serde(default = "default_guard_confidence")]
        min_confidence: f32,
        #[serde(default)]
        min_duration_ms: Option<u64>,
    },
    #[serde(rename = "all_zones_vacant")]
    AllZonesVacant {
        #[serde(default)]
        min_duration_ms: Option<u64>,
    },
    #[serde(rename = "data_stale")]
    DataStale,
    /// Complemento de `data_stale`: la senal volvio. Necesario para salir de
    /// un estado de seguridad — en TOML no era expresable.
    #[serde(rename = "data_fresh")]
    DataFresh,
    /// Regla depth por nombre (spec depth-standard §9/§15). `triggered`
    /// invierte la condicion (por defecto exige regla disparada). Sin
    /// evidencia de la regla en el frame, el guard es falso.
    #[serde(rename = "depth_rule")]
    DepthRule {
        rule: String,
        #[serde(default = "default_guard_triggered")]
        triggered: bool,
    },
    /// Scene guards used by blueprint-specific face state machines.
    #[serde(rename = "cardinality")]
    Cardinality { value: String },
    #[serde(rename = "person_present")]
    PersonPresent,
    #[serde(rename = "person_absent")]
    PersonAbsent,
    #[serde(rename = "face_detected")]
    FaceDetected {
        #[serde(default = "default_guard_confidence")]
        min_confidence: f32,
    },
    #[serde(rename = "face_absent")]
    FaceAbsent,
    #[serde(rename = "face_in_dwell")]
    FaceInDwell,
    #[serde(rename = "face_not_in_dwell")]
    FaceNotInDwell,
    #[serde(rename = "face_at_edge")]
    FaceAtEdge,
    #[serde(rename = "face_not_at_edge")]
    FaceNotAtEdge,
    #[serde(rename = "face_was_inside")]
    FaceWasInside,
    #[serde(rename = "face_was_not_inside")]
    FaceWasNotInside,
}

fn default_guard_triggered() -> bool {
    true
}

fn default_guard_confidence() -> f32 {
    0.5
}

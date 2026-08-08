use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Default, Deserialize)]
pub struct ZoneCatalog {
    #[serde(default)]
    pub zones: HashMap<String, ZoneEntry>,
    #[serde(default)]
    pub face_dwell: Option<ZoneEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ZoneEntry {
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_hysteresis")]
    pub hysteresis_ms: u64,
}

impl ZoneEntry {
    pub fn rect(&self) -> [u32; 4] {
        [self.x1, self.y1, self.x2, self.y2]
    }
}

fn default_hysteresis() -> u64 {
    500
}

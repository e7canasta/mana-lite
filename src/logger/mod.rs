mod event;
mod serialize;

#[allow(unused_imports)]
pub use event::{DetRecord, Event, JsonlLevel, MaskRecord};
use serialize::write_event;

use crate::config::{MetricsJsonlConfig, Rotate};
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

pub struct Logger {
    buffer: Vec<Event>,
    started_at: Instant,
    target: OutputTarget,
    level: JsonlLevel,
    jsonl_config: MetricsJsonlConfig,
}

enum OutputTarget {
    Stdout(BufWriter<io::Stdout>),
    Rotating {
        dir: PathBuf,
        writer: BufWriter<fs::File>,
        current_hour: u32,
        current_day: u32,
    },
}

impl Logger {
    pub fn new(level: JsonlLevel) -> Self {
        Self {
            buffer: Vec::with_capacity(32),
            started_at: Instant::now(),
            target: OutputTarget::Stdout(BufWriter::new(io::stdout())),
            level,
            jsonl_config: MetricsJsonlConfig::default(),
        }
    }

    pub fn rotating(dir: PathBuf, rotate: &Rotate, level: JsonlLevel) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let (writer, hour, day) = match rotate {
            Rotate::Hourly => {
                let (w, h, _) = open_file(&dir, "%Y%m%dT%H")?;
                (w, h, 0)
            }
            Rotate::Daily => {
                let (w, _, d) = open_file(&dir, "%Y%m%d")?;
                (w, 0, d)
            }
            Rotate::Never => {
                let (w, _, _) = open_file(&dir, "all")?;
                (w, 0, 0)
            }
        };
        Ok(Self {
            buffer: Vec::with_capacity(32),
            started_at: Instant::now(),
            target: OutputTarget::Rotating {
                dir,
                writer,
                current_hour: hour,
                current_day: day,
            },
            level,
            jsonl_config: MetricsJsonlConfig::default(),
        })
    }

    pub fn set_jsonl_config(&mut self, config: MetricsJsonlConfig) {
        self.jsonl_config = config;
    }

    pub fn emit(&mut self, event: Event) {
        if self.level.allows(event.min_level()) && self.jsonl_config.allows(&event) {
            self.buffer.push(event);
        }
    }

    pub fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let events: Vec<Event> = self.buffer.drain(..).collect();
        let mut buf = Vec::with_capacity(512);
        for event in &events {
            write_event(event, &now, &mut buf);
            if let Err(e) = self.write_buf(&buf) {
                log::warn!("logger flush write failed: {e}");
            }
        }
    }

    fn write_buf(&mut self, buf: &[u8]) -> io::Result<()> {
        match &mut self.target {
            OutputTarget::Stdout(w) => {
                w.write_all(buf)?;
                w.flush()
            }
            OutputTarget::Rotating {
                dir,
                writer,
                current_hour,
                current_day,
            } => {
                let now = chrono::Utc::now();
                let rotate_needed = match *current_hour {
                    0 if *current_day > 0 => {
                        // daily mode
                        let today = now.format("%Y%m%d").to_string().parse::<u32>().unwrap_or(0);
                        today != *current_day
                    }
                    _ => {
                        // hourly mode or never (current_day == 0)
                        let this_hour = now.format("%H").to_string().parse::<u32>().unwrap_or(0);
                        this_hour != *current_hour
                    }
                };
                if rotate_needed {
                    let (new_writer, h, d) = if *current_hour == 0 && *current_day == 0 {
                        let (w, _, _) = open_file(dir, "all")?;
                        (w, 0, 0)
                    } else if *current_day > 0 {
                        let (w, _, d) = open_file(dir, "%Y%m%d")?;
                        (w, 0, d)
                    } else {
                        let (w, h, _) = open_file(dir, "%Y%m%dT%H")?;
                        (w, h, 0)
                    };
                    *writer = new_writer;
                    *current_hour = h;
                    *current_day = d;
                }
                writer.write_all(buf)?;
                writer.flush()
            }
        }
    }

    pub fn shutdown(&mut self, reason: &str) {
        self.emit(Event::Meta {
            event: "shutdown".into(),
            detail: reason.into(),
            attrs: vec![(
                "uptime".into(),
                self.started_at.elapsed().as_secs().to_string(),
            )],
        });
        self.flush();
    }

    #[doc(hidden)]
    #[allow(dead_code)]
    pub fn flush_to_buffer(&mut self, out: &mut Vec<u8>) {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let events: Vec<Event> = self.buffer.drain(..).collect();
        for event in &events {
            write_event(event, &now, out);
        }
    }
}

impl MetricsJsonlConfig {
    fn allows(&self, event: &Event) -> bool {
        match event {
            Event::Frame { .. } => self.frame_events,
            Event::Detection { .. } | Event::ConsolidatedDetection { .. } => self.detection_events,
            Event::Depth { .. } | Event::DepthRegion { .. } => self.depth_events,
            Event::Zone { .. } => self.zone_events,
            Event::Fsm { .. } => self.fsm_events,
            Event::Metrics(_) => self.metrics_event,
            Event::Meta { .. } | Event::Health { .. } | Event::Entity { .. } => true,
        }
    }
}

fn open_file(dir: &PathBuf, fmt: &str) -> io::Result<(BufWriter<fs::File>, u32, u32)> {
    let now = chrono::Utc::now();
    let filename = format!("mana-{}.jsonl", now.format(fmt));
    let path = dir.join(&filename);
    let hour = now.format("%H").to_string().parse::<u32>().unwrap_or(0);
    let day = now.format("%Y%m%d").to_string().parse::<u32>().unwrap_or(0);
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    log::info!("logger: {}", path.display());
    Ok((BufWriter::new(file), hour, day))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_logger() -> Logger {
        Logger::new(JsonlLevel::Debug)
    }

    fn collect(logger: &mut Logger) -> String {
        let mut buf = Vec::new();
        logger.flush_to_buffer(&mut buf);
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn depth_event_v2_allows_nullable_fields() {
        let mut log = test_logger();
        log.emit(Event::depth(
            1,
            "depth-standard",
            1,
            1,
            None,
            0,
            0,
            0,
            None,
            None,
            None,
        ));
        let out = collect(&mut log);
        assert!(out.contains("\"version\":2"));
        assert!(out.contains("\"roi\":null"));
        assert!(out.contains("\"valid_ratio\":null"));
        assert!(out.contains("\"min_depth_m\":null"));
        assert!(out.contains("\"max_depth_m\":null"));
    }

    #[test]
    fn depth_region_event_serializes_evidence() {
        let mut log = test_logger();
        log.emit(Event::depth_region(
            42,
            "bed-approach",
            [560, 140, 1240, 820],
            "median",
            Some(1.2),
            1.5,
            true,
            462400,
            Some(1.0),
        ));
        let out = collect(&mut log);
        assert!(out.contains("\"type\":\"depth_region\""));
        assert!(out.contains("\"version\":1"));
        assert!(out.contains("\"rule\":\"bed-approach\""));
        assert!(out.contains("\"region\":[560,140,1240,820]"));
        assert!(out.contains("\"metric\":\"median\""));
        assert!(out.contains("\"value\":1.2"));
        assert!(out.contains("\"threshold_m\":1.5"));
        assert!(out.contains("\"triggered\":true"));
        assert!(out.contains("\"valid_pixels\":462400"));
        assert!(out.contains("\"valid_ratio\":1"));
    }

    #[test]
    fn depth_region_event_allows_null_value() {
        let mut log = test_logger();
        log.emit(Event::depth_region(
            1,
            "bed-approach",
            [0, 0, 1, 1],
            "median",
            None,
            1.5,
            false,
            0,
            None,
        ));
        let out = collect(&mut log);
        assert!(out.contains("\"value\":null"));
        assert!(out.contains("\"triggered\":false"));
        assert!(out.contains("\"valid_ratio\":null"));
    }

    #[test]
    fn depth_event_v2_has_version_roi_and_map_dims() {
        let mut log = test_logger();
        log.emit(Event::depth(
            123,
            "depth-standard",
            268,
            281,
            Some([560, 140, 1240, 820]),
            680,
            680,
            462400,
            Some(1.0),
            Some(2.19),
            Some(5.16),
        ));
        let out = collect(&mut log);
        assert!(out.contains("\"type\":\"depth\""));
        assert!(out.contains("\"version\":2"));
        assert!(out.contains("\"roi\":[560,140,1240,820]"));
        assert!(out.contains("\"map_width\":680"));
        assert!(out.contains("\"map_height\":680"));
        assert!(out.contains("\"valid_pixels\":462400"));
        assert!(out.contains("\"valid_ratio\":1"));
        assert!(out.contains("\"min_depth_m\":2.19"));
        assert!(out.contains("\"max_depth_m\":5.16"));
        assert!(!out.contains("\"width\":680"));
    }

    #[test]
    fn detection_emits_class_and_bbox() {
        let det = vec![DetRecord {
            class: "person".into(),
            confidence: 0.87,
            bbox: [100.0, 200.0, 300.0, 500.0],
            mask: None,
        }];
        let mut log = test_logger();
        log.emit(Event::detection(
            1,
            "detect-fast",
            52,
            60,
            det,
            0,
            0,
            None,
            None,
        ));
        let out = collect(&mut log);
        assert!(out.contains("\"type\":\"detection\""));
        assert!(out.contains("\"pipeline_ms\":60"));
        assert!(out.contains("\"class\":\"person\""));
        assert!(out.contains("\"confidence\":0.87"));
        assert!(out.contains("\"bbox\":[100,200,300,500]"));
    }

    #[test]
    fn detection_emits_mask_wire_record() {
        use crate::infer::DetectionMask;
        use mana_geometry::compact_mask::CompactMask;
        use std::sync::Arc;
        let compact = CompactMask::from_dense(&[1, 1, 1, 1], 2, 2, (3, 4), (10, 10)).unwrap();
        let mask = DetectionMask {
            compact: Arc::new(compact),
            polygons: Arc::new(vec![vec![[0.25, 0.25], [0.75, 0.25], [0.75, 0.75]]]),
            origin: [0, 0],
            mask_dims: [10, 10],
        };
        let record = mask.to_wire_record();
        let det = vec![DetRecord {
            class: "person".into(),
            confidence: 0.9,
            bbox: [3.0, 4.0, 5.0, 6.0],
            mask: Some(record),
        }];
        let mut log = test_logger();
        log.emit(Event::detection(
            1,
            "seg-standard",
            52,
            60,
            det,
            0,
            0,
            None,
            None,
        ));
        let out = collect(&mut log);
        assert!(out.contains("\"mask\":{\"rle\":["), "missing rle: {out}");
        assert!(
            out.contains("\"bbox\":[3,4,5,6]"),
            "missing mask bbox: {out}"
        );
        assert!(out.contains("\"origin\":[0,0]"));
        assert!(out.contains("\"mask_dims\":[10,10]"));
        assert!(out.contains("\"polygons\":[[[0.25,0.25],[0.75,0.25],[0.75,0.75]]]"));
    }

    #[test]
    fn mask_wire_record_round_trips_through_rle() {
        use crate::infer::DetectionMask;
        use mana_geometry::compact_mask::CompactMask;
        use std::sync::Arc;
        let compact = CompactMask::from_dense(&[1, 1, 1, 1], 2, 2, (3, 4), (10, 10)).unwrap();
        let mask = DetectionMask {
            compact: Arc::new(compact),
            polygons: Arc::new(vec![]),
            origin: [0, 0],
            mask_dims: [10, 10],
        };
        let record = mask.to_wire_record();
        let rebuilt = vernier_mask::Rle::from_counts(
            (record.bbox[3] - record.bbox[1]) as u32,
            (record.bbox[2] - record.bbox[0]) as u32,
            record.rle.clone(),
        );
        let raster = rebuilt.to_raster_bytes();
        assert_eq!(
            raster,
            vec![1, 1, 1, 1],
            "lossless round-trip of mask raster"
        );
    }

    #[test]
    fn jsonl_config_filters_optional_events() {
        let mut log = test_logger();
        log.set_jsonl_config(MetricsJsonlConfig {
            frame_events: false,
            ..MetricsJsonlConfig::default()
        });
        log.emit(Event::frame_ingest(1, true, 10, 100));
        log.emit(Event::detection(
            1,
            "detect-fast",
            10,
            12,
            Vec::new(),
            0,
            0,
            None,
            None,
        ));

        let out = collect(&mut log);
        assert!(!out.contains("\"type\":\"frame\""));
        assert!(out.contains("\"type\":\"detection\""));
    }

    #[test]
    fn consolidated_detection_has_no_track_id() {
        let mut log = test_logger();
        log.emit(Event::consolidated_detection(
            4,
            "person",
            0.91,
            [10.0, 20.0, 110.0, 220.0],
            "detect-fast",
            vec!["detect-fast".into(), "pose-standard".into()],
        ));
        let out = collect(&mut log);
        assert!(out.contains("\"type\":\"consolidated_detection\""));
        assert!(out.contains("\"primary_model\":\"detect-fast\""));
        assert!(out.contains("\"sources\":[\"detect-fast\",\"pose-standard\"]"));
        assert!(!out.contains("track_id"));
    }

    #[test]
    fn fsm_transition_has_from_to_trigger() {
        let mut log = test_logger();
        log.emit(Event::fsm_transition(
            "idle",
            None,
            "monitoring",
            None,
            "bed_occupied",
            0,
        ));
        let out = collect(&mut log);
        assert!(out.contains("\"type\":\"fsm\""));
        assert!(out.contains("\"from\":\"idle\""));
        assert!(out.contains("\"to\":\"monitoring\""));
        assert!(out.contains("\"trigger\":\"bed_occupied\""));
    }

    #[test]
    fn health_blind_has_message() {
        let mut log = test_logger();
        log.emit(Event::health_blind(10_000));
        let out = collect(&mut log);
        assert!(out.contains("\"type\":\"health\""));
        assert!(out.contains("\"event\":\"blind\""));
        assert!(out.contains("10000ms"));
    }

    #[test]
    fn escape_json_string_quotes_and_backslash() {
        let mut log = test_logger();
        log.emit(Event::Meta {
            event: "test".into(),
            detail: "say \"hello\"".into(),
            attrs: vec![("path".into(), "C:\\Users\\test".into())],
        });
        let out = collect(&mut log);
        assert!(out.contains("say \\\"hello\\\""));
        assert!(out.contains("C:\\\\Users\\\\test"));
    }

    #[test]
    fn escape_json_control_chars() {
        let mut log = test_logger();
        log.emit(Event::Meta {
            event: "test".into(),
            detail: "line1\nline2".into(),
            attrs: vec![],
        });
        let out = collect(&mut log);
        assert!(out.contains("line1\\nline2"));
    }

    #[test]
    fn negative_float_is_valid_json() {
        let det = vec![DetRecord {
            class: "x".into(),
            confidence: 0.5,
            bbox: [-10.5, 0.0, 100.0, 200.25],
            mask: None,
        }];
        let mut log = test_logger();
        log.emit(Event::detection(1, "m", 10, 12, det, 0, 0, None, None));
        let out = collect(&mut log);
        assert!(out.contains("\"bbox\":[-10.5,0,100,200.25]"));
    }

    #[test]
    fn depth_emits_dimensions_and_finite_stats() {
        let mut log = test_logger();
        log.emit(Event::depth(
            7,
            "depth-standard",
            180,
            190,
            Some([560, 140, 1240, 820]),
            680,
            680,
            462400,
            Some(0.9999),
            Some(0.42),
            Some(8.31),
        ));
        let out = collect(&mut log);
        assert!(out.contains("\"type\":\"depth\""));
        assert!(out.contains("\"version\":2"));
        assert!(out.contains("\"roi\":[560,140,1240,820]"));
        assert!(out.contains("\"map_width\":680"));
        assert!(out.contains("\"map_height\":680"));
        assert!(out.contains("\"valid_pixels\":462400"));
        assert!(out.contains("\"valid_ratio\":0.9999"));
        assert!(out.contains("\"min_depth_m\":0.42"));
        assert!(out.contains("\"max_depth_m\":8.31"));
    }

    #[test]
    fn depth_emits_null_stats_when_map_is_empty() {
        let mut log = test_logger();
        log.emit(Event::depth(
            8,
            "depth-standard",
            10,
            12,
            None,
            0,
            0,
            0,
            None,
            None,
            None,
        ));
        let out = collect(&mut log);
        assert!(out.contains("\"roi\":null"));
        assert!(out.contains("\"valid_pixels\":0"));
        assert!(out.contains("\"valid_ratio\":null"));
        assert!(out.contains("\"min_depth_m\":null"));
        assert!(out.contains("\"max_depth_m\":null"));
    }
}

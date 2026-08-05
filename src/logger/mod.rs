mod event;
mod serialize;

#[allow(unused_imports)]
pub use event::{DetRecord, Event, JsonlLevel};
use serialize::write_event;

use crate::config::Rotate;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

pub struct Logger {
    buffer: Vec<Event>,
    started_at: Instant,
    target: OutputTarget,
    level: JsonlLevel,
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
            target: OutputTarget::Rotating { dir, writer, current_hour: hour, current_day: day },
            level,
        })
    }

    pub fn emit(&mut self, event: Event) {
        if self.level.allows(event.min_level()) {
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
            OutputTarget::Rotating { dir, writer, current_hour, current_day } => {
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
    fn meta_startup_has_type_and_version() {
        let mut log = test_logger();
        log.emit(Event::meta_startup("0.1.0", "mana.toml"));
        let out = collect(&mut log);
        assert!(out.contains("\"type\":\"meta\""));
        assert!(out.contains("\"event\":\"startup\""));
        assert!(out.contains("\"version\":\"0.1.0\""));
    }

    #[test]
    fn detection_emits_class_and_bbox() {
        let det = vec![DetRecord { class: "person".into(), confidence: 0.87, bbox: [100.0, 200.0, 300.0, 500.0] }];
        let mut log = test_logger();
        log.emit(Event::detection(1, "detect-fast", 52, det, None, None));
        let out = collect(&mut log);
        assert!(out.contains("\"type\":\"detection\""));
        assert!(out.contains("\"class\":\"person\""));
        assert!(out.contains("\"confidence\":0.87"));
        assert!(out.contains("\"bbox\":[100,200,300,500]"));
    }

    #[test]
    fn fsm_transition_has_from_to_trigger() {
        let mut log = test_logger();
        log.emit(Event::fsm_transition("idle", None, "monitoring", None, "bed_occupied", 0));
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
        let det = vec![DetRecord { class: "x".into(), confidence: 0.5, bbox: [-10.5, 0.0, 100.0, 200.25] }];
        let mut log = test_logger();
        log.emit(Event::detection(1, "m", 10, det, None, None));
        let out = collect(&mut log);
        assert!(out.contains("\"bbox\":[-10.5,0,100,200.25]"));
    }
}

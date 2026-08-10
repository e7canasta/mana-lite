mod event;
mod serialize;

#[allow(unused_imports)]
pub use event::{
    DetRecord, Event, FaceDwellTimerRecord, JsonlLevel, MaskRecord, scene_events_to_log,
    track_event_to_log, zone_event_to_log,
};
use serialize::write_event;

use crate::config::{MetricsJsonlConfig, Rotate};
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

/// Port used by pipeline code to publish domain events without depending on a
/// concrete output backend.
pub trait LogSink {
    fn emit(&mut self, event: Event);
    fn flush(&mut self);
    fn shutdown(&mut self, reason: &str);
}

/// In-memory sink for deterministic event assertions in tests.
#[derive(Debug, Default)]
pub struct RecordingSink {
    pub events: Vec<Event>,
}

impl LogSink for RecordingSink {
    fn emit(&mut self, event: Event) {
        self.events.push(event);
    }

    fn flush(&mut self) {}

    fn shutdown(&mut self, _reason: &str) {}
}

/// Render events as JSONL with a caller-provided deterministic timestamp.
#[must_use]
pub fn render_events_fixed_ts(events: &[Event], ts: &str) -> String {
    let mut line = Vec::new();
    let mut output = String::new();
    for event in events {
        write_event(event, ts, &mut line);
        output.push_str(std::str::from_utf8(&line).expect("event JSON is UTF-8"));
    }
    output
}

/// Adapter contract for output backends. A manager can fan one event out to
/// several handlers, such as a JSONL file and a narrow diagnostic stream.
pub trait LogHandler {
    fn handle(&mut self, event: Event);
    fn flush(&mut self);
    fn configure_jsonl(&mut self, _config: MetricsJsonlConfig) {}
    #[doc(hidden)]
    fn flush_to_buffer(&mut self, _out: &mut Vec<u8>) {}
}

pub struct LogManager {
    handlers: Vec<Box<dyn LogHandler>>,
    started_at: Instant,
}

pub struct JsonlHandler {
    buffer: Vec<Event>,
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

impl LogManager {
    pub fn new(level: JsonlLevel) -> Self {
        Self::with_handler(JsonlHandler::new(level))
    }

    pub fn rotating(dir: PathBuf, rotate: &Rotate, level: JsonlLevel) -> io::Result<Self> {
        Ok(Self::with_handler(JsonlHandler::rotating(
            dir, rotate, level,
        )?))
    }

    fn with_handler<H: LogHandler + 'static>(handler: H) -> Self {
        Self::with_handlers(vec![Box::new(handler)])
    }

    pub fn with_handlers(handlers: Vec<Box<dyn LogHandler>>) -> Self {
        Self {
            handlers,
            started_at: Instant::now(),
        }
    }

    pub fn set_jsonl_config(&mut self, config: MetricsJsonlConfig) {
        for handler in &mut self.handlers {
            handler.configure_jsonl(config.clone());
        }
    }

    #[doc(hidden)]
    #[allow(dead_code)]
    pub fn flush_to_buffer(&mut self, out: &mut Vec<u8>) {
        for handler in &mut self.handlers {
            handler.flush_to_buffer(out);
        }
    }
}

impl LogSink for LogManager {
    fn emit(&mut self, event: Event) {
        let last = self.handlers.len().saturating_sub(1);
        let mut event = Some(event);
        for (index, handler) in self.handlers.iter_mut().enumerate() {
            if index == last {
                handler.handle(event.take().expect("last log handler owns event"));
            } else {
                handler.handle(event.as_ref().expect("log event not consumed").clone());
            }
        }
    }

    fn flush(&mut self) {
        for handler in &mut self.handlers {
            handler.flush();
        }
    }

    fn shutdown(&mut self, reason: &str) {
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
}

impl Drop for LogManager {
    fn drop(&mut self) {
        LogSink::flush(self);
    }
}

impl<T: LogSink + ?Sized> LogSink for Box<T> {
    fn emit(&mut self, event: Event) {
        (**self).emit(event);
    }

    fn flush(&mut self) {
        (**self).flush();
    }

    fn shutdown(&mut self, reason: &str) {
        (**self).shutdown(reason);
    }
}

impl JsonlHandler {
    pub fn new(level: JsonlLevel) -> Self {
        Self {
            buffer: Vec::with_capacity(32),
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

    fn handle_event(&mut self, event: Event) {
        if self.level.allows(event.min_level()) && self.jsonl_config.allows(&event) {
            self.buffer.push(event);
        }
    }

    fn flush_buffer(&mut self) {
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

    fn copy_buffer(&mut self, out: &mut Vec<u8>) {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let events: Vec<Event> = self.buffer.drain(..).collect();
        for event in &events {
            write_event(event, &now, out);
        }
    }
}

impl LogHandler for JsonlHandler {
    fn handle(&mut self, event: Event) {
        self.handle_event(event);
    }

    fn flush(&mut self) {
        self.flush_buffer();
    }

    fn configure_jsonl(&mut self, config: MetricsJsonlConfig) {
        self.jsonl_config = config;
    }

    fn flush_to_buffer(&mut self, out: &mut Vec<u8>) {
        self.copy_buffer(out);
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
            Event::Presence { .. } => self.presence_events,
            Event::FaceDwell { .. } => self.face_dwell_events,
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
mod tests;

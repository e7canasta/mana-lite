Here are the complete findings for all three items:

---

## 1. Per-model text log lines (e.g., `detect-fast: 0.4 Hz | 2 calls | 15ms avg | 10-22ms | 6/5fr`)

### Where the format string is constructed and printed

**File:** `/home/care/opt/workspace/references/mana-lite/src/pipeline.rs`, lines **125–148**

```rust
fn log_per_model(name: &str, m: &PerModelMetrics, window_s: u64) {
    let m_hz = hz(m.inferences, window_s);
    let m_avg = avg_ms(m.infer_total_us / 1000, m.inferences);
    let m_range = if m.inferences > 0 {
        format!("{}-{}ms", m.infer_min_us / 1000, m.infer_max_us / 1000)
    } else {
        "---".into()
    };
    let m_ratio = if window_s > 0 {
        format!("{}/{}fr", m.total_dets, window_s)
    } else {
        "?/0".into()
    };
    let flags: Vec<&str> = vec![
        if m.skips > 0 { Some("skip") } else { None },
        if m.empty > 0 { Some("empty") } else { None },
    ].into_iter().flatten().collect();
    let flag_str = if flags.is_empty() { String::new() } else { format!(" | {}", flags.join(",")) };

    log::info!(
        "  {:>16}: {:.1} Hz | {} calls | {} ({}) | {}{}",
        name, m_hz, m.inferences, m_avg, m_range, m_ratio, flag_str,
    );
}
```

The format is: `"  {:>16}: {:.1} Hz | {} calls | {} ({}) | {}{}"` which produces output like:

```
  detect-fast: 0.4 Hz | 2 calls | 15ms (10-22ms) | 6/5fr
```

### Where it is called from

**File:** `/home/care/opt/workspace/references/mana-lite/src/pipeline.rs`, lines **118–122**

```rust
    for name in model_order {
        if let Some(m) = report.model_metrics.get(name) {
            log_per_model(name, m, report.window_s);
        }
    }
```

This is inside `log_infer_line` (line 96), which is called from `log_report` (line 150–153):

```rust
fn log_report(report: &MetricsReport, model_order: &[String]) {
    log_ingest_line(report);
    log_infer_line(report, model_order);
}
```

### Config flag that controls this

**File:** `/home/care/opt/workspace/references/mana-lite/src/config.rs`, line **436**

```rust
#[serde(default = "default_true")] pub per_model_lines: bool,
```

---

## 2. JSONL detection event serialization (`{"type":"detection", ...}`)

### The `Detection` event variant (struct)

**File:** `/home/care/opt/workspace/references/mana-lite/src/logger/event.rs`, lines **21–27**

```rust
    Detection {
        frame_id: u64,
        model: String,
        infer_ms: u64,
        detections: Vec<DetRecord>,
        per_class: Option<PerClassFrameStats>,
    },
```

### The `DetRecord` struct being serialized per detection

**File:** `/home/care/opt/workspace/references/mana-lite/src/logger/event.rs`, lines **83–87**

```rust
pub struct DetRecord {
    pub class: String,
    pub confidence: f32,
    pub bbox: [f32; 4],
}
```

### The `Detection` to `DetRecord` conversion

**File:** `/home/care/opt/workspace/references/mana-lite/src/infer.rs`, lines **29–37**

```rust
impl From<&Detection> for DetRecord {
    fn from(d: &Detection) -> Self {
        DetRecord {
            class: d.class.clone(),
            confidence: d.confidence,
            bbox: d.bbox,
        }
    }
}
```

### The constructor (`Event::detection`)

**File:** `/home/care/opt/workspace/references/mana-lite/src/logger/event.rs`, lines **141–143**

```rust
    pub fn detection(frame_id: u64, model: &str, infer_ms: u64, detections: Vec<DetRecord>, per_class: Option<PerClassFrameStats>) -> Self {
        Event::Detection { frame_id, model: model.into(), infer_ms, detections, per_class }
    }
```

### The serialization to JSONL bytes (the `"\"type\":\"detection\"` write)

**File:** `/home/care/opt/workspace/references/mana-lite/src/logger/serialize.rs`, lines **51–97**

```rust
        Event::Detection { frame_id, model, infer_ms, detections, per_class } => {
            buf.extend_from_slice(b"\"type\":\"detection\",\"frame_id\":");
            write_u64(*frame_id, buf);
            buf.extend_from_slice(b",\"model\":\"");
            write_json_string(model, buf);
            buf.extend_from_slice(b"\",\"infer_ms\":");
            write_u64(*infer_ms, buf);
            buf.extend_from_slice(b",\"det\":[");
            for (i, d) in detections.iter().enumerate() {
                if i > 0 { buf.push(b','); }
                buf.extend_from_slice(b"{\"class\":\"");
                write_json_string(&d.class, buf);
                buf.extend_from_slice(b"\",\"confidence\":");
                write_f32(d.confidence, buf);
                buf.extend_from_slice(b",\"bbox\":[");
                for (j, v) in d.bbox.iter().enumerate() {
                    if j > 0 { buf.push(b','); }
                    write_f32(*v, buf);
                }
                buf.extend_from_slice(b"]}");
            }
            buf.extend_from_slice(b"]");
            if let Some(pc) = per_class {
                if !pc.stats.is_empty() {
                    buf.extend_from_slice(b",\"per_class\":{");
                    let mut first = true;
                    for (cls, stat) in &pc.stats {
                        if !first { buf.push(b','); }
                        first = false;
                        buf.extend_from_slice(b"\"");
                        buf.extend_from_slice(cls.as_bytes());
                        buf.extend_from_slice(b"\":{\"count\":");
                        write_u64(stat.count, buf);
                        buf.extend_from_slice(b",\"conf_min\":");
                        write_f32(stat.conf_min, buf);
                        buf.extend_from_slice(b",\"conf_max\":");
                        write_f32(stat.conf_max, buf);
                        buf.extend_from_slice(b",\"area_min\":");
                        write_f64(stat.area_min, buf);
                        buf.extend_from_slice(b",\"area_max\":");
                        write_f64(stat.area_max, buf);
                        buf.extend_from_slice(b"}");
                    }
                    buf.extend_from_slice(b"}");
                }
            }
        }
```

This produces output like:

```json
{"type":"detection","frame_id":1,"model":"detect-fast","infer_ms":52,"det":[{"class":"person","confidence":0.87,"bbox":[100,200,300,500]}]}
```

---

## 3. Where `record_model_result` is called and what it does with detections — does it have access to `crop_rect`?

### The call site

**File:** `/home/care/opt/workspace/references/mana-lite/src/main.rs`, lines **270–271**, inside `run_inference`:

```rust
            if let Some((detections, infer_ms)) = self.infer.run(model_key, &fb.rgb, fb.w, fb.h, crop_rect) {
                self.record_model_result(model_key, infer_ms, &detections);
```

### The `record_model_result` function

**File:** `/home/care/opt/workspace/references/mana-lite/src/main.rs`, lines **313–324**

```rust
    fn record_model_result(&mut self, model_key: &str, infer_ms: u64, detections: &[Detection]) {
        let per_class = PerClassFrameStats::from_detections(detections);
        self.metrics.tick_inference_model(model_key, infer_ms, detections);
        self.viz.log_infer_latency(model_key, infer_ms);
        self.viz.log_detection_boxes(model_key, detections);
        self.viz.log_per_frame_class_stats(model_key, &per_class);
        self.log.emit(Event::detection(
            self.state.frame_number(), model_key, infer_ms,
            detections.iter().map(DetRecord::from).collect(),
            Some(per_class),
        ));
    }
```

### The full `run_inference` method for context

**File:** `/home/care/opt/workspace/references/mana-lite/src/main.rs`, lines **254–278**

```rust
    fn run_inference(&mut self, fb: &FrameBuffer, config: &AppConfig) {
        let requested = self.resolve_models(config);
        let ordered = self.cascade.ordered(&requested);
        let mut model_dets: HashMap<String, Vec<Detection>> = HashMap::new();

        for model_key in &ordered {
            let crop_rect = self.resolve_crop_rect(model_key, &model_dets, fb);

            let always_run = self.infer.crop_info(model_key)
                .map_or(false, |c| c.always_run());

            if crop_rect.is_none() && !always_run && !self.cascade.should_run(model_key, &model_dets) {
                self.metrics.tick_infer_skip(model_key);
                continue;
            }

            if let Some((detections, infer_ms)) = self.infer.run(model_key, &fb.rgb, fb.w, fb.h, crop_rect) {
                self.record_model_result(model_key, infer_ms, &detections);
                if config.pipeline.track {
                    self.run_tracking(&detections);
                }
                model_dets.insert(model_key.clone(), detections);
            }
        }
    }
```

**Does `record_model_result` have access to `crop_rect`?**

**No.** The function signature is:

```rust
fn record_model_result(&mut self, model_key: &str, infer_ms: u64, detections: &[Detection])
```

It receives `model_key`, `infer_ms`, and `detections` — but **not** `crop_rect`. The `crop_rect` variable is defined at line 260 (as a local in the loop body) and is passed to `self.infer.run(...)` at line 270, but it is **not forwarded** to `record_model_result`. The detection bounding boxes that come back from `self.infer.run(...)` are already mapped back to full-frame coordinates by the inference engine, so the serialized `DetRecord` bboxes are in frame-space, not crop-space. The `crop_rect` itself is not logged or attached to the event.

use super::app::AppConfig;

macro_rules! env_str {
    ($var:literal => $field:expr) => {
        if let Ok(v) = std::env::var($var) {
            $field = v;
        }
    };
}

macro_rules! env_path {
    ($var:literal => $field:expr) => {
        if let Ok(v) = std::env::var($var) {
            $field = v.into();
        }
    };
}

macro_rules! env_bool {
    ($var:literal => $field:expr) => {
        if let Ok(v) = std::env::var($var) {
            $field = v == "1" || v == "true";
        }
    };
}

macro_rules! env_parse {
    ($var:literal => $field:expr) => {
        if let Ok(v) = std::env::var($var) {
            if let Ok(n) = v.parse() {
                $field = n;
            }
        }
    };
}

macro_rules! env_opt {
    ($var:literal => $field:expr) => {
        if let Ok(v) = std::env::var($var) {
            $field = if v.is_empty() { None } else { Some(v.into()) };
        }
    };
}

pub(crate) fn apply_env_overrides(cfg: &mut AppConfig) {
    env_str!("MANA_SOURCE_URL" => cfg.source.url);
    env_opt!("MANA_SOURCE_USERNAME" => cfg.source.username);
    env_opt!("MANA_SOURCE_PASSWORD" => cfg.source.password);
    env_str!("MANA_TRANSPORT" => cfg.source.transport);
    env_bool!("MANA_KEYFRAMES_ONLY" => cfg.source.keyframes_only);
    env_path!("MANA_MODEL_CATALOG" => cfg.inference.model_catalog);
    env_opt!("MANA_BLUEPRINT_FILE" => cfg.inference.blueprint_file);
    env_opt!("MANA_DEFAULT_MODEL" => cfg.inference.default_model);
    env_opt!("MANA_METRICS_FILE" => cfg.metrics_file);
    env_parse!("MANA_DATA_STALE_MS" => cfg.health.data_stale_ms);
    env_parse!("MANA_REPORT_INTERVAL" => cfg.health.report_interval_s);
    env_opt!("MANA_SAVE_DIR" => cfg.output.save_dir);
    env_bool!("MANA_VIZ_ENABLED" => cfg.viz.enabled);
    env_str!("MANA_RERUN_ADDR" => cfg.viz.rerun_addr);
    env_opt!("MANA_SNAPSHOT_DIR" => cfg.output.snapshot_dir);
    env_bool!("MANA_SNAPSHOT_VERBOSE" => cfg.output.snapshot_verbose);
    env_str!("MANA_JSONL_LEVEL" => cfg.output.jsonl_level);
    env_parse!("MANA_POLL_TIMEOUT_MS" => cfg.ingest.poll_timeout_ms);
    env_parse!("MANA_ERROR_WINDOW_SIZE" => cfg.ingest.error_window_size);
    env_parse!("MANA_ERROR_WINDOW_THRESHOLD" => cfg.ingest.error_window_threshold);
    env_parse!("MANA_BACKOFF_INITIAL_MS" => cfg.ingest.reconnect_backoff_initial_ms);
    env_parse!("MANA_BACKOFF_MAX_MS" => cfg.ingest.reconnect_backoff_max_ms);
}

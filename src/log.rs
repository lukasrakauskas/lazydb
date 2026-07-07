use std::fs::File;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

// ponytail: one global file behind a Mutex — serializes all log writes. Fine
// for a debug log at human keypress rates; upgrade to a dedicated logger thread
// (or a real tracing crate) if throughput or structured machine parsing matters.
static LOGGER: OnceLock<Mutex<File>> = OnceLock::new();

/// Open `path` in append mode and install it as the global log target.
/// Calling more than once is a no-op (the first file wins).
pub fn init(path: &str) -> anyhow::Result<()> {
    if LOGGER.get().is_some() {
        return Ok(());
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    // Ignore a lost OnceLock race (single-threaded init in practice).
    let _ = LOGGER.set(Mutex::new(file));
    Ok(())
}

/// Pure: build one tab-separated structured line.
/// Format: `<ts_ms>\t<LEVEL>\t<msg>\t<k>=<v>;<k>=<v>`
/// ponytail: only `\t`,`\n`,`\r` are replaced with a space — no full CSV/JSON
/// escaping. Fine for human+grep debug reading; upgrade to JSON if a machine
/// parser ever needs to consume these lines.
fn format_line(ts_ms: u128, level: &str, msg: &str, fields: &[(&str, &str)]) -> String {
    let esc = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let mut line = format!("{}\t{}\t{}", ts_ms, level, esc(msg));
    if !fields.is_empty() {
        line.push('\t');
        for (i, (k, v)) in fields.iter().copied().enumerate() {
            if i > 0 {
                line.push(';');
            }
            line.push_str(k);
            line.push('=');
            line.push_str(&esc(v));
        }
    }
    line
}

/// Emit a structured event. No-op when `init` was never called, so calls are
/// cheap and safe when logging is off.
// ponytail: unix millis from SystemTime, no calendar math — the debug log only
// needs relative ordering, not wall-clock dates. Upgrade to chrono/time for
// human-readable timestamps if needed.
pub fn event(level: &str, msg: &str, fields: &[(&str, &str)]) {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let ts_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let line = format_line(ts_ms, level, msg, fields);
    // ponytail: swallow IO errors — logging must never crash the TUI.
    if let Ok(mut f) = logger.lock() {
        let _ = writeln!(f, "{line}");
        let _ = f.flush();
    }
}

pub fn info(msg: &str, fields: &[(&str, &str)]) {
    event("INFO", msg, fields);
}
// ponytail: warn is part of the level API but no call site needs it yet.
#[allow(dead_code)]
pub fn warn(msg: &str, fields: &[(&str, &str)]) {
    event("WARN", msg, fields);
}
pub fn error(msg: &str, fields: &[(&str, &str)]) {
    event("ERROR", msg, fields);
}

#[cfg(test)]
mod tests {
    use super::format_line;

    #[test]
    fn formats_tab_separated_line() {
        let line = format_line(
            1700000000123,
            "INFO",
            "query",
            &[("ms", "42"), ("rows", "7")],
        );
        assert_eq!(line, "1700000000123\tINFO\tquery\tms=42;rows=7");
    }

    #[test]
    fn empty_fields_omits_field_section() {
        let line = format_line(5, "INFO", "start", &[]);
        assert_eq!(line, "5\tINFO\tstart");
    }

    #[test]
    fn escapes_tab_and_newline_in_values() {
        // A message/value containing tab/newline must have them replaced with a
        // space so each log line stays one-line and grep-parseable.
        let line = format_line(1, "ERROR", "a\tb\nc", &[("k", "v1\tv2\nv3")]);
        // Only the three structural separators (ts/LEVEL/msg/fields) remain.
        assert_eq!(line.matches('\t').count(), 3);
        assert_eq!(line, "1\tERROR\ta b c\tk=v1 v2 v3");
    }
}

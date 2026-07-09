mod app;
mod autocomplete;
mod config;
mod db;
mod editor;
mod filter;
mod highlight;
mod log;
mod shortcuts;
mod theme;
mod ui;

use std::io;

use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

/// Restores the terminal even if the app panics.
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            PopKeyboardEnhancementFlags
        );
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lazydb [--log-file PATH] [SCRIPT]");
        println!("  --log-file PATH   Log debug output to file");
        println!("  --help, -h        Show this help");
        println!("  --version, -V     Show version");
        println!("  SCRIPT            SQL file to preload in editor");
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("lazydb {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    // ponytail: hand-rolled --log-file scan, no clap.
    // Strips the flag (and its value) so the existing
    // `lazydb script.sql` positional logic below still works unchanged.
    let (log_file, args) = parse_log_file(args);
    if let Some(path) = log_file {
        crate::log::init(&path).unwrap_or_else(|e| {
            eprintln!("lazydb: cannot open log file '{path}': {e}");
            std::process::exit(1);
        });
        let script = if args.len() >= 2 { &args[1] } else { "" };
        crate::log::info("start", &[("script", script)]);
    }
    let mut app = app::App::load()?;
    // ponytail: optional `lazydb path/to/script.sql` preloads the editor.
    if args.len() >= 2 {
        let path = &args[1];
        match std::fs::read_to_string(path) {
            Ok(text) => app.load_script(text),
            Err(err) => {
                eprintln!("lazydb: cannot read script '{}': {}", path, err);
                std::process::exit(1);
            }
        }
    }

    enable_raw_mode()?;
    let _guard = Guard;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    // ponytail: Kitty keyboard protocol so Shift+Enter (and other modified keys)
    // report their modifiers. No-op on terminals that don't support it; errors on
    // the legacy Windows console (ignored) where modifiers are reported natively.
    let _ = execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    app::run(&mut terminal, app)
}

/// Extract `--log-file <path>` / `--log-file=<path>` from argv, returning the
/// requested path (if any) and the remaining args with the flag stripped.
fn parse_log_file(args: Vec<String>) -> (Option<String>, Vec<String>) {
    let mut log_file: Option<String> = None;
    let mut out: Vec<String> = Vec::with_capacity(args.len());
    let mut iter = args.into_iter();
    if let Some(prog) = iter.next() {
        out.push(prog);
    }
    while let Some(arg) = iter.next() {
        if arg == "--log-file" {
            if let Some(val) = iter.next() {
                log_file = Some(val);
            }
        } else if let Some(val) = arg.strip_prefix("--log-file=") {
            log_file = Some(val.to_string());
        } else {
            out.push(arg);
        }
    }
    (log_file, out)
}

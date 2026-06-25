mod app;
mod config;
mod db;
mod editor;
mod ui;

use std::io;

use anyhow::Result;
use crossterm::{
    execute,
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};


/// Restores the terminal even if the app panics.
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture, PopKeyboardEnhancementFlags);
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut app = app::App::load()?;
    // ponytail: optional `lazydb path/to/script.sql` preloads the editor.
    if args.len() > 1 {
        if let Ok(text) = std::fs::read_to_string(&args[1]) {
            app.load_script(text);
        }
    }

    enable_raw_mode()?;
    let _guard = Guard;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    // ponytail: Kitty keyboard protocol so Shift+Enter (and other modified keys)
    // report their modifiers. No-op on terminals that don't support it; errors on
    // the legacy Windows console (ignored) where modifiers are reported natively.
    let _ = execute!(stdout, PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    app::run(&mut terminal, app)
}

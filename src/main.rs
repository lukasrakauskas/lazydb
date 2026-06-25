mod app;
mod config;
mod db;
mod editor;
mod ui;

use std::io;

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};


/// Restores the terminal even if the app panics.
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
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
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    app::run(&mut terminal, app)
}

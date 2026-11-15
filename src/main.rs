//! Binary entry point. Initializes the terminal, sets a panic hook that
//! restores it on crash, and runs the main event loop at ~60 FPS.

use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::Parser;
use color_eyre::eyre::Result;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement};

use rs_typing_trainer::app::App;
use rs_typing_trainer::cli::{Cli, Command};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::i18n::Msg;
use rs_typing_trainer::layout::load_user_layouts;
use rs_typing_trainer::record::{db, storage, ttrec};
use rs_typing_trainer::ui;

const TICK: Duration = Duration::from_millis(16);
const RELEASE_WAIT_LIMIT: Duration = Duration::from_millis(300);
const CLEANUP_DRAIN_LIMIT: Duration = Duration::from_millis(75);

fn main() -> ExitCode {
    let cli = Cli::parse();
    let lang = cli.lang.unwrap_or_default();
    if let Err(error) = rs_typing_trainer::analysis::grades::bundled_thresholds() {
        eprintln!("{}: invalid bundled grade thresholds: {error}", Msg::AppTitle.text(lang));
        return ExitCode::from(1);
    }
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));

    match cli.command.as_ref() {
        Some(Command::Replay {
            file,
            interval,
            speed,
            paused,
            exit_on_end,
            print,
        }) => match run_replay(file, *interval, *speed, *paused, *exit_on_end, *print, lang) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{}: {error}", Msg::AppTitle.text(lang));
                ExitCode::from(1)
            }
        },
        Some(Command::Stats {
            csv,
            summary,
            grades,
            coach,
            since,
            mode,
        }) => match run_stats(&cli, &home, *csv, *summary, *grades, *coach, since.as_deref(), mode.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{}: {error}", Msg::AppTitle.text(lang));
                ExitCode::from(1)
            }
        },
        Some(Command::Export { session_id, output }) => match run_export(&cli, &home, *session_id, output.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{}: {error}", Msg::AppTitle.text(lang));
                ExitCode::from(1)
            }
        },
        None | Some(Command::Play) => match run_game(&cli, &home) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{}: {error}", Msg::AppTitle.text(lang));
                ExitCode::from(1)
            }
        },
    }
}

fn run_replay(
    file: &std::path::Path,
    interval_ms: Option<u64>,
    speed: f64,
    paused: bool,
    exit_on_end: bool,
    print_summary: bool,
    lang: rs_typing_trainer::i18n::Lang,
) -> Result<()> {
    let input = std::fs::File::open(file)?;
    let record = ttrec::read(std::io::BufReader::new(input))?;
    if print_summary {
        print!("{}", rs_typing_trainer::replay::format_summary(&record));
        return Ok(());
    }
    let timing = interval_ms.map_or(rs_typing_trainer::replay::PlaybackTiming::Recorded { speed }, |milliseconds| {
        rs_typing_trainer::replay::PlaybackTiming::Fixed {
            interval: Duration::from_millis(milliseconds),
        }
    });
    let replay = rs_typing_trainer::replay::Replay::new_with_lang(record, timing, paused, exit_on_end, lang);
    run_replay_tui(replay, lang)
}

fn run_replay_tui(mut replay: rs_typing_trainer::replay::Replay, lang: rs_typing_trainer::i18n::Lang) -> Result<()> {
    let (mut terminal, mut terminal_guard, release_events) = init_terminal()?;
    let started = Instant::now();
    let result = (|| -> Result<()> {
        loop {
            terminal.draw(|frame| ui::replay::render(frame, &replay, lang))?;
            if replay.should_exit() {
                break;
            }
            if event::poll(TICK)?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                let elapsed = started.elapsed();
                let should_exit = match key.code {
                    KeyCode::Char(' ') => {
                        replay.toggle_pause(elapsed);
                        false
                    }
                    KeyCode::Right => {
                        replay.step();
                        false
                    }
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => true,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
                    _ => false,
                };
                if should_exit {
                    if release_events {
                        wait_for_key_release(&key, RELEASE_WAIT_LIMIT)?;
                    }
                    break;
                }
            }
            replay.advance_to(started.elapsed());
        }
        Ok(())
    })();
    terminal_guard.restore()?;
    result
}

#[allow(clippy::too_many_arguments)]
fn run_stats(
    cli: &Cli,
    home: &std::path::Path,
    csv: Option<rs_typing_trainer::cli::StatsCsv>,
    summary: bool,
    grades: bool,
    coach: bool,
    since: Option<&str>,
    mode: Option<&str>,
) -> Result<()> {
    let config = Config::load_from(home, cli.config.as_deref())?;
    prepare_default_storage(&config, home)?;
    let since = since
        .map(|value| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d"))
        .transpose()
        .map_err(|_| color_eyre::eyre::eyre!("--since must use YYYY-MM-DD"))?;
    let output = rs_typing_trainer::analysis::stats::render(
        &config.resolve_db_path(home),
        home,
        &config,
        csv,
        summary,
        grades,
        coach,
        since,
        mode,
        cli.effective_lang(config.general.lang),
        chrono::Local::now().date_naive(),
    )?;
    print!("{output}");
    Ok(())
}

fn run_export(cli: &Cli, home: &std::path::Path, session_id: i64, output: Option<&std::path::Path>) -> Result<()> {
    let config = Config::load_from(home, cli.config.as_deref())?;
    prepare_default_storage(&config, home)?;
    let path = config.resolve_db_path(home);
    let record = db::load_record(&path, session_id)?.ok_or_else(|| color_eyre::eyre::eyre!("session {session_id} not found"))?;
    if record.header.metrics_version == 0 {
        return Err(color_eyre::eyre::eyre!(
            "session {session_id} is a v1 record and has no keystroke log to export"
        ));
    }
    if let Some(path) = output {
        let file = std::fs::File::create(path)?;
        ttrec::write(&record, file)?;
    } else {
        let stdout = io::stdout();
        ttrec::write(&record, stdout.lock())?;
    }
    Ok(())
}

fn prepare_default_storage(config: &Config, home: &std::path::Path) -> Result<bool> {
    let plan = storage::storage_plan(home, config);
    Ok(storage::prepare_storage(&plan)?)
}

fn run_game(cli: &Cli, home: &std::path::Path) -> Result<()> {
    color_eyre::install()?;

    let mut config = Config::load_from(home, cli.config.as_deref())?;
    config.general.lang = cli.effective_lang(config.general.lang);
    let copied_legacy_db = prepare_default_storage(&config, home)?;
    let config_path = rs_typing_trainer::config::config_path_for_write(home, cli.config.as_deref());
    let user_layouts = config_path.parent().map(load_user_layouts).transpose()?.unwrap_or_default();

    let mut app = App::new(config, home.to_path_buf())
        .with_storage_notice(copied_legacy_db)
        .with_config_path(config_path);
    app.add_layouts(user_layouts);
    if !app.layouts.contains_key(&app.config.keyboard.layout) {
        return Err(color_eyre::eyre::eyre!("unknown keyboard layout: {}", app.config.keyboard.layout));
    }
    if app.config.keyboard.input_layout != "same" && !app.layouts.contains_key(&app.config.keyboard.input_layout) {
        return Err(color_eyre::eyre::eyre!("unknown input layout: {}", app.config.keyboard.input_layout));
    }

    let (mut terminal, mut terminal_guard, release_events) = init_terminal()?;
    app.release_events_supported = release_events;
    let result = run(&mut terminal, app);
    terminal_guard.restore()?;
    result.map_err(|error| color_eyre::eyre::eyre!("{error}"))
}

fn run<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>, mut app: App) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    let mut last_tick = Instant::now();
    while !app.should_quit {
        terminal.draw(|f| ui::render(f, &app))?;

        let timeout = TICK.checked_sub(last_tick.elapsed()).unwrap_or_else(|| Duration::from_millis(0));
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
        {
            let was_quitting = app.should_quit;
            app.handle_key_at(key, Instant::now());
            if !was_quitting && app.should_quit && app.release_events_supported && key.kind == KeyEventKind::Press {
                wait_for_key_release(&key, RELEASE_WAIT_LIMIT)?;
            }
        }
        let now = Instant::now();
        let dt = now.saturating_duration_since(last_tick);
        if dt >= TICK {
            app.on_tick_at(dt, now)?;
            last_tick = now;
        }
    }
    Ok(())
}

fn init_terminal() -> Result<(Terminal<CrosstermBackend<io::Stdout>>, TerminalGuard<CrosstermCleanup>, bool)> {
    enable_raw_mode()?;
    let mut guard = TerminalGuard::new(CrosstermCleanup, true);
    let mut stdout = io::stdout();
    let release_events = supports_keyboard_enhancement().unwrap_or(false);
    // The kitty protocol keeps separate flag stacks for the main and the
    // alternate screen. Push only after entering the alternate screen so the
    // pop in `TerminalGuard::restore` (also made on the alternate screen)
    // removes the same entry; pushing on the main screen leaves the flags
    // active in the user's shell after the app exits.
    execute!(stdout, EnterAlternateScreen)?;
    guard.screen_active = true;
    if release_events {
        guard.enhancement_active = true;
        execute!(stdout, PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES))?;
    }
    execute!(stdout, EnableMouseCapture, Hide)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok((terminal, guard, release_events))
}

fn wait_for_key_release(pressed: &ratatui::crossterm::event::KeyEvent, limit: Duration) -> io::Result<()> {
    let deadline = Instant::now() + limit;
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Ok(());
        };
        if !event::poll(remaining)? {
            return Ok(());
        }
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Release
            && key.code == pressed.code
        {
            return Ok(());
        }
    }
}

trait CleanupActions {
    fn pop_keyboard_enhancement(&mut self) -> io::Result<()>;
    fn drain_input(&mut self) -> io::Result<()>;
    fn leave_screen_and_show_cursor(&mut self) -> io::Result<()>;
    fn disable_raw_mode(&mut self) -> io::Result<()>;
}

struct CrosstermCleanup;

impl CleanupActions for CrosstermCleanup {
    fn pop_keyboard_enhancement(&mut self) -> io::Result<()> {
        // Kitty's CSI `<u` pops the current keyboard-enhancement flags.
        let mut stdout = io::stdout();
        stdout.write_all(b"\x1b[<u")?;
        stdout.flush()
    }

    fn drain_input(&mut self) -> io::Result<()> {
        let deadline = Instant::now() + CLEANUP_DRAIN_LIMIT;
        loop {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Ok(());
            };
            if !event::poll(remaining)? {
                return Ok(());
            }
            let _ = event::read()?;
        }
    }

    fn leave_screen_and_show_cursor(&mut self) -> io::Result<()> {
        execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture, Show)
    }

    fn disable_raw_mode(&mut self) -> io::Result<()> {
        disable_raw_mode()
    }
}

struct TerminalGuard<C: CleanupActions> {
    cleanup: C,
    raw_active: bool,
    enhancement_active: bool,
    screen_active: bool,
    restored: bool,
}

impl<C: CleanupActions> TerminalGuard<C> {
    fn new(cleanup: C, raw_active: bool) -> Self {
        Self {
            cleanup,
            raw_active,
            enhancement_active: false,
            screen_active: false,
            restored: false,
        }
    }

    #[cfg(test)]
    fn for_test(cleanup: C, raw_active: bool, enhancement_active: bool, screen_active: bool) -> Self {
        Self {
            cleanup,
            raw_active,
            enhancement_active,
            screen_active,
            restored: false,
        }
    }

    fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;
        let mut first_error = None;
        if self.enhancement_active {
            remember_cleanup_error(&mut first_error, self.cleanup.pop_keyboard_enhancement());
            remember_cleanup_error(&mut first_error, self.cleanup.drain_input());
        }
        if self.screen_active {
            remember_cleanup_error(&mut first_error, self.cleanup.leave_screen_and_show_cursor());
            if self.enhancement_active {
                // Pop once more on the main screen. With an empty stack this is
                // a no-op, and it clears flags left behind by older builds that
                // pushed on the main screen.
                remember_cleanup_error(&mut first_error, self.cleanup.pop_keyboard_enhancement());
            }
        }
        if self.raw_active {
            remember_cleanup_error(&mut first_error, self.cleanup.disable_raw_mode());
        }
        first_error.map_or(Ok(()), Err)
    }
}

impl<C: CleanupActions> Drop for TerminalGuard<C> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn remember_cleanup_error(first_error: &mut Option<io::Error>, result: io::Result<()>) {
    if first_error.is_none()
        && let Err(error) = result
    {
        *first_error = Some(error);
    }
}

#[cfg(test)]
mod terminal_cleanup_tests {
    use std::cell::RefCell;
    use std::io;
    use std::rc::Rc;

    use ratatui::crossterm::cursor::Show;
    use ratatui::crossterm::event::DisableMouseCapture;
    use ratatui::crossterm::execute;
    use ratatui::crossterm::terminal::LeaveAlternateScreen;

    use super::{CleanupActions, TerminalGuard};

    #[derive(Clone)]
    struct RecordingCleanup {
        output: Rc<RefCell<Vec<u8>>>,
        fail_pop: bool,
    }

    impl CleanupActions for RecordingCleanup {
        fn pop_keyboard_enhancement(&mut self) -> io::Result<()> {
            self.output.borrow_mut().extend_from_slice(b"\x1b[<u");
            if self.fail_pop { Err(io::Error::other("pop failed")) } else { Ok(()) }
        }

        fn drain_input(&mut self) -> io::Result<()> {
            self.output.borrow_mut().extend_from_slice(b"<drain>");
            Ok(())
        }

        fn leave_screen_and_show_cursor(&mut self) -> io::Result<()> {
            let mut output = self.output.borrow_mut();
            execute!(&mut *output, LeaveAlternateScreen, DisableMouseCapture, Show)
        }

        fn disable_raw_mode(&mut self) -> io::Result<()> {
            self.output.borrow_mut().extend_from_slice(b"<raw-off>");
            Ok(())
        }
    }

    #[test]
    fn cleanup_pops_then_drains_then_leaves_screen_then_disables_raw_mode() {
        let output = Rc::new(RefCell::new(Vec::new()));
        let cleanup = RecordingCleanup {
            output: output.clone(),
            fail_pop: false,
        };
        let mut guard = TerminalGuard::for_test(cleanup, true, true, true);

        guard.restore().unwrap();

        let output = output.borrow();
        let pop = find_bytes(&output, b"\x1b[<u");
        let drain = find_bytes(&output, b"<drain>");
        let leave = find_bytes(&output, b"\x1b[?1049l");
        let raw_off = find_bytes(&output, b"<raw-off>");
        assert!(pop < drain && drain < leave && leave < raw_off, "{output:?}");
    }

    #[test]
    fn cleanup_pops_again_on_the_main_screen_before_raw_mode_is_disabled() {
        // Kitty keeps separate flag stacks per screen; a pop issued only on the
        // alternate screen leaves main-screen flags active in the shell.
        let output = Rc::new(RefCell::new(Vec::new()));
        let cleanup = RecordingCleanup {
            output: output.clone(),
            fail_pop: false,
        };
        let mut guard = TerminalGuard::for_test(cleanup, true, true, true);

        guard.restore().unwrap();

        let output = output.borrow();
        let leave = find_bytes(&output, b"\x1b[?1049l");
        let raw_off = find_bytes(&output, b"<raw-off>");
        let second_pop = output[leave..]
            .windows(4)
            .position(|window| window == b"\x1b[<u")
            .map(|offset| leave + offset)
            .expect("a pop after leaving the alternate screen");
        assert!(second_pop < raw_off, "{output:?}");
    }

    #[test]
    fn cleanup_guard_runs_during_panic_unwind() {
        let output = Rc::new(RefCell::new(Vec::new()));
        let cleanup = RecordingCleanup {
            output: output.clone(),
            fail_pop: false,
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = TerminalGuard::for_test(cleanup, true, true, true);
            panic!("test panic");
        }));

        assert!(result.is_err());
        let output = output.borrow();
        assert!(find_bytes(&output, b"\x1b[<u") < find_bytes(&output, b"<raw-off>"), "{output:?}");
    }

    #[test]
    fn cleanup_continues_after_an_error_and_is_idempotent() {
        let output = Rc::new(RefCell::new(Vec::new()));
        let cleanup = RecordingCleanup {
            output: output.clone(),
            fail_pop: true,
        };
        let mut guard = TerminalGuard::for_test(cleanup, true, true, true);

        assert!(guard.restore().is_err());
        let length_after_first_restore = output.borrow().len();
        guard.restore().unwrap();

        let output = output.borrow();
        assert_eq!(output.len(), length_after_first_restore);
        assert!(find_bytes(&output, b"<drain>") < find_bytes(&output, b"<raw-off>"), "{output:?}");
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
            .expect("escape sequence missing")
    }
}

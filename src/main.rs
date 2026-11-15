//! Binary entry point. Initializes the terminal, sets a panic hook that
//! restores it on crash, and runs the main event loop at ~60 FPS.

use std::io;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use clap::Parser;
use color_eyre::eyre::Result;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
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
static KEYBOARD_ENHANCEMENT_ACTIVE: AtomicBool = AtomicBool::new(false);

fn main() -> ExitCode {
    let cli = Cli::parse();
    let lang = cli.lang.unwrap_or_default();
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
        Some(Command::Stats { csv, since, mode }) => match run_stats(&cli, &home, *csv, since.as_deref(), mode.as_deref()) {
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
    install_panic_hook();
    let (mut terminal, _) = init_terminal()?;
    let started = Instant::now();
    let result = (|| -> Result<()> {
        loop {
            terminal.draw(|frame| ui::replay::render(frame, &replay, lang))?;
            if replay.should_exit() {
                break;
            }
            if event::poll(TICK)?
                && let Event::Key(key) = event::read()?
                && key.kind != KeyEventKind::Release
            {
                let elapsed = started.elapsed();
                match key.code {
                    KeyCode::Char(' ') => replay.toggle_pause(elapsed),
                    KeyCode::Right => replay.step(),
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    _ => {}
                }
            }
            replay.advance_to(started.elapsed());
        }
        Ok(())
    })();
    restore_terminal()?;
    result
}

fn run_stats(
    cli: &Cli,
    home: &std::path::Path,
    csv: Option<rs_typing_trainer::cli::StatsCsv>,
    since: Option<&str>,
    mode: Option<&str>,
) -> Result<()> {
    let config = Config::load_from(home, cli.config.as_deref())?;
    prepare_default_storage(&config, home)?;
    let since = since
        .map(|value| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d"))
        .transpose()
        .map_err(|_| color_eyre::eyre::eyre!("--since must use YYYY-MM-DD"))?;
    let output = rs_typing_trainer::analysis::stats::render(&config.resolve_db_path(home), &config, csv, since, mode)?;
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

    install_panic_hook();
    let (mut terminal, release_events) = init_terminal()?;
    app.release_events_supported = release_events;
    let result = run(&mut terminal, app);
    restore_terminal()?;
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
            app.handle_key_at(key, Instant::now());
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

fn init_terminal() -> Result<(Terminal<CrosstermBackend<io::Stdout>>, bool)> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    let release_events = supports_keyboard_enhancement().unwrap_or(false);
    if release_events {
        if let Err(error) = execute!(stdout, PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)) {
            let _ = disable_raw_mode();
            return Err(error.into());
        }
        KEYBOARD_ENHANCEMENT_ACTIVE.store(true, Ordering::SeqCst);
    }
    if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
        let _ = restore_terminal();
        return Err(error.into());
    }
    let backend = CrosstermBackend::new(stdout);
    match Terminal::new(backend) {
        Ok(terminal) => Ok((terminal, release_events)),
        Err(error) => {
            let _ = restore_terminal();
            Err(error.into())
        }
    }
}

fn restore_terminal() -> Result<()> {
    let mut stdout = io::stdout();
    let pop_result = if KEYBOARD_ENHANCEMENT_ACTIVE.swap(false, Ordering::SeqCst) {
        execute!(stdout, PopKeyboardEnhancementFlags)
    } else {
        Ok(())
    };
    let raw_result = disable_raw_mode();
    let screen_result = execute!(stdout, LeaveAlternateScreen, DisableMouseCapture);
    pop_result?;
    raw_result?;
    screen_result?;
    Ok(())
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original(info);
    }));
}

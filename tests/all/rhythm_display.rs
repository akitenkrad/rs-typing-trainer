use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::rhythm::{display_divisor, pendulum_position};
use rs_typing_trainer::ui;

use crate::common::{accept_briefing, test_app_at};

fn date() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 11, 20).unwrap()
}

fn start_rhythm(lang: Lang, bpm: f64, elapsed: Duration) -> crate::common::TestApp {
    let mut config = Config::default();
    config.general.lang = lang;
    config.mode.kind = ModeKind::Rhythm;
    config.rhythm.bpm = Some(bpm);
    config.keyboard.show = "hidden".to_owned();
    config.display.prompt_style = "normal".to_owned();
    config.ui.animation_enabled = false;

    let mut app = test_app_at(config, date());
    let started = Instant::now();
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key_at(enter, started);
    app.handle_key_at(enter, started);
    accept_briefing(&mut app, started);
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    app.on_tick_at_date(Duration::from_secs(3), started, date()).unwrap();
    app.on_tick_at_date(Duration::from_millis(1), started + elapsed, date()).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    app
}

fn render(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal
}

fn render_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect()
}

fn rhythm_row_at(app: &rs_typing_trainer::app::App, width: u16) -> String {
    render_rows(&render(app, width, 24))
        .into_iter()
        .find(|row| row.contains('拍') || row.contains("Beat"))
        .expect("rhythm row")
}

fn rhythm_row(app: &rs_typing_trainer::app::App) -> String {
    rhythm_row_at(app, 100)
}

fn ball_column(app: &rs_typing_trainer::app::App) -> u16 {
    let terminal = render(app, 100, 24);
    let buffer = terminal.backend().buffer();
    buffer
        .content()
        .iter()
        .enumerate()
        .find(|(_, cell)| cell.symbol() == "●")
        .map(|(index, _)| index as u16 % buffer.area.width)
        .expect("pendulum ball")
}

#[test]
fn display_divisor_uses_the_smallest_power_of_two_at_or_below_120_bpm() {
    for (bpm, expected) in [(90.0, 1), (120.0, 1), (121.0, 2), (200.0, 2), (360.0, 4), (500.0, 8), (0.0, 1)] {
        assert_eq!(display_divisor(bpm), expected, "{bpm} BPM");
    }
    assert_eq!(display_divisor(-1.0), 1);
    assert_eq!(display_divisor(f64::NAN), 1);
    assert_eq!(display_divisor(f64::INFINITY), 1);
    assert_eq!(display_divisor(2_000.0), 16);
}

#[test]
fn pendulum_position_reaches_each_end_on_a_display_beat() {
    let bpm = 360.0;
    let divisor = 4;
    let cases = [
        (Duration::ZERO, 0.0),
        (Duration::from_nanos(333_333_333), 0.5),
        (Duration::from_nanos(666_666_667), 1.0),
        (Duration::from_nanos(1_333_333_333), 0.0),
    ];
    for (elapsed, expected) in cases {
        let actual = pendulum_position(bpm, divisor, elapsed);
        assert!((actual - expected).abs() < 1e-6, "elapsed={elapsed:?}: {actual} != {expected}");
    }
}

#[test]
fn pendulum_position_stays_at_the_left_for_invalid_timing() {
    assert_eq!(pendulum_position(0.0, 4, Duration::from_secs(1)), 0.0);
    assert_eq!(pendulum_position(f64::NAN, 4, Duration::from_secs(1)), 0.0);
    assert_eq!(pendulum_position(360.0, 0, Duration::from_secs(1)), 0.0);
}

#[test]
fn rhythm_playing_screen_draws_a_scaled_pendulum_track() {
    let app = start_rhythm(Lang::Ja, 360.0, Duration::ZERO);
    let row = rhythm_row(&app);

    assert!(row.contains('┃'), "{row:?}");
    assert!(row.contains('●'), "{row:?}");
    assert!(row.contains('╎'), "{row:?}");
    assert!(row.contains("1/4"), "{row:?}");
    assert!(!row.contains('○'), "{row:?}");
}

#[test]
fn rhythm_pendulum_ball_moves_as_elapsed_time_advances() {
    let left = start_rhythm(Lang::Ja, 360.0, Duration::ZERO);
    let middle = start_rhythm(Lang::Ja, 360.0, Duration::from_nanos(333_333_333));

    assert_ne!(ball_column(&left), ball_column(&middle));
}

#[test]
fn unscaled_rhythm_explanation_only_shows_the_actual_bpm() {
    let app = start_rhythm(Lang::Ja, 90.0, Duration::ZERO);
    let row = rhythm_row(&app);

    assert!(row.contains("90 BPM"), "{row:?}");
    assert!(!row.contains("1/"), "{row:?}");
    assert!(!row.contains('╎'), "{row:?}");
}

#[test]
fn rhythm_explanation_shortens_and_then_disappears_on_narrow_screens() {
    let app = start_rhythm(Lang::Ja, 360.0, Duration::ZERO);
    let short = rhythm_row_at(&app, 30);
    let omitted = rhythm_row_at(&app, 20);

    assert!(short.contains("90/360 BPM"), "{short:?}");
    assert!(!short.contains("表示"), "{short:?}");
    assert!(!omitted.contains("BPM"), "{omitted:?}");
    assert!(omitted.contains('┃') && omitted.contains('●'), "{omitted:?}");
}

#[test]
fn rhythm_track_internal_width_is_capped_at_41_cells() {
    let app = start_rhythm(Lang::En, 360.0, Duration::ZERO);
    let terminal = render(&app, 160, 24);
    let buffer = terminal.backend().buffer();
    let boundaries = buffer
        .content()
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.symbol() == "┃")
        .map(|(index, _)| index as u16 % buffer.area.width)
        .collect::<Vec<_>>();

    assert_eq!(boundaries.len(), 2);
    assert_eq!(boundaries[1] - boundaries[0] - 1, 41);
}

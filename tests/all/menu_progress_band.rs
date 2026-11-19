//! MYTASK-3627: the countdown and result screens of today's training show the
//! whole menu and where the player is.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::ui;
use unicode_width::UnicodeWidthStr;

use crate::common::{TestApp, test_app_at};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Rows of the rendered screen; a wide character's trailing cell is skipped
/// so the rows read as the text on screen.
fn render(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            let mut row = String::new();
            let mut skip = 0;
            for x in 0..buffer.area.width {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let symbol = buffer[(x, y)].symbol();
                skip = UnicodeWidthStr::width(symbol).saturating_sub(1);
                row.push_str(symbol);
            }
            row
        })
        .collect()
}

fn config(lang: Lang) -> Config {
    let mut config = Config::default();
    config.general.lang = lang;
    config.session.result_input_delay_ms = 0;
    config.ui.animation_enabled = false;
    config
}

/// A new player: the 15-minute introductory menu (2 + 4 + 4 + 3 + 2).
fn menu_app(lang: Lang) -> TestApp {
    let mut app = test_app_at(config(lang), chrono::NaiveDate::from_ymd_opt(2026, 11, 19).unwrap());
    app.handle_key(key(KeyCode::Char('m')));
    assert_eq!(app.screen, Screen::Menu);
    let minutes = app.daily_menu.as_ref().unwrap().steps.iter().map(|step| step.minutes).collect::<Vec<_>>();
    assert_eq!(minutes, vec![2, 4, 4, 3, 2]);
    app
}

/// Start the selected step and stop at its countdown.
fn to_countdown(app: &mut rs_typing_trainer::app::App, now: Instant) {
    app.handle_key_at(key(KeyCode::Enter), now);
    assert!(matches!(app.screen, Screen::MenuFocus { .. }));
    app.handle_key_at(key(KeyCode::Char('x')), now + Duration::from_millis(10));
    assert!(matches!(app.screen, Screen::Countdown { .. }));
}

/// Play the step that is counting down to its end (the session is shortened).
fn finish_step(app: &mut rs_typing_trainer::app::App, now: Instant) {
    app.config.session.duration_seconds = 5;
    app.on_tick_at(Duration::from_secs(3), now + Duration::from_secs(4)).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    app.on_tick_at(Duration::from_secs(5), now + Duration::from_secs(10)).unwrap();
    assert_eq!(app.screen, Screen::Result);
}

fn band_rows<'a>(rows: &'a [String], marker: &str) -> Vec<&'a String> {
    rows.iter().filter(|row| row.contains(marker)).collect()
}

#[test]
fn countdown_and_result_show_the_whole_menu_in_japanese_at_80_by_24() {
    let mut app = menu_app(Lang::Ja);
    let now = Instant::now();
    to_countdown(&mut app, now);
    let rows = render(&app, 80, 24);
    let text = rows.join("\n");
    let band = band_rows(&rows, "段 1/5");
    assert_eq!(band.len(), 1, "{text}");
    assert!(
        band[0].contains("▶ウォームアップ 2 › 遷移 4 › 遷移 4 › 確認 3 › 仕上げ 2  段 1/5 · 残り 15 分"),
        "{text}"
    );
    println!("countdown (ja, 80x24):\n{text}");

    finish_step(&mut app, now);
    let rows = render(&app, 80, 24);
    let text = rows.join("\n");
    // 77 columns in full, 76 available: the done step's minutes go first.
    let band = band_rows(&rows, "段 2/5");
    assert_eq!(band.len(), 1, "{text}");
    assert!(
        band[0].contains("✓ウォームアップ › ▶遷移 4 › 遷移 4 › 確認 3 › 仕上げ 2  段 2/5 · 残り 13 分"),
        "{text}"
    );
    // The existing rows keep their place: the hint and the save line.
    assert_eq!(rows.iter().filter(|row| row.contains("[Enter] 次の段")).count(), 1, "{text}");
    assert_eq!(rows.iter().filter(|row| row.contains("保存")).count(), 1, "{text}");
    println!("result (ja, 80x24):\n{text}");

    // Into the second step: the band moves with it.
    let next = now + Duration::from_secs(20);
    app.handle_key_at(key(KeyCode::Enter), next);
    assert!(matches!(app.screen, Screen::MenuFocus { .. }));
    app.handle_key_at(key(KeyCode::Char('x')), next + Duration::from_millis(10));
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    let text = render(&app, 80, 24).join("\n");
    assert!(
        text.contains("✓ウォームアップ 2 › ▶遷移 4 › 遷移 4 › 確認 3 › 仕上げ 2  段 2/5 · 残り 13 分"),
        "{text}"
    );
}

#[test]
fn countdown_and_result_show_the_whole_menu_in_english_at_80_by_24() {
    let mut app = menu_app(Lang::En);
    let now = Instant::now();
    to_countdown(&mut app, now);
    let rows = render(&app, 80, 24);
    let text = rows.join("\n");
    // 85 columns with the minutes: they are dropped to fit 78.
    let band = band_rows(&rows, "Step 1/5");
    assert_eq!(band.len(), 1, "{text}");
    assert!(
        band[0].contains("▶Warm-up › Transition › Transition › Check › Finish  Step 1/5 · 15 min left"),
        "{text}"
    );
    println!("countdown (en, 80x24):\n{text}");

    finish_step(&mut app, now);
    let rows = render(&app, 80, 24);
    let text = rows.join("\n");
    let band = band_rows(&rows, "Step 2/5");
    assert_eq!(band.len(), 1, "{text}");
    assert!(
        band[0].contains("✓Warm-up › ▶Transition › Transition › Check › Finish  Step 2/5 · 13 min left"),
        "{text}"
    );
    assert_eq!(rows.iter().filter(|row| row.contains("[Enter] Next step")).count(), 1, "{text}");
    assert_eq!(rows.iter().filter(|row| row.contains("Saved to DB")).count(), 1, "{text}");
    println!("result (en, 80x24):\n{text}");

    // Wide enough, the minutes come back.
    let wide = render(&app, 120, 30).join("\n");
    assert!(
        wide.contains("✓Warm-up 2 › ▶Transition 4 › Transition 4 › Check 3 › Finish 2  Step 2/5 · 13 min left"),
        "{wide}"
    );
}

#[test]
fn a_narrow_window_shows_only_the_position_and_the_current_step() {
    for (lang, expected) in [
        (Lang::Ja, "段 1/5 ▶ウォームアップ · 残り 15 分"),
        (Lang::En, "Step 1/5 ▶Warm-up · 15 min left"),
    ] {
        let mut app = menu_app(lang);
        to_countdown(&mut app, Instant::now());
        let rows = render(&app, 60, 24);
        let text = rows.join("\n");
        assert!(text.contains(expected), "{lang:?}:\n{text}");
        assert!(!text.contains("仕上げ") && !text.contains("Finish"), "{lang:?}:\n{text}");
    }
}

#[test]
fn the_band_fits_the_given_width_at_every_step() {
    let app = menu_app(Lang::Ja);
    let menu = app.daily_menu.as_ref().unwrap();
    for width in [20, 40, 60, 76, 100] {
        for current in 0..menu.steps.len() {
            let completed = (0..current).collect::<BTreeSet<_>>();
            for lang in [Lang::Ja, Lang::En] {
                let band = ui::menu::progress_band(menu, &completed, Some(current), current + 1, lang, width);
                assert!(UnicodeWidthStr::width(band.as_str()) <= width, "{width}: {band}");
            }
        }
    }
    // The minutes left count every step not done yet plus the marked one.
    let completed = BTreeSet::from([0, 2]);
    let band = ui::menu::progress_band(menu, &completed, Some(1), 2, Lang::Ja, 200);
    assert_eq!(band, "✓ウォームアップ 2 › ▶遷移 4 › ✓遷移 4 › 確認 3 › 仕上げ 2  段 2/5 · 残り 9 分");
    // After the last step nothing is marked and nothing is left.
    let all = (0..menu.steps.len()).collect::<BTreeSet<_>>();
    let band = ui::menu::progress_band(menu, &all, None, 5, Lang::En, 200);
    assert_eq!(
        band,
        "✓Warm-up 2 › ✓Transition 4 › ✓Transition 4 › ✓Check 3 › ✓Finish 2  Step 5/5 · 0 min left"
    );
}

#[test]
fn individual_training_shows_no_band() {
    for lang in [Lang::Ja, Lang::En] {
        let mut app = test_app_at(config(lang), chrono::NaiveDate::from_ymd_opt(2026, 11, 19).unwrap());
        let now = Instant::now();
        app.handle_key_at(key(KeyCode::Char(' ')), now);
        assert_eq!(app.screen, Screen::ModeSelect);
        app.handle_key_at(key(KeyCode::Enter), now);
        assert!(matches!(app.screen, Screen::Countdown { .. }));
        let text = render(&app, 80, 24).join("\n");
        assert!(
            !text.contains("段 1/") && !text.contains("Step 1/") && !text.contains('›'),
            "{lang:?}:\n{text}"
        );

        finish_step(&mut app, now);
        let text = render(&app, 80, 24).join("\n");
        assert!(
            !text.contains("残り") && !text.contains("min left") && !text.contains('›'),
            "{lang:?}:\n{text}"
        );
    }
}

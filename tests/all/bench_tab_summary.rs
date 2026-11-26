//! MYTASK-3644: the Summary of the analysis screen's benchmark tab.

use std::time::Duration;

use chrono::{TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Rect, Size};
use rs_typing_trainer::app::{ANALYSIS_BENCH_TAB, ANALYSIS_COACH_TAB};
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::record::{SessionRecord, db};
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::bigtext_image::resolve_big_font;
use rs_typing_trainer::ui::theme::Theme;
use tempfile::TempDir;

use crate::common;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render_buffer(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn rows(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect()
}

fn text(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> String {
    rows(&render_buffer(app, width, height)).join("\n")
}

fn compact(text: &str) -> String {
    text.chars().filter(|character| !character.is_whitespace()).collect()
}

const BLOCKS: &str = "▀▄█▌▐▖▗▘▙▚▛▜▝▞▟";

fn has_blocks(text: &str) -> bool {
    text.chars().any(|character| BLOCKS.contains(character))
}

/// The outer frame stays intact: every row starts and ends with the border.
fn assert_inside_frame(buffer: &Buffer) {
    let (width, height) = (buffer.area.width, buffer.area.height);
    for y in 1..height - 1 {
        assert_eq!(buffer[(0, y)].symbol(), "│", "row {y}:\n{}", rows(buffer).join("\n"));
        assert_eq!(buffer[(width - 1, y)].symbol(), "│", "row {y}:\n{}", rows(buffer).join("\n"));
    }
}

/// A benchmark part saved on November `day` at 12:00 UTC (the same local
/// date from UTC−11 to UTC+11), minute `order` keeping the parts in order.
fn save_part(path: &std::path::Path, bench_id: &str, wpm: f64, accuracy: f64, day: u32, order: u32) {
    let config = Config {
        mode: rs_typing_trainer::config::ModeConfig {
            kind: ModeKind::Words,
            ..Config::default().mode
        },
        ..Config::default()
    };
    let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("as"));
    session.activate(Duration::ZERO);
    session.input_char_with_code(Duration::from_millis(100), "a", layout.stroke_for('a').map(|stroke| stroke.code));
    session.input_char_with_code(Duration::from_millis(250), "s", layout.stroke_for('s').map(|stroke| stroke.code));
    session.tick(Duration::from_secs(1));
    let mut record = SessionRecord::from_log(&config, session.into_log(), Utc.with_ymd_and_hms(2026, 11, day, 12, order, 0).unwrap());
    record.header.net_wpm = wpm;
    record.header.accuracy = accuracy;
    record.header.mode = "bench".into();
    record.header.bench_id = Some(bench_id.into());
    db::save(path, &record).unwrap();
}

/// Three `bench-v2` runs: 11/12 (form 01), 11/15 (form 02) and 11/19
/// (form 03, with the optional part D). On 11/19 A and C beat their best,
/// B is slower but its accuracy is a new best, and D is a first result.
fn seed(lang: Lang) -> (TempDir, Config) {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    for (day, form, parts) in [
        (12, 1, vec![("A", 50.0, 0.95), ("B", 40.0, 0.96), ("C", 45.0, 0.93)]),
        (15, 2, vec![("A", 52.0, 0.96), ("B", 41.0, 0.955), ("C", 44.0, 0.94)]),
        (19, 3, vec![("A", 58.0, 0.97), ("B", 39.5, 0.97), ("C", 47.0, 0.92), ("D", 30.0, 0.9)]),
    ] {
        for (order, (part, wpm, accuracy)) in parts.into_iter().enumerate() {
            save_part(&path, &format!("bench-v2/{part}/{form:02}"), wpm, accuracy, day, order as u32);
        }
    }
    let mut config = Config::default();
    config.general.lang = lang;
    config.storage.db_path = Some(path);
    (directory, config)
}

fn v3_split_record(id: i64, day: u32, part: &str, form: u8, half_a_wpm: f64, half_b_wpm: f64) -> SessionRecord {
    let intervals_us = [(12_000_000.0 / half_a_wpm).round() as u64, (12_000_000.0 / half_b_wpm).round() as u64];
    let mut items = Vec::new();
    let mut keystrokes = Vec::new();
    for item_idx in 0..4_u32 {
        let shown_us = u64::from(item_idx) * 1_000_000;
        items.push(rs_typing_trainer::engine::session::ItemLog {
            idx: item_idx,
            text: "aa".into(),
            reading: None,
            kind: None,
            shown_us,
            done_us: None,
            outcome: rs_typing_trainer::engine::session::ItemOutcome::Completed,
        });
        for pos in 0..2_u32 {
            keystrokes.push(rs_typing_trainer::engine::session::KeystrokeLog {
                seq: item_idx * 2 + pos,
                t_us: shown_us + 100_000 + u64::from(pos) * intervals_us[item_idx as usize % 2],
                release_us: None,
                item_idx,
                pos,
                kind: rs_typing_trainer::engine::session::KeystrokeKind::Char,
                typed: Some("a".into()),
                expected: Some("a".into()),
                correct: true,
                code: Some("KeyA".into()),
            });
        }
    }
    let mut record = SessionRecord::from_log(
        &Config::default(),
        rs_typing_trainer::engine::session::SessionLog {
            duration_us: 4_000_000,
            items,
            keystrokes,
            ..Default::default()
        },
        Utc.with_ymd_and_hms(2026, 11, day, 12, u32::from(form), 0).unwrap(),
    );
    record.header.id = Some(id);
    record.header.mode = "bench".into();
    record.header.bench_id = Some(format!("bench-v3/{part}/{form:02}"));
    record
}

fn seed_v3(lang: Lang) -> (TempDir, Config) {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    let mut id = 1_i64;
    for (day, form, half_a, half_b) in [(10, 1, 60.0, 40.0), (11, 2, 120.0, 40.0), (19, 3, 62.0, 42.0)] {
        for part in ["EW", "ES", "JW", "JS", "PW"] {
            let mut record = v3_split_record(id, day, part, form, half_a, half_b);
            record.header.id = None;
            db::save(&path, &record).unwrap();
            id += 1;
        }
    }
    // Retained history must not make the v3 Summary fall back to v2.
    save_part(&path, "bench-v2/A/01", 35.0, 0.95, 8, 0);
    let mut config = Config::default();
    config.general.lang = lang;
    config.storage.db_path = Some(path);
    (directory, config)
}

fn seed_v3_n(lang: Lang, run_count: usize) -> (TempDir, Config) {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    let rows = [
        (10, 1, 60.0, 40.0),
        (11, 2, 120.0, 40.0),
        (12, 3, 70.0, 45.0),
        (13, 4, 90.0, 50.0),
        (19, 5, 62.0, 42.0),
    ];
    for (day, form, half_a, half_b) in rows.into_iter().take(run_count) {
        for part in ["EW", "ES", "JW", "JS", "PW"] {
            let mut record = v3_split_record(0, day, part, form, half_a, half_b);
            record.header.id = None;
            db::save(&path, &record).unwrap();
        }
    }
    let mut config = Config::default();
    config.general.lang = lang;
    config.storage.db_path = Some(path);
    (directory, config)
}

fn open_bench(config: Config, graphics: GraphicsMode) -> common::TestApp {
    let mut app = common::test_app_at(config, chrono::NaiveDate::from_ymd_opt(2026, 11, 19).unwrap());
    app.set_graphics_detection(GraphicsDetection {
        mode: graphics,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Disabled,
    });
    app.handle_key(key(KeyCode::Char('a')));
    app.analysis_tab = ANALYSIS_BENCH_TAB;
    app
}

#[test]
fn s_toggles_the_bench_summary_and_moving_tabs_leaves_it() {
    let (_directory, config) = seed(Lang::Ja);
    let mut app = open_bench(config, GraphicsMode::Off);
    assert!(!app.analysis_bench_summary);
    let tab = text(&app, 80, 24);
    assert!(compact(&tab).contains("[S]Summary"), "{tab}");
    app.handle_key(key(KeyCode::Char('s')));
    assert!(app.analysis_bench_summary);
    let summary = text(&app, 80, 24);
    assert!(compact(&summary).contains("Summary[S]詳細[Esc]戻る"), "{summary}");
    // No tab bar on the Summary.
    assert!(!compact(&summary).contains("コーチ"), "{summary}");
    app.handle_key(key(KeyCode::Char('S')));
    assert!(!app.analysis_bench_summary);
    assert!(compact(&text(&app, 80, 24)).contains("前回比"));
    // Moving to another tab and back shows the tab again.
    app.handle_key(key(KeyCode::Char('s')));
    app.handle_key(key(KeyCode::Tab));
    assert!(!app.analysis_bench_summary);
    app.handle_key(key(KeyCode::BackTab));
    assert_eq!(app.analysis_tab, ANALYSIS_BENCH_TAB);
    assert!(!app.analysis_bench_summary);
    app.handle_key(key(KeyCode::Char('s')));
    app.handle_key(key(KeyCode::Left));
    assert!(!app.analysis_bench_summary);
    // Not kept across openings of the analysis screen.
    app.analysis_tab = ANALYSIS_BENCH_TAB;
    app.handle_key(key(KeyCode::Char('s')));
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Char('a')));
    assert_eq!(app.analysis_tab, ANALYSIS_COACH_TAB);
    assert!(!app.analysis_bench_summary);
    // `S` on other tabs does not turn it on.
    app.handle_key(key(KeyCode::Char('s')));
    assert!(!app.analysis_bench_summary);
}

#[test]
fn text_summary_draws_each_part_with_big_text_and_differences() {
    for lang in [Lang::Ja, Lang::En] {
        let (_directory, config) = seed(lang);
        let mut app = open_bench(config, GraphicsMode::Off);
        app.handle_key(key(KeyCode::Char('s')));
        let buffer = render_buffer(&app, 80, 24);
        assert_inside_frame(&buffer);
        let rows = rows(&buffer);
        let text = rows.join("\n");
        if std::env::var_os("BENCH_SUMMARY_PRINT").is_some() {
            println!("{text}");
        }
        let flat = compact(&text);
        // The four values in big text: four columns of blocks on the same rows.
        let quarters = rows
            .iter()
            .filter(|row| {
                let characters = row.chars().collect::<Vec<_>>();
                let quarter = characters.len() / 4;
                (0..4).all(|index| has_blocks(&characters[index * quarter..(index + 1) * quarter].iter().collect::<String>()))
            })
            .count();
        assert!(quarters >= 4, "{text}");
        match lang {
            Lang::Ja => {
                assert!(flat.contains("ベンチ2026-11-19"), "{text}");
                assert!(flat.contains("bench-v2・フォーム03・パートA・B・C・D"), "{text}");
                // Short part names: «パート A 英単語と句» does not fit a quarter of 80 columns.
                assert!(flat.contains("パートAパートBパートCパートD"), "{text}");
                assert!(flat.contains("WPM正確さ97.0%"), "{text}");
                // A: 58.0 against 52.0 before and the median of (50.0, 52.0).
                assert!(flat.contains("前回比▲+6.0WPM"), "{text}");
                assert!(flat.contains("直近3回比▲+6.0"), "{text}");
                // B is slower than last time.
                assert!(flat.contains("前回比▼-1.5WPM"), "{text}");
                // D: a first result.
                assert!(flat.contains("前回比初回"), "{text}");
                assert!(flat.contains("技能の8軸"), "{text}");
                assert!(flat.contains("持続速度"), "{text}");
                assert!(flat.contains("▲自己ベスト更新!パートA・C"), "{text}");
            }
            Lang::En => {
                assert!(flat.contains("Benchmark2026-11-19"), "{text}");
                assert!(flat.contains("bench-v2/form03/PartA,B,C,D"), "{text}");
                assert!(flat.contains("WPMacc97.0%"), "{text}");
                assert!(flat.contains("vsprevious▲+6.0"), "{text}");
                assert!(flat.contains("vsprevious▼-1.5"), "{text}");
                assert!(flat.contains("recent-3▲+6.0"), "{text}");
                assert!(flat.contains("prevfirstresult"), "{text}");
                assert!(flat.contains("Sustainedspeed"), "{text}");
                assert!(flat.contains("▲Newbest!PartA,C"), "{text}");
            }
        }
    }
}

#[test]
fn summary_view_follows_the_result_screen() {
    use rs_typing_trainer::ui::bench_tab_summary::{app_view, latest_bench_run};
    let (_directory, config) = seed(Lang::Ja);
    let app = open_bench(config, GraphicsMode::Off);
    let run = latest_bench_run(&app.analysis_records).unwrap();
    assert_eq!(run.series, "bench-v2");
    assert_eq!(run.form, 3);
    assert_eq!(run.previous_date, chrono::NaiveDate::from_ymd_opt(2026, 11, 15));
    let view = app_view(&app);
    assert_eq!(view.parts.len(), 4);
    let a = &view.parts[0];
    assert_eq!(a.value, "58.0");
    assert_eq!(a.accuracy, "正確さ 97.0%");
    assert_eq!(a.previous.delta, Some(6.0));
    assert_eq!(a.previous.delta_text, "前回比 +6.0 WPM");
    assert_eq!(a.recent.delta_text, "直近 3 回の中央値との差 +6.0 WPM");
    let b = &view.parts[1];
    assert_eq!(b.previous.delta, Some(-1.5));
    // Median of the last three (40.0, 41.0) is 41.0.
    assert_eq!(b.recent.delta, Some(-1.5));
    assert_eq!(view.parts[3].previous.delta_text, "前回比 初回");
    assert_eq!(view.parts[3].previous.delta, None);
    assert_eq!(
        view.events,
        vec!["自己ベスト更新! パート A・C".to_owned(), "正確さの自己ベスト更新! パート A・B".to_owned()]
    );
    assert!(view.highlight);
    assert_eq!(view.axes.len(), 8);
}

#[test]
fn a_new_version_is_not_compared_with_the_old_one() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_part(&path, "bench-v1/A/01", 50.0, 0.95, 12, 0);
    save_part(&path, "bench-v1/B/01", 40.0, 0.95, 12, 1);
    save_part(&path, "bench-v2/A/01", 55.0, 0.96, 19, 0);
    save_part(&path, "bench-v2/B/01", 39.0, 0.94, 19, 1);
    for lang in [Lang::Ja, Lang::En] {
        let mut config = Config::default();
        config.general.lang = lang;
        config.storage.db_path = Some(path.clone());
        let mut app = open_bench(config, GraphicsMode::Off);
        let view = rs_typing_trainer::ui::bench_tab_summary::app_view(&app);
        // A changed in v2: no difference across versions. B and D did not.
        assert_eq!(view.parts[0].previous.delta, None);
        assert_eq!(view.parts[1].previous.delta, Some(-1.0));
        app.handle_key(key(KeyCode::Char('s')));
        let text = text(&app, 80, 24);
        let flat = compact(&text);
        match lang {
            Lang::Ja => {
                assert_eq!(view.parts[0].previous.delta_text, "版が違うため比較なし");
                assert!(flat.contains("版が違うため比較なし"), "{text}");
                assert!(flat.contains("前回比▼-1.0WPM"), "{text}");
                assert!(!flat.contains("初回"), "{text}");
                assert!(flat.contains("前回のベンチ:2026-11-12"), "{text}");
            }
            Lang::En => {
                assert!(flat.contains("vsprevious▼-1.0WPM"), "{text}");
                assert!(flat.contains("Differentversion:nocomparison"), "{text}");
                assert!(flat.contains("Previousbenchmark:2026-11-12"), "{text}");
            }
        }
    }
}

#[test]
fn summary_without_benchmarks_says_so() {
    for (lang, expected) in [(Lang::Ja, "ベンチの記録はまだありません"), (Lang::En, "Nobenchmarkrecordedyet")] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut app = open_bench(config, GraphicsMode::Kitty);
        app.handle_key(key(KeyCode::Char('s')));
        for (width, height) in [(80, 24), (160, 45), (40, 12)] {
            let buffer = render_buffer(&app, width, height);
            assert_inside_frame(&buffer);
            let rows = rows(&buffer);
            let text = rows.join("\n");
            assert!(compact(&text).contains(expected), "{text}");
            // In the middle of the screen.
            let row = rows.iter().position(|row| compact(row).contains(expected)).unwrap();
            assert!((height as usize / 3..=height as usize * 2 / 3).contains(&row), "{row}\n{text}");
        }
    }
}

#[test]
fn small_windows_keep_the_frame() {
    let (_directory, config) = seed(Lang::En);
    let mut app = open_bench(config, GraphicsMode::Off);
    app.handle_key(key(KeyCode::Char('s')));
    for (width, height) in [(60, 14), (40, 16), (30, 8), (120, 40)] {
        let buffer = render_buffer(&app, width, height);
        assert_inside_frame(&buffer);
    }
    let text = text(&app, 60, 14);
    assert!(compact(&text).contains("58.0"), "{text}");
}

#[test]
fn image_summary_draws_four_large_values() {
    use rs_typing_trainer::ui::analysis_image::{bench_summary_value_height, render_bench_summary};
    use rs_typing_trainer::ui::bench_tab_summary::{app_view, image_spec, uses_image};
    let theme = Theme::from_name("default");
    let font = resolve_big_font("biz-ud-gothic", std::path::Path::new("/unused"));
    let cell_pixels = CellPixels::new(9, 20);
    for (lang, name) in [(Lang::Ja, "bench-summary-ja.png"), (Lang::En, "bench-summary-en.png")] {
        let (_directory, config) = seed(lang);
        let app = open_bench(config, GraphicsMode::Kitty);
        let view = app_view(&app);
        assert!(uses_image(&app, &view, Rect::new(0, 0, 158, 42)));
        assert!(!uses_image(&app, &view, Rect::new(0, 0, 100, 42)));
        let spec = image_spec(&view, &theme, Size::new(158, 42));
        let image = render_bench_summary(&font, cell_pixels, &spec).unwrap();
        assert_eq!((image.width(), image.height()), (158 * 9, 42 * 20));
        // Four values side by side: each glyph box is at least a tenth of the height.
        let ratio = bench_summary_value_height(&spec, cell_pixels) / image.height() as f32;
        assert!(ratio >= 0.10, "{ratio}");
        // Accent-colored value pixels in each quarter, below the heading band.
        let accent = [41u8, 184, 219];
        let top = image.height() / 7;
        let bottom = image.height() / 2;
        for quarter in 0..4 {
            let (left, right) = (image.width() * (1 + 4 * quarter) / 18, image.width() * (4 + 4 * quarter) / 18);
            let rows_with_value = (top..bottom)
                .filter(|y| (left..right).any(|x| image.get_pixel(x, *y).0[..3] == accent))
                .count();
            assert!(
                rows_with_value as f32 / image.height() as f32 >= 0.07,
                "quarter {quarter}: {rows_with_value}"
            );
        }
        if let Some(directory) = std::env::var_os("BENCH_SUMMARY_PNG_DIR") {
            image.save(std::path::Path::new(&directory).join(name)).unwrap();
        }
        // The smallest window that uses the image still fits everything.
        let small = image_spec(&view, &theme, Size::new(120, 29));
        let image = render_bench_summary(&font, cell_pixels, &small).unwrap();
        assert_eq!((image.width(), image.height()), (120 * 9, 29 * 20));
        assert!(bench_summary_value_height(&small, cell_pixels) / image.height() as f32 >= 0.10);
        if let Some(directory) = std::env::var_os("BENCH_SUMMARY_PNG_DIR") {
            image
                .save(std::path::Path::new(&directory).join(name.replace(".png", "-120x29.png")))
                .unwrap();
        }
    }
}

#[test]
fn v3_summary_view_has_language_cards_grades_axes_trends_and_reliability() {
    use rs_typing_trainer::ui::bench_tab_summary::app_view;

    for (lang, labels, tempo, accuracy, unchanged, best, reference) in [
        (
            Lang::En,
            ["English", "Japanese", "Transitions"],
            "tempo",
            "accuracy",
            "no change",
            "best",
            "reference",
        ),
        (
            Lang::Ja,
            ["英語", "日本語", "移り変わり"],
            "テンポ",
            "正確さ",
            "変化なし",
            "自己ベスト",
            "参考",
        ),
    ] {
        let (_directory, config) = seed_v3(lang);
        let app = open_bench(config, GraphicsMode::Off);
        let view = app_view(&app);
        assert!(view.heading.contains("2026-11-19"), "{view:#?}");
        assert!(view.subheading.contains("bench-v3") && view.subheading.contains("03"), "{view:#?}");
        assert_eq!(view.parts.len(), 3, "{view:#?}");
        for (card, label) in view.parts.iter().zip(labels) {
            assert!(compact(&card.label).contains(&compact(label)), "{view:#?}");
            assert!(compact(&card.accuracy).contains(&compact(tempo)), "{view:#?}");
            assert!(compact(&card.accuracy).contains(&compact(accuracy)), "{view:#?}");
            assert!(compact(&card.previous.delta_text).contains(&compact(unchanged)), "{view:#?}");
            assert!(compact(&card.recent.delta_text).contains(&compact(best)), "{view:#?}");
        }
        assert_eq!(view.axes.len(), 8, "{view:#?}");
        let supporting = compact(&view.events.join("\n"));
        for expected in ["grades-v2", "SEM", "MDC95", reference, "EW", "ES", "JW", "JS", "PW"] {
            assert!(supporting.contains(&compact(expected)), "missing {expected:?}: {view:#?}");
        }
    }
}

#[test]
fn v3_summary_renders_in_both_languages_and_falls_back_to_text_when_small() {
    for (lang, expected) in [
        (
            Lang::En,
            ["Benchmark", "bench-v3", "English", "grades-v2", "8 skill axes", "SEM", "MDC95"],
        ),
        (Lang::Ja, ["ベンチ", "bench-v3", "英語", "grades-v2", "技能の 8 軸", "SEM", "MDC95"]),
    ] {
        for graphics in [GraphicsMode::Off, GraphicsMode::Kitty] {
            let (_directory, config) = seed_v3(lang);
            let mut app = open_bench(config, graphics);
            app.handle_key(key(KeyCode::Char('s')));
            for (width, height) in [(80, 24), (120, 40), (158, 42)] {
                let buffer = render_buffer(&app, width, height);
                assert_inside_frame(&buffer);
                if graphics == GraphicsMode::Off || width < 120 || height < 29 {
                    let output = rows(&buffer).join("\n");
                    let normalized = compact(&output);
                    for value in expected {
                        assert!(
                            normalized.contains(&compact(value)),
                            "{graphics:?} {width}x{height}, missing {value:?}:\n{output}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn v3_summary_image_reuses_the_existing_summary_renderer() {
    use rs_typing_trainer::ui::analysis_image::{bench_summary_value_height, render_bench_summary};
    use rs_typing_trainer::ui::bench_tab_summary::{app_view, image_spec, uses_image};

    let theme = Theme::from_name("default");
    let font = resolve_big_font("biz-ud-gothic", std::path::Path::new("/unused"));
    let cell_pixels = CellPixels::new(9, 20);
    for lang in [Lang::Ja, Lang::En] {
        let (_directory, config) = seed_v3(lang);
        let app = open_bench(config, GraphicsMode::Kitty);
        let view = app_view(&app);
        assert!(uses_image(&app, &view, Rect::new(0, 0, 158, 42)));
        assert!(!uses_image(&app, &view, Rect::new(0, 0, 80, 24)));
        let spec = image_spec(&view, &theme, Size::new(158, 42));
        assert_eq!(spec.parts.len(), 3);
        assert_eq!(spec.axes.len(), 8);
        assert_eq!(spec.radar.as_ref().unwrap().axes.len(), 8);
        assert_eq!(spec.trends.len(), 5);
        assert!(
            spec.trends
                .iter()
                .all(|(_, chart)| chart.raw_values.len() == 3 && chart.values.len() == 3)
        );
        let image = render_bench_summary(&font, cell_pixels, &spec).unwrap();
        assert_eq!((image.width(), image.height()), (158 * 9, 42 * 20));
        assert!(bench_summary_value_height(&spec, cell_pixels) / image.height() as f32 >= 0.10);
    }
}

#[test]
fn v3_summary_reliability_stops_calling_five_runs_a_reference() {
    use rs_typing_trainer::ui::bench_tab_summary::app_view;

    for lang in [Lang::Ja, Lang::En] {
        let (_directory, config) = seed_v3_n(lang, 4);
        let four = app_view(&open_bench(config, GraphicsMode::Off));
        assert!(
            four.events.iter().any(|line| line.contains("reference") || line.contains("参考")),
            "{four:#?}"
        );

        let (_directory, config) = seed_v3_n(lang, 5);
        let five = app_view(&open_bench(config, GraphicsMode::Off));
        assert!(
            five.events.iter().all(|line| !line.contains("reference") && !line.contains("参考")),
            "{five:#?}"
        );
    }
}

#[test]
fn v3_summary_two_runs_keeps_raw_axes_and_text_trial_series() {
    use rs_typing_trainer::ui::analysis_image::render_bench_summary;
    use rs_typing_trainer::ui::bench_tab_summary::{app_view, image_spec};

    let (_directory, config) = seed_v3_n(Lang::En, 2);
    let mut app = open_bench(config, GraphicsMode::Off);
    let view = app_view(&app);
    assert_eq!(view.axes.len(), 8);
    assert!(view.axes.iter().any(|axis| axis.value.ends_with('*')), "{view:#?}");
    let spec = image_spec(&view, &Theme::from_name("default"), Size::new(158, 42));
    let radar = spec.radar.as_ref().expect("v3 radar area");
    assert!(radar.axes.iter().any(|axis| axis.label.contains('*') && axis.value.is_none()));
    let font = resolve_big_font("biz-ud-gothic", std::path::Path::new("/unused"));
    render_bench_summary(&font, CellPixels::new(9, 20), &spec).unwrap();

    app.handle_key(key(KeyCode::Char('s')));
    let output = text(&app, 80, 24);
    for part in ["EW", "ES", "JW", "JS", "PW"] {
        assert!(output.contains(part), "missing {part}:\n{output}");
    }
    assert!(output.contains('·') && output.contains('━'), "{output}");
}

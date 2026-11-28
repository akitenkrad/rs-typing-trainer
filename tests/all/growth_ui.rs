use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::analysis::bench_v3::{BenchV3Change, BenchV3Group, BenchV3Reliability};
use rs_typing_trainer::analysis::growth::{
    Accumulation, BenchGrowth, BenchPoint, BenchV3Growth, BenchV3GrowthTarget, GrowthReport, LanguageGrowth, PersonalBest, TrendPeriod,
};
use rs_typing_trainer::analysis::progress::{DailyPoint, DailyTrendSource};
use rs_typing_trainer::app::{ANALYSIS_GROWTH_TAB, Screen};
use rs_typing_trainer::bench::{BenchPart, BenchV3Part};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::ui;

use crate::common::test_app_at;

fn day(value: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 11, value).unwrap()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn fixture(lang: Lang) -> crate::common::TestApp {
    let mut config = Config::default();
    config.general.lang = lang;
    let mut app = test_app_at(config, day(26));
    app.screen = Screen::Analysis;
    app.analysis_tab = ANALYSIS_GROWTH_TAB;
    app.analysis_growth = GrowthReport {
        languages: vec![LanguageGrowth {
            language: "en".into(),
            source: DailyTrendSource::Bench,
            recorded_days: 14,
            first: Some(TrendPeriod {
                start: day(1),
                end: day(7),
                speed: Some(32.1),
                accuracy: Some(0.942),
            }),
            recent: Some(TrendPeriod {
                start: day(20),
                end: day(26),
                speed: Some(41.5),
                accuracy: Some(0.970),
            }),
            month_ago: None,
            quarter_ago: None,
            change: Some(rs_typing_trainer::analysis::growth::Change {
                speed: 9.4,
                speed_percent: Some(29.3),
                accuracy_points: Some(2.8),
            }),
            points: (1..=26)
                .step_by(2)
                .map(|date| DailyPoint {
                    date: day(date),
                    value: 30.0 + f64::from(date) / 2.0,
                })
                .collect(),
            benches: vec![],
        }],
        accumulation: Accumulation {
            duration_seconds: 45_000,
            month_duration_seconds: 15_000,
            practice_days: 42,
            current_streak: 5,
            longest_streak: 9,
            keystrokes: 183_402,
        },
        personal_bests: vec![],
        bench_v3: vec![],
        promotions: vec![],
        graduations: vec![],
    };
    app
}

fn v3_fixture(lang: Lang) -> crate::common::TestApp {
    let mut app = fixture(lang);
    let language = &mut app.analysis_growth.languages[0];
    language.source = DailyTrendSource::BenchV3;
    language.points = [(1, 40.0), (2, 45.0), (3, 50.0), (4, 55.0)]
        .into_iter()
        .map(|(date, value)| DailyPoint { date: day(date), value })
        .collect();
    language.benches = vec![BenchGrowth {
        part: BenchPart::A,
        first: BenchPoint { date: day(1), wpm: 34.0 },
        latest: BenchPoint { date: day(4), wpm: 34.6 },
        best: BenchPoint { date: day(4), wpm: 34.6 },
    }];
    let reliability = Some(BenchV3Reliability {
        sample_count: 3,
        sem_wpm: 0.9,
        mdc95_wpm: 2.5,
        reference_only: true,
    });
    let row = |target| BenchV3Growth {
        target,
        first: BenchPoint { date: day(1), wpm: 40.0 },
        latest: BenchPoint { date: day(4), wpm: 55.0 },
        best: BenchPoint { date: day(4), wpm: 55.0 },
        raw: [(1, 40.0), (2, 50.0), (3, 45.0), (4, 60.0)]
            .into_iter()
            .map(|(date, wpm)| BenchPoint { date: day(date), wpm })
            .collect(),
        mainline: [(1, 40.0), (2, 45.0), (3, 45.0), (4, 50.0)]
            .into_iter()
            .map(|(date, wpm)| BenchPoint { date: day(date), wpm })
            .collect(),
        reliability,
        change: BenchV3Change::Increase,
    };
    app.analysis_growth.bench_v3 = vec![
        row(BenchV3GrowthTarget::Group(BenchV3Group::English)),
        row(BenchV3GrowthTarget::Part(BenchV3Part::EW)),
    ];
    app
}

fn render(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("")
}

fn render_lines(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    (0..height)
        .map(|y| (0..width).map(|x| terminal.backend().buffer()[(x, y)].symbol()).collect::<String>())
        .collect()
}

#[test]
fn growth_is_the_last_tab_and_tab_reaches_it_without_changing_the_default() {
    let mut app = test_app_at(Config::default(), day(26));
    app.handle_key(key(KeyCode::Char('a')));
    assert_eq!(app.analysis_tab, rs_typing_trainer::app::ANALYSIS_COACH_TAB);
    for _ in 0..7 {
        app.handle_key(key(KeyCode::Tab));
    }
    assert_eq!(app.analysis_tab, ANALYSIS_GROWTH_TAB);
    assert!(render(&app, 120, 40).contains("Growth"));
}

#[test]
fn growth_renders_at_both_sizes_and_localizes_the_all_time_footer() {
    for (lang, tab, footer, section) in [
        (Lang::En, "Growth", "Compares all time", "Since you started"),
        (Lang::Ja, "成長", "全期間で比べます", "はじめの頃と今"),
    ] {
        let app = fixture(lang);
        for (width, height) in [(80, 24), (120, 40)] {
            let text = render(&app, width, height);
            let compact = text.chars().filter(|character| !character.is_whitespace()).collect::<String>();
            assert!(
                compact.contains(&tab.chars().filter(|character| !character.is_whitespace()).collect::<String>()),
                "{width}x{height}: {text}"
            );
            assert!(
                compact.contains(&footer.chars().filter(|character| !character.is_whitespace()).collect::<String>()),
                "{width}x{height}: {text}"
            );
            assert!(
                compact.contains(&section.chars().filter(|character| !character.is_whitespace()).collect::<String>()),
                "{width}x{height}: {text}"
            );
        }
    }
}

#[test]
fn v3_growth_renders_rolling_mainline_raw_points_mdc_and_previous_v2_at_reference_sizes() {
    for (lang, source, rolling, previous) in [
        (
            Lang::En,
            "benchmark v3 (accuracy-adjusted speed)",
            "rolling median of the latest 3 runs",
            "Previous series: v2",
        ),
        (Lang::Ja, "ベンチ v3（正確さを含む速さ）", "直近 3 回の中央値", "以前の系列: v2"),
    ] {
        for (width, height) in [(80, 24), (120, 40)] {
            let mut app = v3_fixture(lang);
            let mut pages = render(&app, width, height);
            for _ in 0..8 {
                app.handle_key(key(KeyCode::PageDown));
                pages.push_str(&render(&app, width, height));
            }
            let compact = pages.chars().filter(|character| !character.is_whitespace()).collect::<String>();
            let contains_compact =
                |needle: &str| compact.contains(&needle.chars().filter(|character| !character.is_whitespace()).collect::<String>());
            assert!(contains_compact(source), "{width}x{height}: {pages}");
            assert!(contains_compact(rolling), "{width}x{height}: {pages}");
            assert!(contains_compact(previous), "{width}x{height}: {pages}");
            assert!(
                contains_compact("English summary") || contains_compact("英語まとめ"),
                "{width}x{height}: {pages}"
            );
            assert!(
                pages.chars().any(|character| ('\u{2800}'..='\u{28ff}').contains(&character)),
                "{width}x{height}: {pages}"
            );
        }
    }
}

#[test]
fn text_growth_chart_uses_braille_and_labels_its_first_and_latest_points() {
    let app = fixture(Lang::En);
    let text = render(&app, 120, 40);
    assert!(text.chars().any(|character| ('\u{2800}'..='\u{28ff}').contains(&character)), "{text}");
    assert!(text.contains("first") && text.contains("latest") && text.contains("WPM"), "{text}");
}

#[test]
fn growth_scroll_has_a_rendered_upper_bound_and_supports_page_keys() {
    let mut app = fixture(Lang::Ja);
    let _ = render(&app, 80, 24);
    app.handle_key(key(KeyCode::PageDown));
    let first = app.analysis_growth_scroll;
    assert!(first > 0);
    for _ in 0..100 {
        app.handle_key(key(KeyCode::PageDown));
    }
    let maximum = app.analysis_growth_scroll;
    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.analysis_growth_scroll, maximum);
    app.handle_key(key(KeyCode::PageUp));
    assert!(app.analysis_growth_scroll < maximum);
}

#[test]
fn period_and_mode_filters_do_not_change_the_cached_growth_report() {
    let mut app = fixture(Lang::En);
    let expected = app.analysis_growth.clone();
    app.handle_key(key(KeyCode::Char('p')));
    app.handle_key(key(KeyCode::Char('m')));
    assert_eq!(app.analysis_growth, expected);
}

#[test]
fn insufficient_copy_names_benchmark_or_comparable_speed_records_in_both_languages() {
    for (lang, source, expected) in [
        (Lang::Ja, DailyTrendSource::Bench, "ベンチの記録が7日分"),
        (Lang::En, DailyTrendSource::Bench, "7daysofbenchmarkrecords"),
        (Lang::Ja, DailyTrendSource::CorrectedWpm, "速さを比べられる記録が7日分"),
        (Lang::En, DailyTrendSource::CorrectedWpm, "7daysofcomparablespeedrecords"),
    ] {
        let mut app = fixture(lang);
        app.analysis_growth.languages[0].source = source;
        app.analysis_growth.languages[0].recorded_days = 3;
        app.analysis_growth.languages[0].change = None;
        let text = render(&app, 120, 40);
        let compact = text.chars().filter(|character| !character.is_whitespace()).collect::<String>();
        assert!(compact.contains(expected), "{lang:?} {source:?}: {text}");
    }
}

#[test]
fn chart_falls_back_to_daily_points_and_only_one_day_shows_guidance() {
    let mut app = fixture(Lang::Ja);
    app.analysis_growth.languages[0].points = vec![DailyPoint { date: day(20), value: 30.0 }, DailyPoint { date: day(21), value: 31.0 }];
    let daily = render(&app, 120, 40);
    let daily_compact = daily.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(daily_compact.contains("速さの推移（全期間，日ごとの値）"), "{daily}");
    assert!(daily.chars().any(|character| ('\u{2800}'..='\u{28ff}').contains(&character)), "{daily}");

    app.analysis_growth.languages[0].points.truncate(1);
    let single = render(&app, 120, 40);
    let compact = single.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("記録が2日分たまると図を描けます（今1日分）"), "{single}");
}

#[test]
fn rounded_zero_changes_use_plus_minus_without_a_direction_marker() {
    let mut app = fixture(Lang::Ja);
    let language = &mut app.analysis_growth.languages[0];
    language.change = Some(rs_typing_trainer::analysis::growth::Change {
        speed: 0.04,
        speed_percent: Some(0.04),
        accuracy_points: Some(-0.04),
    });
    language.month_ago = Some(TrendPeriod {
        start: day(1),
        end: day(1),
        speed: Some(41.46),
        accuracy: None,
    });
    language.benches = vec![BenchGrowth {
        part: BenchPart::A,
        first: BenchPoint { date: day(1), wpm: 30.0 },
        latest: BenchPoint { date: day(2), wpm: 30.04 },
        best: BenchPoint { date: day(2), wpm: 30.04 },
    }];
    let text = render(&app, 120, 50);
    assert!(text.matches("±0.0").count() >= 4, "{text}");
    assert!(!text.contains("▲ +0.0") && !text.contains("▼ -0.0"), "{text}");
}

#[test]
fn growth_indents_sections_groups_large_counts_and_wraps_personal_bests_at_value_column() {
    let mut app = fixture(Lang::En);
    app.analysis_growth.personal_bests = (0..5)
        .map(|offset| PersonalBest {
            language: "en".into(),
            part: BenchPart::B,
            date: day(20 - offset),
            wpm: 40.0 + f64::from(offset),
        })
        .collect();
    let rows = render_lines(&app, 80, 50);
    let locate = |needle: &str| {
        rows.iter()
            .enumerate()
            .find_map(|(row, line)| line.find(needle).map(|column| (row, column)))
            .unwrap_or_else(|| panic!("missing {needle:?}:\n{}", rows.join("\n")))
    };
    let (_, language_column) = locate("■ English");
    let (_, heading_column) = locate("Since you started");
    let (_, detail_column) = locate("First 7 days");
    let (months_row, _) = locate("One and three months ago");
    let (benchmarks_row, _) = locate("Benchmarks (first");
    let (chart_row, _) = locate("Speed over time");
    let (accumulation_row, accumulation_column) = locate("■ Accumulation");
    let (_, accumulation_detail_column) = locate("Practice time");
    assert_eq!(heading_column, language_column + 2);
    assert_eq!(detail_column, language_column + 4);
    assert_eq!(accumulation_detail_column, accumulation_column + 2);
    for section_row in [months_row, benchmarks_row, chart_row, accumulation_row] {
        assert!(rows[section_row - 1].trim_matches('│').trim().is_empty(), "{}", rows.join("\n"));
    }
    assert!(rows.iter().any(|row| row.contains("183,402")), "{}", rows.join("\n"));

    let (personal_best_row, label_column) = locate("Recent personal bests");
    let (_, first_value_column) = locate("11/20 English B");
    let wrapped_value_column = rows[personal_best_row + 1]
        .char_indices()
        .find_map(|(column, character)| (!character.is_whitespace() && character != '│').then_some(column))
        .unwrap();
    assert!(first_value_column > label_column);
    assert_eq!(wrapped_value_column, first_value_column, "{}", rows.join("\n"));
}

#[test]
fn japanese_growth_uses_zero_two_and_four_column_section_indents() {
    let app = fixture(Lang::Ja);
    let rows = render_lines(&app, 80, 50);
    let first_content = |line: &str| {
        line.char_indices()
            .find_map(|(column, character)| (!character.is_whitespace() && character != '│').then_some((column, character)))
    };
    let language_row = rows
        .iter()
        .position(|line| first_content(line).is_some_and(|(_, character)| character == '■'))
        .unwrap();
    let (language_column, _) = first_content(&rows[language_row]).unwrap();
    let (heading_column, heading) = first_content(&rows[language_row + 1]).unwrap();
    let (detail_column, detail) = first_content(&rows[language_row + 2]).unwrap();
    assert_eq!((heading_column, heading), (language_column + 2, 'は'));
    assert_eq!((detail_column, detail), (language_column + 4, '最'));

    let accumulation_row = rows
        .iter()
        .enumerate()
        .skip(language_row + 1)
        .find_map(|(row, line)| first_content(line).is_some_and(|(_, character)| character == '■').then_some(row))
        .unwrap();
    let (accumulation_column, _) = first_content(&rows[accumulation_row]).unwrap();
    let (accumulation_detail_column, accumulation_detail) = first_content(&rows[accumulation_row + 1]).unwrap();
    assert_eq!((accumulation_detail_column, accumulation_detail), (accumulation_column + 2, '練'));
}

#[test]
fn growth_ctrl_u_and_ctrl_d_scroll_like_page_keys() {
    let mut page = fixture(Lang::Ja);
    let mut chord = fixture(Lang::Ja);
    let _ = render(&page, 80, 24);
    let _ = render(&chord, 80, 24);
    for _ in 0..30 {
        page.handle_key(key(KeyCode::PageDown));
        chord.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert_eq!(chord.analysis_growth_scroll, page.analysis_growth_scroll);
    }
    assert!(chord.analysis_growth_scroll > 0);
    for _ in 0..30 {
        page.handle_key(key(KeyCode::PageUp));
        chord.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(chord.analysis_growth_scroll, page.analysis_growth_scroll);
    }
    assert_eq!(chord.analysis_growth_scroll, 0);
}

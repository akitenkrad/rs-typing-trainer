//! MYTASK-3643: the skills tab is readable — its x axis runs over the days
//! with records (not the whole period), carries date ticks (with vertical
//! grid lines on the images), the grade bands are named with the current
//! band highlighted, components without a record take one row, and the
//! median line starts from the second run.

use std::time::Duration;

use chrono::{NaiveDate, TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Size;
use ratatui::style::Color;
use rs_typing_trainer::app::{ANALYSIS_SKILLS_TAB, AnalysisPeriod, Screen};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::menu::{DailyMenu, MenuRole, MenuStep, bundled_catalog};
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::record::db::{self, MenuStepRecord, SkillSnapshotRecord};
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::analysis_image::{SkillChartSpec, TrendAxis, render_skill_chart, skill_axis_fraction};

use crate::common;

/// «Today» is 2026-11-20; the records sit on the last `days` days.
fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 20).unwrap()
}

fn session(date: NaiveDate) -> SessionRecord {
    let config = Config::default();
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("as"));
    session.activate(Duration::ZERO);
    session.input_char(Duration::from_millis(100), "a");
    session.input_char(Duration::from_millis(250), "s");
    session.tick(Duration::from_secs(1));
    SessionRecord::from_log(&config, session.into_log(), Utc.from_utc_datetime(&date.and_hms_opt(9, 0, 0).unwrap()))
}

fn primary(id: &str, language: Lang) -> MenuStep {
    let prescription = bundled_catalog().unwrap().prescription(id).unwrap();
    let row = &prescription.levels[0];
    MenuStep {
        role: MenuRole::PrimaryA,
        language,
        prescription_id: id.to_owned(),
        title: prescription.title.text(Lang::En).to_owned(),
        level: Some(1),
        mode: row.mode.clone(),
        minutes: row.minutes,
        reason: "-".to_owned(),
        focus: String::new(),
        advance: String::new(),
        advance_condition: Some(row.advance.clone()),
        evidence: None,
        evidence_note: None,
        settings: row.settings.clone(),
    }
}

fn snapshot(run: i64, date: NaiveDate, language: &str, component: &str, value: f64) -> SkillSnapshotRecord {
    SkillSnapshotRecord {
        run_id: run,
        date,
        lang: language.to_owned(),
        component: component.to_owned(),
        raw_value: value,
        n: 100,
        source: "check".to_owned(),
        corrected: component == "speed",
    }
}

/// Japanese menu runs (speed, accuracy and rhythm) on the given days.
fn skills_app(dates: &[NaiveDate], lang: Lang, graphics: GraphicsMode) -> (tempfile::TempDir, common::TestApp) {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    for (index, &date) in dates.iter().enumerate() {
        let menu = DailyMenu {
            date,
            language: "ja".to_owned(),
            catalog_version: "menu-v1".to_owned(),
            requested_minutes: 15,
            introductory: false,
            bench_recommended: false,
            steps: vec![primary("accuracy.gate", Lang::Ja)],
        };
        let run = db::ensure_menu_run(&path, date, "qwerty", "en,ja", "menu-v1", 15, &serde_json::to_string(&menu).unwrap())
            .unwrap()
            .id;
        let session_id = db::save(&path, &session(date)).unwrap();
        db::record_menu_step(
            &path,
            &MenuStepRecord {
                run_id: run,
                idx: 1,
                role: "primary-a".to_owned(),
                prescription_id: Some("accuracy.gate".to_owned()),
                level: Some(1),
                session_id,
                metric: None,
                value: None,
                advance_met: None,
                level_after: Some(1),
            },
        )
        .unwrap();
        let shift = index as f64;
        db::replace_skill_snapshots(
            &path,
            run,
            &[
                snapshot(run, date, "ja", "speed", 33.0 + shift * 1.5),
                snapshot(run, date, "ja", "accuracy", 93.0 + shift * 0.4),
                snapshot(run, date, "ja", "rhythm", 0.30 - shift * 0.01),
            ],
        )
        .unwrap();
    }
    let mut config = Config::default();
    config.general.lang = lang;
    config.storage.db_path = Some(path);
    let mut app = common::test_app_at(config, today());
    app.set_graphics_detection(GraphicsDetection {
        mode: graphics,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: if graphics == GraphicsMode::Off {
            GraphicsReason::Disabled
        } else {
            GraphicsReason::Explicit(graphics)
        },
    });
    app.handle_key(ratatui::crossterm::event::KeyEvent::new(
        ratatui::crossterm::event::KeyCode::Char('a'),
        ratatui::crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.screen, Screen::Analysis);
    app.analysis_tab = ANALYSIS_SKILLS_TAB;
    assert_eq!(app.analysis_period, AnalysisPeriod::Days30);
    (directory, app)
}

fn last_days(count: i64) -> Vec<NaiveDate> {
    (0..count).rev().map(|back| today() - chrono::Duration::days(back)).collect()
}

fn render_buffer(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

/// Rows as strings, wide cells reduced to their first character so that the
/// character index is the column.
fn rows(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer.cell((x, y)).unwrap().symbol().chars().next().unwrap_or(' '))
                .collect()
        })
        .collect()
}

/// The screen as text (wide characters take their own width).
fn text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn compact(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

/// Prints the text tab at 80×24 and 160×50 in both languages
/// (`cargo test --test all skills_tab_readability::samples -- --nocapture`).
#[test]
fn samples() {
    for lang in [Lang::Ja, Lang::En] {
        let (_directory, app) = skills_app(&last_days(4), lang, GraphicsMode::Off);
        for (width, height) in [(80, 24), (160, 50)] {
            println!("--- {lang:?} {width}x{height}\n{}", text(&render_buffer(&app, width, height)));
        }
    }
    // With `SKILLS_DUMP` naming a directory, the image tab at 200×55 is
    // written there: `cells.tsv` (x, y, foreground, symbol of every text
    // cell) and one PNG per chart image (`image_<x>_<y>.png`).
    if let Some(directory) = std::env::var_os("SKILLS_DUMP") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        for (name, dates) in [("recent", last_days(4)), ("gap", gap_days())] {
            let (_db, mut app) = skills_app(&dates, Lang::Ja, GraphicsMode::Kitty);
            app.analysis_period = AnalysisPeriod::All;
            let buffer = render_buffer(&app, 200, 55);
            let out = directory.join(name);
            std::fs::create_dir_all(&out).unwrap();
            let mut cells = String::new();
            for y in 0..buffer.area.height {
                for x in 0..buffer.area.width {
                    let cell = &buffer[(x, y)];
                    if let Some((width, height, rgba)) = kitty_rgba(cell.symbol()) {
                        let image = image::RgbaImage::from_raw(width, height, rgba).unwrap();
                        image.save(out.join(format!("image_{x}_{y}.png"))).unwrap();
                    } else if !cell.symbol().contains('\u{1b}') {
                        cells.push_str(&format!("{x}\t{y}\t{:?}\t{}\n", cell.fg, cell.symbol()));
                    }
                }
            }
            std::fs::write(out.join("cells.tsv"), cells).unwrap();
        }
    }
}

/// Two runs in early October and four in the last days (a cut gap between).
fn gap_days() -> Vec<NaiveDate> {
    let mut dates = vec![today() - chrono::Duration::days(45), today() - chrono::Duration::days(44)];
    dates.extend(last_days(4));
    dates
}

fn decode_base64(input: &str) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut bits = 0_u32;
    let mut bit_count = 0_u8;
    for byte in input.bytes().filter(|byte| *byte != b'=') {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        bits = (bits << 6) | u32::from(value);
        bit_count += 6;
        while bit_count >= 8 {
            bit_count -= 8;
            output.push((bits >> bit_count) as u8);
            bits &= (1_u32 << bit_count).saturating_sub(1);
        }
    }
    output
}

/// The RGBA pixels of a Kitty image cell (`f=32`).
fn kitty_rgba(symbol: &str) -> Option<(u32, u32, Vec<u8>)> {
    let first = symbol.find("\u{1b}_G")?;
    let header_end = symbol[first..].find(';')? + first;
    let header = &symbol[first..header_end];
    let field = |name: &str| {
        header
            .split(',')
            .find_map(|part| part.strip_prefix(name))
            .and_then(|value| value.parse::<u32>().ok())
    };
    let width = field("s=")?;
    let height = field("v=")?;
    let mut encoded = String::new();
    let mut rest = &symbol[first..];
    while let Some(start) = rest.find("\u{1b}_G") {
        rest = &rest[start..];
        let separator = rest.find(';')?;
        let payload = &rest[separator + 1..];
        let end = payload.find("\u{1b}\\")?;
        encoded.push_str(&payload[..end]);
        rest = &payload[end + 2..];
    }
    Some((width, height, decode_base64(&encoded)))
}

/// (row, first column, last column) of the prescription band — the plot's
/// columns, since every column between the first and the last menu day is
/// covered.
fn plot_columns(buffer: &Buffer) -> (u16, u16, u16) {
    for y in 0..buffer.area.height {
        let columns = (0..buffer.area.width).filter(|&x| buffer[(x, y)].symbol() == "▀").collect::<Vec<_>>();
        if let (Some(&first), Some(&last)) = (columns.first(), columns.last()) {
            return (y, first, last);
        }
    }
    panic!("no prescription band:\n{}", text(buffer));
}

/// Column of `needle` in a row from [`rows`] (one character a column).
fn column_of(row: &str, needle: &str) -> Option<u16> {
    row.find(needle).map(|index| row[..index].chars().count() as u16)
}

/// The date row: the row with the `YYYY/MM/DD` first day.
fn date_row(buffer: &Buffer, first: NaiveDate) -> u16 {
    let needle = first.format("%Y/%m/%d").to_string();
    (0..buffer.area.height)
        .find(|&y| rows(buffer)[usize::from(y)].contains(&needle))
        .unwrap_or_else(|| panic!("no {needle}:\n{}", text(buffer)))
}

/// `MM/DD` labels on a row that are not part of a `YYYY/MM/DD` date:
/// (centre column, month, day).
fn tick_labels(row: &str) -> Vec<(u16, u32, u32)> {
    let row = row.chars().collect::<Vec<_>>();
    let mut labels = Vec::new();
    for start in 0..row.len().saturating_sub(4) {
        let slice = &row[start..start + 5];
        let digits = |range: std::ops::Range<usize>| slice[range].iter().all(char::is_ascii_digit);
        if !(digits(0..2) && slice[2] == '/' && digits(3..5)) {
            continue;
        }
        let touching = |ch: Option<char>| ch.is_some_and(|ch| ch.is_ascii_digit() || ch == '/');
        if touching(start.checked_sub(1).map(|index| row[index])) || touching(row.get(start + 5).copied()) {
            continue;
        }
        let number = |range: std::ops::Range<usize>| slice[range].iter().collect::<String>().parse::<u32>().unwrap();
        labels.push(((start + 2) as u16, number(0..2), number(3..5)));
    }
    labels
}

/// Row of the label line of `name` (the component name at the start of the
/// label column, right of the border).
fn label_row(buffer: &Buffer, name: &str) -> u16 {
    (0..buffer.area.height)
        .find(|&y| compact(&rows(buffer)[usize::from(y)]).starts_with(&format!("│{}", compact(name))))
        .unwrap_or_else(|| panic!("no row for {name}:\n{}", text(buffer)))
}

fn speed_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Ja => "速さ",
        Lang::En => "Speed",
    }
}

fn accuracy_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Ja => "正確さ",
        Lang::En => "Accuracy",
    }
}

#[test]
fn the_axis_runs_over_the_days_with_records_not_the_whole_period() {
    let dates = last_days(4);
    for lang in [Lang::Ja, Lang::En] {
        for graphics in [GraphicsMode::Off, GraphicsMode::Kitty] {
            let (_directory, app) = skills_app(&dates, lang, graphics);
            for (width, height) in [(80, 24), (160, 50)] {
                let buffer = render_buffer(&app, width, height);
                let screen = text(&buffer);
                let (_, left, right) = plot_columns(&buffer);
                let plot_width = f64::from(right - left + 1);
                // The date row: the first day at the left of the plot, the
                // last day (11/20) at its right.
                let row = date_row(&buffer, dates[0]);
                let start = column_of(&rows(&buffer)[usize::from(row)], "2026/11/17").unwrap();
                assert!(
                    start <= left + 1,
                    "{lang:?} {graphics:?} {width}x{height}: first day at {start}, plot from {left}\n{screen}"
                );
                let last = tick_labels(&rows(&buffer)[usize::from(row)])
                    .into_iter()
                    .find(|&(_, month, day)| (month, day) == (11, 20))
                    .unwrap_or_else(|| panic!("no 11/20:\n{screen}"));
                assert!(
                    f64::from(last.0 - left) >= 0.8 * plot_width,
                    "{lang:?} {graphics:?} {width}x{height}: last day at {} of {left}..{right}\n{screen}",
                    last.0
                );
                if graphics == GraphicsMode::Off {
                    // The run points of the speed chart span the plot.
                    let (top, bottom) = (label_row(&buffer, speed_name(lang)), label_row(&buffer, accuracy_name(lang)));
                    let buffer = &buffer;
                    let columns = (top..bottom)
                        .flat_map(|y| (left..=right).filter(move |&x| buffer[(x, y)].symbol() == "•"))
                        .collect::<Vec<_>>();
                    let (first, last) = (columns.iter().min().unwrap(), columns.iter().max().unwrap());
                    assert!(
                        f64::from(last - first) >= 0.8 * plot_width,
                        "{lang:?} {width}x{height}: points from {first} to {last} in {left}..{right}\n{screen}"
                    );
                }
            }
        }
    }
    // On the axis itself: 4 days of a 30-day period use almost all the width,
    // and a single day sits in the middle.
    let axis = TrendAxis::from_dates(dates.clone()).unwrap();
    assert!(skill_axis_fraction(&axis, dates[3]) - skill_axis_fraction(&axis, dates[0]) >= 0.9);
    let single = TrendAxis::from_dates([today()]).unwrap();
    assert!((skill_axis_fraction(&single, today()) - 0.5).abs() < 1e-9);
}

#[test]
fn a_single_day_is_drawn_in_the_middle_with_its_date() {
    for graphics in [GraphicsMode::Off, GraphicsMode::Kitty] {
        let (_directory, app) = skills_app(&[today()], Lang::En, graphics);
        let buffer = render_buffer(&app, 80, 24);
        let screen = text(&buffer);
        assert!(screen.contains("2026/11/20"), "{screen}");
        let (_, left, right) = plot_columns(&buffer);
        assert!(right - left > 30, "{screen}");
    }
}

#[test]
fn date_ticks_are_labelled_under_the_charts() {
    let dates = last_days(4);
    for lang in [Lang::Ja, Lang::En] {
        for graphics in [GraphicsMode::Off, GraphicsMode::Kitty] {
            let (_directory, app) = skills_app(&dates, lang, graphics);
            for (width, height) in [(80, 24), (160, 50)] {
                let buffer = render_buffer(&app, width, height);
                let row = date_row(&buffer, dates[0]);
                let ticks = tick_labels(&rows(&buffer)[usize::from(row)])
                    .into_iter()
                    .filter(|&(_, month, day)| (month, day) != (11, 20))
                    .collect::<Vec<_>>();
                assert!(!ticks.is_empty(), "{lang:?} {graphics:?} {width}x{height}: no tick\n{}", text(&buffer));
                // The date row is the last row above the key hints.
                assert!(row > label_row(&buffer, speed_name(lang)));
            }
        }
    }
}

#[test]
fn long_gaps_are_cut_and_marked() {
    let (_directory, mut app) = skills_app(&gap_days(), Lang::En, GraphicsMode::Off);
    app.analysis_period = AnalysisPeriod::All;
    let buffer = render_buffer(&app, 80, 24);
    let screen = text(&buffer);
    let row = date_row(&buffer, gap_days()[0]);
    assert!(rows(&buffer)[usize::from(row)].contains('≈'), "{screen}");
}

/// The speed chart image of the screen: (first column, row, RGBA image).
fn speed_image(buffer: &Buffer, lang: Lang) -> (u16, u16, image::RgbaImage) {
    let y = label_row(buffer, speed_name(lang));
    (0..buffer.area.width)
        .find_map(|x| {
            kitty_rgba(buffer[(x, y)].symbol()).map(|(width, height, rgba)| (x, y, image::RgbaImage::from_raw(width, height, rgba).unwrap()))
        })
        .expect("speed chart image")
}

#[test]
fn image_charts_draw_a_vertical_grid_line_at_each_labelled_tick() {
    let dates = last_days(4);
    let (_directory, app) = skills_app(&dates, Lang::Ja, GraphicsMode::Kitty);
    let buffer = render_buffer(&app, 160, 50);
    let (_, image) = {
        let (x, _, image) = speed_image(&buffer, Lang::Ja);
        (x, image)
    };
    let axis = TrendAxis::from_dates(dates.clone()).unwrap();
    let row = date_row(&buffer, dates[0]);
    let ticks = tick_labels(&rows(&buffer)[usize::from(row)])
        .into_iter()
        .filter(|&(_, month, day)| (month, day) != (11, 20))
        .collect::<Vec<_>>();
    assert!(!ticks.is_empty());
    for (_, month, day) in ticks {
        let date = NaiveDate::from_ymd_opt(2026, month, day).unwrap();
        let (left, right) = (2.0, f64::from(image.width()) - 3.0);
        let x = (left + skill_axis_fraction(&axis, date) * (right - left)).floor() as u32;
        let changed = (0..image.height())
            .filter(|&y| image.get_pixel(x, y) != image.get_pixel(x + 4, y))
            .count() as u32;
        assert!(changed * 2 > image.height(), "{month:02}/{day:02}: {changed} of {} rows", image.height());
    }

    // The same on the chart alone: ticks add a faint line and nothing else.
    let mut spec = chart_spec();
    let plain = render_skill_chart(CellPixels::new(4, 4), &spec).unwrap();
    spec.x_ticks = vec![day(5)];
    let lined = render_skill_chart(CellPixels::new(4, 4), &spec).unwrap();
    let (left, right) = (2.0, f64::from(lined.width()) - 3.0);
    let x = (left + skill_axis_fraction(&spec.x_axis, day(5)) * (right - left)).floor() as u32;
    let changed = (0..lined.height()).filter(|&y| lined.get_pixel(x, y) != plain.get_pixel(x, y)).count() as u32;
    assert!(changed * 2 > lined.height(), "{changed}");
    let far = (left + skill_axis_fraction(&spec.x_axis, day(2)) * (right - left)).floor() as u32 + 3;
    assert!((0..lined.height()).all(|y| lined.get_pixel(far, y) == plain.get_pixel(far, y)));
}

fn day(offset: i64) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 1, 1).unwrap() + chrono::Duration::days(offset)
}

/// Bands every 10 from 0 (G) to 80, the view from 25 to 55 (bands E, D, C
/// and B in view), the current band D (index 3).
fn chart_spec() -> SkillChartSpec {
    SkillChartSpec {
        cells: Size::new(40, 6),
        x_axis: TrendAxis::from_dates((0..10).map(day)).unwrap(),
        x_ticks: Vec::new(),
        y_range: (25.0, 55.0),
        points: Vec::new(),
        medians: Vec::new(),
        bench: Vec::new(),
        boundaries: (0..9).map(|index| f64::from(index) * 10.0).collect(),
        current_band: Some(3),
        current_color: Color::Rgb(220, 40, 40),
        point_color: Color::White,
        line_color: Color::LightCyan,
        bench_color: Color::LightYellow,
    }
}

#[test]
fn bands_are_named_and_the_current_band_is_highlighted() {
    // On the screen: the speed chart (33.0 … 37.5 WPM, grade E) names its
    // band with the value that enters it, and the next grade's threshold.
    for (lang, graphics) in [
        (Lang::Ja, GraphicsMode::Off),
        (Lang::En, GraphicsMode::Off),
        (Lang::Ja, GraphicsMode::Kitty),
    ] {
        let (_directory, app) = skills_app(&last_days(4), lang, graphics);
        let buffer = render_buffer(&app, 80, 24);
        let screen = text(&buffer);
        for expected in ["E≥32.0", "D≥42.0", "F≥22.0"] {
            assert!(compact(&screen).contains(expected), "{lang:?} {graphics:?} {expected}:\n{screen}");
        }
        // The current band's name is drawn in its grade colour, the others not.
        let (row, column) = (0..buffer.area.height)
            .find_map(|y| column_of(&rows(&buffer)[usize::from(y)], "E≥32.0").map(|x| (y, x)))
            .unwrap();
        let other = (0..buffer.area.height)
            .find_map(|y| column_of(&rows(&buffer)[usize::from(y)], "D≥42.0").map(|x| (y, x)))
            .unwrap();
        assert_ne!(buffer[(column, row)].fg, buffer[(other.1, other.0)].fg, "{screen}");
        // Lower-is-better components say so (rhythm: 0.30 … 0.27).
        assert!(compact(&screen).contains("S≤0.300"), "{screen}");
    }

    // On the image: the current band is tinted with its colour, the others grey.
    let spec = chart_spec();
    let image = render_skill_chart(CellPixels::new(9, 20), &spec).unwrap();
    let height = f64::from(image.height());
    let y_of = |value: f64| ((2.0 + (55.0 - value) / 30.0 * (height - 5.0)) as u32).min(image.height() - 1);
    let x = image.width() / 2;
    let current = image.get_pixel(x, y_of(35.0)).0;
    let other = image.get_pixel(x, y_of(45.0)).0;
    assert!(current[3] > 0 && other[3] > 0, "{current:?} {other:?}");
    assert!(current[0] > current[1] + 40, "current band is tinted: {current:?}");
    assert!(other[0].abs_diff(other[1]) < 20, "other bands stay grey: {other:?}");
    let unlit = render_skill_chart(
        CellPixels::new(9, 20),
        &SkillChartSpec {
            current_band: None,
            ..chart_spec()
        },
    )
    .unwrap();
    assert_ne!(unlit.get_pixel(x, y_of(35.0)).0, current);
}

#[test]
fn components_without_records_take_one_row() {
    for (lang, name, none) in [(Lang::Ja, "遷移", "記録なし"), (Lang::En, "Transitions", "no records")] {
        let (_directory, app) = skills_app(&last_days(4), lang, GraphicsMode::Off);
        for (width, height) in [(80, 24), (160, 50)] {
            let buffer = render_buffer(&app, width, height);
            let screen = rows(&buffer);
            let row = usize::from(label_row(&buffer, name));
            assert!(compact(&screen[row]).contains(&compact(none)), "{}", text(&buffer));
            // The next row is already the next component (preview), also empty.
            let next = match lang {
                Lang::Ja => "先読み",
                Lang::En => "Preview",
            };
            assert!(compact(&screen[row + 1]).starts_with(&format!("│{next}")), "{}", text(&buffer));
            assert!(!compact(&text(&buffer)).contains("中央値—"), "{}", text(&buffer));
        }
    }
}

#[test]
fn label_says_how_far_the_next_grade_is_with_the_unit() {
    let (_directory, app) = skills_app(&last_days(4), Lang::Ja, GraphicsMode::Off);
    let screen = compact(&text(&render_buffer(&app, 80, 24)));
    // Speed 37.5 WPM, grade E; D starts at 42.0.
    assert!(screen.contains("速さE37.5WPM"), "{screen}");
    assert!(screen.contains("次の段Dまで+4.5WPM"), "{screen}");
    assert!(screen.contains("最上位の段"), "{screen}");
    let (_directory, app) = skills_app(&last_days(4), Lang::En, GraphicsMode::Off);
    let screen = compact(&text(&render_buffer(&app, 80, 24)));
    assert!(screen.contains("+4.5WPMtoD"), "{screen}");
    assert!(screen.contains("topgrade"), "{screen}");
}

#[test]
fn the_median_line_is_drawn_from_the_second_run() {
    let dates = last_days(2);
    // Text: the accent-coloured braille line of the speed chart.
    let (_directory, app) = skills_app(&dates, Lang::En, GraphicsMode::Off);
    let buffer = render_buffer(&app, 80, 24);
    let (top, bottom) = (label_row(&buffer, "Speed"), label_row(&buffer, "Accuracy"));
    let (_, left, right) = plot_columns(&buffer);
    let buffer_ref = &buffer;
    let line = (top..bottom)
        .flat_map(|y| (left..=right).map(move |x| (x, y)))
        .filter(|&(x, y)| buffer_ref[(x, y)].symbol() != " ")
        .filter(|&(x, y)| {
            let cell = &buffer[(x, y)];
            cell.fg == Color::LightCyan && cell.symbol().chars().next().is_some_and(|ch| ('\u{2801}'..='\u{28FF}').contains(&ch))
        })
        .count();
    assert!(line > 10, "{line} line cells:\n{}", text(&buffer));
    assert!(compact(&text(&buffer)).contains("median"), "{}", text(&buffer));

    // Image: a thick line in the line colour across the plot.
    let (_directory, app) = skills_app(&dates, Lang::En, GraphicsMode::Kitty);
    let buffer = render_buffer(&app, 80, 24);
    let (_, _, image) = speed_image(&buffer, Lang::En);
    let columns = (0..image.width())
        .filter(|&x| {
            (0..image.height()).any(|y| {
                let pixel = image.get_pixel(x, y).0;
                pixel[3] == 255 && pixel[0].abs_diff(41) < 30 && pixel[1].abs_diff(184) < 30 && pixel[2].abs_diff(219) < 30
            })
        })
        .count() as u32;
    assert!(columns * 10 > image.width() * 8, "line over {columns} of {} columns", image.width());
}

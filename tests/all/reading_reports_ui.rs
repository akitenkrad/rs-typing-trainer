use chrono::{TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::reading_report::{ReadingReportKind, append_reading_report, save_reading_correction};
use rs_typing_trainer::ui;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::common;
use crate::common::TestApp;

const LONG_TEXT: &str = "十分に長い日本語の表記を一覧では省略して全文欄の末尾まで表示";
const LONG_READING: &str = "じゅうぶんにながいにほんごのひょうきをいちらんではしょうりゃくしてぜんぶんらんのまつびまですべてひょうじ";
const LONG_CORRECTED: &str = "じゅうぶんにながいしゅうせいごのよみをいちらんではしょうりゃくしてひょうじ";

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn report(app: &TestApp, hour: u32, kind: ReadingReportKind, text: &str, reading: &str) {
    let db_path = app.db_path();
    let directory = db_path.parent().unwrap();
    append_reading_report(directory, Utc.with_ymd_and_hms(2026, 11, 14, hour, 30, 0).unwrap(), kind, text, reading).unwrap();
}

fn correction(app: &TestApp, kind: ReadingReportKind, text: &str, wrong: &str, correct: &str) {
    let db_path = app.db_path();
    let directory = db_path.parent().unwrap();
    save_reading_correction(
        directory,
        Utc.with_ymd_and_hms(2026, 11, 14, 12, 30, 0).unwrap(),
        kind,
        text,
        wrong,
        correct,
    )
    .unwrap();
}

fn open_reading_reports(app: &mut TestApp) {
    app.handle_key(key(KeyCode::Char('h')));
    assert_eq!(app.screen, Screen::History);
    app.handle_key(key(KeyCode::Char('r')));
    assert_eq!(app.screen, Screen::ReadingReports);
}

fn render_buffer(app: &TestApp, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn buffer_text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn render(app: &TestApp, width: u16, height: u16) -> String {
    buffer_text(&render_buffer(app, width, height))
}

fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect()
}

fn row_y(buffer: &Buffer, row_text: &str) -> u16 {
    (0..buffer.area.height)
        .find(|&y| contains_display(&row(buffer, y), row_text))
        .unwrap_or_else(|| panic!("missing row containing {row_text:?}"))
}

fn text_x(buffer: &Buffer, y: u16, text: &str) -> u16 {
    (0..buffer.area.width)
        .find(|&start| {
            let mut x = start;
            text.chars().all(|character| {
                if x >= buffer.area.width || buffer[(x, y)].symbol() != character.to_string() {
                    return false;
                }
                x = x.saturating_add(u16::try_from(UnicodeWidthChar::width(character).unwrap_or(0).max(1)).unwrap());
                true
            })
        })
        .unwrap_or_else(|| panic!("missing {text:?} on row {y}: {:?}", row(buffer, y)))
}

fn display_span(buffer: &Buffer, y: u16, start: u16, end: u16) -> String {
    (start..end).map(|x| buffer[(x, y)].symbol()).collect()
}

fn contains_display(output: &str, expected: &str) -> bool {
    output.contains(expected) || output.replace(' ', "").contains(&expected.replace(' ', ""))
}

fn long_report_app(lang: Lang) -> TestApp {
    let mut config = Config::default();
    config.general.lang = lang;
    let mut app = common::test_app(config);
    report(&app, 9, ReadingReportKind::Romaji, "十分", "じゅうふん");
    report(&app, 10, ReadingReportKind::Romaji, "十分", "じゅうふん");
    report(&app, 11, ReadingReportKind::Sentence, LONG_TEXT, LONG_READING);
    open_reading_reports(&mut app);
    app
}

fn find_cell_in_row(buffer: &Buffer, row_text: &str, symbol: &str) -> (u16, u16) {
    for y in 0..buffer.area.height {
        let rendered_row = row(buffer, y);
        if contains_display(&rendered_row, row_text) {
            for x in 0..buffer.area.width {
                if buffer[(x, y)].symbol() == symbol {
                    return (x, y);
                }
            }
        }
    }
    panic!("missing {symbol:?} in row containing {row_text:?}");
}

#[test]
fn history_r_opens_reading_reports_and_escape_returns_to_history() {
    let mut app = common::test_app(Config::default());
    app.handle_key(key(KeyCode::Char('h')));
    assert_eq!(app.screen, Screen::History);

    app.handle_key(key(KeyCode::Char('r')));
    assert_eq!(app.screen, Screen::ReadingReports);

    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.screen, Screen::History);
}

#[test]
fn empty_reading_reports_explain_how_to_report_in_both_languages() {
    for (lang, expected) in [(Lang::Ja, "まだ報告はありません"), (Lang::En, "There are no reading reports yet")] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut app = common::test_app(config);
        open_reading_reports(&mut app);

        let output = render(&app, 120, 40);
        assert!(contains_display(&output, expected), "missing {expected:?}:\n{output}");
        assert!(output.contains("Ctrl+R"), "missing report shortcut:\n{output}");
        assert!(output.contains("F2"), "missing report shortcut:\n{output}");
    }
}

#[test]
fn three_reports_render_at_narrow_and_wide_sizes_in_both_languages() {
    for lang in [Lang::Ja, Lang::En] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut app = common::test_app(config);
        report(&app, 9, ReadingReportKind::Romaji, "十分", "じゅうぶん");
        report(&app, 10, ReadingReportKind::Sentence, "今日は晴れです。", "きょうははれです。");
        report(&app, 11, ReadingReportKind::Romaji, LONG_TEXT, LONG_READING);
        report(&app, 12, ReadingReportKind::Romaji, LONG_TEXT, LONG_READING);
        open_reading_reports(&mut app);

        for (width, height) in [(80, 24), (120, 40)] {
            let buffer = render_buffer(&app, width, height);
            let output = buffer_text(&buffer);
            let expected_summary = if lang == Lang::Ja { "報告 3 件" } else { "3 reports" };
            let expected_word = if lang == Lang::Ja { "単語" } else { "Word" };
            let expected_sentence = if lang == Lang::Ja { "短文" } else { "Sentence" };
            assert!(contains_display(&output, expected_summary), "{width}x{height}:\n{output}");
            assert!(contains_display(&output, expected_word), "{width}x{height}:\n{output}");
            assert!(contains_display(&output, expected_sentence), "{width}x{height}:\n{output}");
            assert!(output.contains("×2"), "{width}x{height}:\n{output}");
            assert!(
                contains_display(&output, "十分に長い日本語の表記"),
                "selected full text missing:\n{output}"
            );
            assert!(
                contains_display(&output, "じゅうぶんにながいにほんご"),
                "selected full reading missing:\n{output}"
            );
            assert!(output.contains('…'), "narrowable columns should use an ellipsis:\n{output}");
        }

        let buffer = render_buffer(&app, 80, 24);
        let hint = row(&buffer, 22);
        let expected_hint = if lang == Lang::Ja { "[↑/↓] 選ぶ" } else { "[↑/↓] Select" };
        assert!(contains_display(&hint, expected_hint), "bottom hint row was {hint:?}");
    }
}

#[test]
fn confirmation_replaces_the_bottom_hint_with_visible_y_and_n_actions() {
    for (lang, remove, cancel) in [(Lang::Ja, "[Y] 取り消す", "[N] やめる"), (Lang::En, "[Y] Remove", "[N] Cancel")] {
        let mut app = long_report_app(lang);
        app.handle_key(key(KeyCode::Char('d')));

        let buffer = render_buffer(&app, 80, 24);
        let hint = row(&buffer, 22);
        assert!(contains_display(&hint, remove), "bottom hint row was {hint:?}");
        assert!(contains_display(&hint, cancel), "bottom hint row was {hint:?}");
    }
}

#[test]
fn header_and_data_columns_start_at_the_same_cells() {
    let app = long_report_app(Lang::Ja);
    let buffer = render_buffer(&app, 80, 24);

    let (header_x, _) = find_cell_in_row(&buffer, "日時", "日");
    let (data_x, _) = find_cell_in_row(&buffer, "2026-", "2");

    assert_eq!(header_x, data_x);
}

#[test]
fn corrected_column_shows_saved_reading_or_dash_with_localized_headers() {
    for (lang, date_header, corrected_header) in [(Lang::Ja, "日時", "修正済み"), (Lang::En, "Date", "Corrected")] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut app = common::test_app(config);
        report(&app, 9, ReadingReportKind::Romaji, "十分", "じゅうふん");
        report(&app, 10, ReadingReportKind::Romaji, "遠足", "えんそく");
        correction(&app, ReadingReportKind::Romaji, "十分", "じゅうふん", "じゅうぶん");
        open_reading_reports(&mut app);

        let buffer = render_buffer(&app, 120, 40);
        let header_y = row_y(&buffer, date_header);
        let corrected_x = text_x(&buffer, header_y, corrected_header);
        let corrected_y = row_y(&buffer, "十分");
        let uncorrected_y = row_y(&buffer, "遠足");

        assert_eq!(text_x(&buffer, corrected_y, "じゅうぶん"), corrected_x);
        assert_eq!(buffer[(corrected_x, uncorrected_y)].symbol(), "-");
    }
}

#[test]
fn headers_and_data_share_content_sized_column_starts_at_wide_sizes() {
    for (lang, headers, kind) in [
        (Lang::Ja, ["日時", "種類", "表記", "読み", "修正済み"], "短文"),
        (Lang::En, ["Date", "Type", "Text", "Reading", "Corrected"], "Sentence"),
    ] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut app = common::test_app(config);
        report(&app, 11, ReadingReportKind::Sentence, "希少表記", "きしょうよみ");
        correction(&app, ReadingReportKind::Sentence, "希少表記", "きしょうよみ", "ただしいよみ");
        open_reading_reports(&mut app);

        for (width, height) in [(120, 40), (200, 50)] {
            let buffer = render_buffer(&app, width, height);
            let header_y = row_y(&buffer, headers[0]);
            let data_y = row_y(&buffer, "希少表記");
            let header_starts = headers.map(|header| text_x(&buffer, header_y, header));
            let data_starts = [
                text_x(&buffer, data_y, "2026-"),
                text_x(&buffer, data_y, kind),
                text_x(&buffer, data_y, "希少表記"),
                text_x(&buffer, data_y, "きしょうよみ"),
                text_x(&buffer, data_y, "ただしいよみ"),
            ];

            assert_eq!(header_starts, data_starts, "{width}x{height}");

            if width == 200 {
                let expected_reading_x = header_starts[0]
                    + 16
                    + 2
                    + u16::try_from(UnicodeWidthStr::width(kind).max(UnicodeWidthStr::width(headers[1]))).unwrap()
                    + 2
                    + u16::try_from(UnicodeWidthStr::width("希少表記").max(UnicodeWidthStr::width(headers[2]))).unwrap()
                    + 2;
                assert_eq!(header_starts[3], expected_reading_x);
            }
        }
    }
}

#[test]
fn narrow_list_truncates_all_variable_columns_without_cutting_date_kind_or_hint() {
    for (lang, headers, kind, hint) in [
        (Lang::Ja, ["日時", "種類", "表記", "読み", "修正済み"], "短文", "[↑/↓] 選ぶ"),
        (Lang::En, ["Date", "Type", "Text", "Reading", "Corrected"], "Sentence", "[↑/↓] Select"),
    ] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut app = common::test_app(config);
        report(&app, 11, ReadingReportKind::Sentence, LONG_TEXT, LONG_READING);
        correction(&app, ReadingReportKind::Sentence, LONG_TEXT, LONG_READING, LONG_CORRECTED);
        open_reading_reports(&mut app);

        let buffer = render_buffer(&app, 80, 24);
        let header_y = row_y(&buffer, headers[0]);
        let data_y = row_y(&buffer, "2026-");
        let starts = headers.map(|header| text_x(&buffer, header_y, header));
        let data_row = row(&buffer, data_y);

        assert!(contains_display(&data_row, "2026-11-14"), "date was cut: {data_row:?}");
        assert!(contains_display(&data_row, kind), "kind was cut: {data_row:?}");
        assert!(display_span(&buffer, data_y, starts[2], starts[3] - 2).contains('…'));
        assert!(display_span(&buffer, data_y, starts[3], starts[4] - 2).contains('…'));
        assert!(display_span(&buffer, data_y, starts[4], buffer.area.width).contains('…'));
        assert!(contains_display(&row(&buffer, 22), hint));
    }
}

#[test]
fn selected_long_text_and_reading_show_their_ends_at_both_sizes() {
    let app = long_report_app(Lang::Ja);

    for (width, height) in [(80, 24), (120, 40)] {
        let output = render(&app, width, height);
        assert!(contains_display(&output, "全文欄の末尾まで表示"), "{width}x{height}:\n{output}");
        assert!(contains_display(&output, "まつびまですべてひょうじ"), "{width}x{height}:\n{output}");
    }
}

#[test]
fn delete_confirmation_ignores_other_keys_then_n_cancels_and_y_removes_the_group() {
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    let mut app = common::test_app(config);
    report(&app, 9, ReadingReportKind::Sentence, "今日は晴れです。", "きょうははれです。");
    report(&app, 10, ReadingReportKind::Romaji, "十分", "じゅうふん");
    report(&app, 11, ReadingReportKind::Romaji, "十分", "じゅうふん");
    open_reading_reports(&mut app);
    let path = app.db_path().parent().unwrap().join("reading_reports.tsv");
    let before = std::fs::read(&path).unwrap();

    app.handle_key(key(KeyCode::Char('d')));
    let confirmation = render(&app, 120, 40);
    assert!(contains_display(&confirmation, "この報告を取り消しますか？"), "{confirmation}");
    assert!(
        contains_display(&confirmation, "次に練習を始めたときから再び出題されます"),
        "{confirmation}"
    );

    app.handle_key(key(KeyCode::Char('j')));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(contains_display(&render(&app, 120, 40), "取り消しますか？"));

    app.handle_key(key(KeyCode::Char('n')));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!contains_display(&render(&app, 120, 40), "取り消しますか？"));

    app.handle_key(key(KeyCode::Delete));
    app.handle_key(key(KeyCode::Char('y')));

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(!after.contains("十分\tじゅうふん"), "{after:?}");
    assert!(after.contains("今日は晴れです。\tきょうははれです。"), "{after:?}");
    let output = render(&app, 120, 40);
    assert!(contains_display(&output, "報告 1 件"), "{output}");
    assert!(contains_display(&output, "取り消しました．"), "{output}");
    assert!(contains_display(&output, "次に練習を始めたときから再び出題されます"), "{output}");
}

#[test]
fn english_confirmation_and_removal_notice_use_fixed_visible_messages() {
    let mut app = long_report_app(Lang::En);

    app.handle_key(key(KeyCode::Char('d')));
    let confirmation = render(&app, 80, 24);
    assert!(contains_display(&confirmation, "Remove this report?"), "{confirmation}");
    assert!(contains_display(&confirmation, "It will appear in prompts again"), "{confirmation}");
    assert!(contains_display(&confirmation, "practicing."), "{confirmation}");

    app.handle_key(key(KeyCode::Char('y')));
    let notice = render(&app, 80, 24);
    assert!(contains_display(&notice, "Removed."), "{notice}");
    assert!(contains_display(&notice, "It will appear in prompts again"), "{notice}");
    assert!(contains_display(&notice, "practicing."), "{notice}");
    assert!(!notice.contains(LONG_TEXT), "notice must not repeat the removed text: {notice}");
}

#[test]
fn reading_report_load_errors_are_rendered_without_panicking() {
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    let mut app = common::test_app(config);
    let path = app.db_path().parent().unwrap().join("reading_reports.tsv");
    std::fs::create_dir_all(&path).unwrap();

    open_reading_reports(&mut app);

    let output = render(&app, 80, 24);
    assert!(contains_display(&output, "読みの報告を読み込めませんでした"), "{output}");
}

#[test]
fn e_edits_and_saves_a_correct_reading_then_marks_the_report_corrected() {
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    let mut app = common::test_app(config);
    report(&app, 9, ReadingReportKind::Romaji, "十分", "じゅうふん");
    open_reading_reports(&mut app);

    app.handle_key(key(KeyCode::Char('e')));
    let editing = render(&app, 80, 24);
    assert!(contains_display(&editing, "正しい読み: [じゅうふん▏]"), "{editing}");
    app.handle_key(key(KeyCode::Backspace));
    app.handle_key(key(KeyCode::Backspace));
    app.handle_key(key(KeyCode::Char('ぶ')));
    app.handle_key(key(KeyCode::Char('ん')));
    app.handle_key(key(KeyCode::Enter));

    let source = std::fs::read_to_string(app.db_path().parent().unwrap().join("reading_corrections.tsv")).unwrap();
    assert!(source.contains("\tromaji\t十分\tじゅうふん\tじゅうぶん\n"), "{source:?}");
    let output = render(&app, 80, 24);
    assert!(contains_display(&output, "修正済み → じゅうぶん"), "{output}");
    assert!(contains_display(&output, "うち修正済み 1 件"), "{output}");
    assert!(
        contains_display(&output, "保存しました．次に練習を始めたときから")
            && contains_display(&output, "じゅうぶん")
            && contains_display(&output, "で出題されます．"),
        "{output}"
    );
}

#[test]
fn enter_opens_editing_escape_cancels_and_d_is_input_while_editing() {
    let mut app = common::test_app(Config::default());
    report(&app, 9, ReadingReportKind::Romaji, "十分", "じゅうふん");
    open_reading_reports(&mut app);
    let path = app.db_path().parent().unwrap().join("reading_corrections.tsv");

    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('D')));
    let output = render(&app, 80, 24);
    assert!(contains_display(&output, "Correct reading: [じゅうふんD▏]"), "{output}");
    assert!(!app.reading_reports_confirming);
    app.handle_key(key(KeyCode::Esc));

    assert!(!path.exists());
    assert!(!contains_display(&render(&app, 80, 24), "Correct reading:"));
}

#[test]
fn editing_hint_stays_on_the_bottom_row_and_invalid_input_shows_a_reason_in_both_languages() {
    for (lang, save, cancel, reason) in [
        (Lang::Ja, "[Enter] 保存", "[Esc] やめる", "ひらがなで入力してください"),
        (Lang::En, "[Enter] Save", "[Esc] Cancel", "Enter hiragana"),
    ] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut app = common::test_app(config);
        report(&app, 9, ReadingReportKind::Romaji, "十分", "じゅうふん");
        open_reading_reports(&mut app);
        app.handle_key(key(KeyCode::Char('e')));
        app.handle_key(key(KeyCode::Char('D')));
        app.handle_key(key(KeyCode::Enter));

        let buffer = render_buffer(&app, 80, 24);
        let output = buffer_text(&buffer);
        let hint = row(&buffer, 22);
        assert!(contains_display(&hint, save), "bottom hint row was {hint:?}");
        assert!(contains_display(&hint, cancel), "bottom hint row was {hint:?}");
        assert!(contains_display(&output, reason), "{output}");
        assert!(
            contains_display(
                &output,
                if lang == Lang::Ja {
                    "直した読みは手元にだけ保存されます"
                } else {
                    "Corrected readings are saved only on this device"
                }
            ),
            "{output}"
        );
    }
}

#[test]
fn corrected_report_confirmation_mentions_and_removes_the_saved_correction() {
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    let mut app = common::test_app(config);
    report(&app, 9, ReadingReportKind::Romaji, "十分", "じゅうふん");
    correction(&app, ReadingReportKind::Romaji, "十分", "じゅうふん", "じゅうぶん");
    open_reading_reports(&mut app);

    app.handle_key(key(KeyCode::Char('d')));
    let confirmation = render(&app, 80, 24);
    assert!(contains_display(&confirmation, "正しい読みの登録も消えます"), "{confirmation}");
    app.handle_key(key(KeyCode::Char('y')));

    assert!(
        std::fs::read_to_string(app.db_path().parent().unwrap().join("reading_corrections.tsv"))
            .unwrap()
            .is_empty()
    );
}

fn ctrl(character: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
}

fn many_reports_app() -> TestApp {
    let mut app = common::test_app(Config::default());
    for hour in 0..24 {
        let text = format!("語{hour:02}");
        report(&app, hour, ReadingReportKind::Romaji, &text, "ご");
    }
    open_reading_reports(&mut app);
    app
}

fn reading_selection_after(start: usize, event: KeyEvent) -> usize {
    let mut app = many_reports_app();
    assert!(app.reading_reports.len() > 20, "{}", app.reading_reports.len());
    app.reading_reports_selected = start;
    app.handle_key(event);
    assert_eq!(app.screen, Screen::ReadingReports);
    assert!(!app.reading_reports_confirming);
    app.reading_reports_selected
}

#[test]
fn reading_reports_less_and_vi_keys_move_like_home_end_and_page_keys() {
    let last = many_reports_app().reading_reports.len() - 1;
    assert_eq!(reading_selection_after(12, key(KeyCode::Char('<'))), 0);
    assert_eq!(reading_selection_after(12, key(KeyCode::Char('>'))), last);
    assert_eq!(
        reading_selection_after(12, key(KeyCode::Char('>'))),
        reading_selection_after(12, key(KeyCode::End))
    );
    for start in [0, 4, 12, last - 3, last] {
        assert_eq!(
            reading_selection_after(start, ctrl('u')),
            reading_selection_after(start, key(KeyCode::PageUp)),
            "{start}"
        );
        assert_eq!(
            reading_selection_after(start, ctrl('d')),
            reading_selection_after(start, key(KeyCode::PageDown)),
            "{start}"
        );
    }
    assert_eq!(reading_selection_after(1, ctrl('d')), 11);
}

#[test]
fn reading_reports_ctrl_d_does_not_ask_to_remove() {
    let mut app = many_reports_app();
    let count = app.reading_reports.len();
    app.handle_key(ctrl('d'));
    app.handle_key(ctrl('D'));
    assert!(!app.reading_reports_confirming);
    assert_eq!(app.screen, Screen::ReadingReports);
    app.handle_key(key(KeyCode::Char('y')));
    assert_eq!(app.reading_reports.len(), count);

    // Plain `d` still asks.
    app.handle_key(key(KeyCode::Char('d')));
    assert!(app.reading_reports_confirming);
}

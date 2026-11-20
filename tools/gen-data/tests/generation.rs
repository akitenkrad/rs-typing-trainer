use std::collections::BTreeSet;

use gen_data::{
    BizUdGenerationReport, FontGenerationOptions, FontKind, OutlineCommand, OutlineFont, OutlineGlyph, aozora_works_from_selection,
    decode_outline_font, encode_outline_font, extract_aozora, extract_english_body, generate_fonts, gzip_deterministic, jis_x0208_to_char,
    load_scowl_words, parse_bdf, prepare_english_phrase, prepare_english_sentence, prepare_japanese_phrases, render_aozora_works, stable_sample,
    update_bizud_manifest, update_bizud_notice, update_shinonome_manifest, update_shinonome_notice,
};
use gen_data::{
    JA_PASSAGE_MAX_CHARS, JA_PASSAGES_FILTER, JA_PASSAGES_PER_WORK, PassageSentence, chunk_run, merge_passages, render_ja_passages,
    select_work_passages, update_passage_manifest,
};
use tempfile::TempDir;

const HALF_12: &str = r#"STARTFONT 2.1
FONTBOUNDINGBOX 6 12 0 -2
CHARS 2
STARTCHAR SPACE
ENCODING 32
BBX 6 12 0 -2
BITMAP
00
00
00
00
00
00
00
00
00
00
00
00
ENDCHAR
STARTCHAR A
ENCODING 65
BBX 6 12 0 -2
BITMAP
00
20
50
50
88
F8
88
88
00
00
00
00
ENDCHAR
ENDFONT
"#;

const FULL_12: &str = r#"STARTFONT 2.1
FONTBOUNDINGBOX 12 12 0 -2
CHARS 2
STARTCHAR 2422
ENCODING 9250
BBX 12 12 0 -2
BITMAP
0000
0400
0400
7FC0
0440
1C80
2680
4500
4600
3C00
0000
0000
ENDCHAR
STARTCHAR 3441
ENCODING 13377
BBX 12 12 0 -2
BITMAP
2440
FFC0
2440
7FC0
4440
7FC0
4440
7FC0
0440
0840
1040
0000
ENDCHAR
ENDFONT
"#;

fn blank_bdf(width: u8, height: u8, encodings: &[u32]) -> String {
    let row = "0".repeat(usize::from(width).div_ceil(8) * 2);
    let glyphs = encodings
        .iter()
        .map(|encoding| {
            format!(
                "STARTCHAR {encoding:X}\nENCODING {encoding}\nBBX {width} {height} 0 -2\nBITMAP\n{}\nENDCHAR\n",
                std::iter::repeat_n(row.as_str(), usize::from(height)).collect::<Vec<_>>().join("\n")
            )
        })
        .collect::<String>();
    format!(
        "STARTFONT 2.1\nFONTBOUNDINGBOX {width} {height} 0 -2\nCHARS {}\n{glyphs}ENDFONT\n",
        encodings.len()
    )
}

#[test]
fn english_body_is_bounded_by_distribution_markers() {
    let source = "header\n*** START OF THE EBOOK SAMPLE ***\nAlpha beta.\n*** END OF THE EBOOK SAMPLE ***\nfooter\n";

    assert_eq!(extract_english_body(source).unwrap(), "Alpha beta.\n");
}

#[test]
fn aozora_extraction_removes_header_legend_notes_and_colophon() {
    let source = "作品名\r\n著者名\r\n\r\n--------------------------------\r\n凡例《はんれい》\r\n--------------------------------\r\n\r\n　親譲《おやゆず》り［＃注記］の本文。\r\n底本：「本」\r\n入力：入力者\r\n";

    let document = extract_aozora(source);

    assert_eq!(document.body, "親譲りの本文。");
    assert_eq!(document.rubies.get("親譲"), Some(&BTreeSet::from(["おやゆず".to_owned()])));
}

#[test]
fn aozora_explicit_ruby_marker_limits_the_parent_text() {
    let source = "題\n著者\n\n本文は夕方｜折戸《おりど》の蔭《かげ》にある。\n底本：本\n";

    let document = extract_aozora(source);

    assert_eq!(document.body, "本文は夕方折戸の蔭にある。");
    assert_eq!(document.rubies.get("折戸"), Some(&BTreeSet::from(["おりど".to_owned()])));
    assert_eq!(document.rubies.get("蔭"), Some(&BTreeSet::from(["かげ".to_owned()])));
}

#[test]
fn aozora_ruby_offsets_follow_trimmed_body_text() {
    let source = "題\n著者\n\n　家《いえ》の外《そと》に出る。\n底本：本\n";

    let document = extract_aozora(source);

    assert_eq!(document.body, "家の外に出る。");
    assert_eq!((document.ruby_spans[0].start, document.ruby_spans[0].end), (0, "家".len()));
    assert_eq!((document.ruby_spans[1].start, document.ruby_spans[1].end), ("家の".len(), "家の外".len()));
}

#[test]
fn scowl_loader_uses_only_requested_word_lists() {
    let directory = TempDir::new().unwrap();
    let final_dir = directory.path().join("final");
    std::fs::create_dir(&final_dir).unwrap();
    std::fs::write(final_dir.join("english-words.10"), "alpha\ncan't\n").unwrap();
    std::fs::write(final_dir.join("american-words.20"), "color\n").unwrap();
    std::fs::write(final_dir.join("english-words.60"), "expanded\n").unwrap();
    std::fs::write(final_dir.join("english-words.70"), "excluded\n").unwrap();
    std::fs::write(final_dir.join("english-proper-names.10"), "Alice\n").unwrap();

    let words = load_scowl_words(directory.path()).unwrap();

    assert_eq!(words, BTreeSet::from(["alpha".to_owned(), "color".to_owned(), "expanded".to_owned()]));
}

#[test]
fn stable_sampling_is_repeatable_and_order_independent() {
    let values_a = vec!["delta", "alpha", "charlie", "bravo"];
    let values_b = vec!["bravo", "charlie", "alpha", "delta"];

    assert_eq!(stable_sample(&values_a, 3, 0x5eed), stable_sample(&values_a, 3, 0x5eed));
    assert_eq!(stable_sample(&values_a, 3, 0x5eed), stable_sample(&values_b, 3, 0x5eed));
}

#[test]
fn complete_english_dialogue_quotes_are_removed() {
    let text = r#""How's everything?" "Horrible, but I will manage.""#;

    assert_eq!(
        prepare_english_sentence(text).as_deref(),
        Some("How's everything? Horrible, but I will manage.")
    );
}

#[test]
fn english_word_quotes_are_preserved() {
    for text in [
        r#""Medicine is a religion," said the faith healer."#,
        r#"Paris is pronounced "Paree" in French."#,
    ] {
        assert_eq!(prepare_english_sentence(text).as_deref(), Some(text));
    }
}

#[test]
fn malformed_or_nested_english_dialogue_quotes_are_preserved() {
    for text in [
        r#""This sentence has a closing quote but also trailing text." extra"#,
        r#""This "nested" quote is not a complete dialogue sequence.""#,
        r#""Two quoted parts touch and therefore stay.""Without a space""#,
    ] {
        assert_eq!(prepare_english_sentence(text).as_deref(), Some(text));
    }
}

#[test]
fn english_sentence_length_is_checked_after_dialogue_quotes_are_removed() {
    let too_short_after_removal = format!("\"{}\"", "a".repeat(28));
    let maximum_after_removal = format!("\"{}\"", "a".repeat(90));

    assert!(prepare_english_sentence(&too_short_after_removal).is_none());
    assert_eq!(prepare_english_sentence(&maximum_after_removal).as_deref(), Some("a".repeat(90).as_str()));
}

#[test]
fn english_phrase_length_is_checked_after_dialogue_quotes_are_removed() {
    let too_short = "a".repeat(9);
    let minimum = format!("\"{}\"", "a".repeat(10));
    let maximum = "a".repeat(29);
    let too_long = "a".repeat(30);

    assert!(prepare_english_phrase(&too_short).is_none());
    assert_eq!(prepare_english_phrase(&minimum).as_deref(), Some("a".repeat(10).as_str()));
    assert_eq!(prepare_english_phrase(&maximum).as_deref(), Some(maximum.as_str()));
    assert!(prepare_english_phrase(&too_long).is_none());
    assert!(prepare_english_phrase("ten letters\n").is_none());
}

#[test]
fn japanese_phrases_are_ranked_by_reading_length_then_source_order() {
    let source = "長い表記\tあいうえおかき\n先の同順位\tあいうえおか\n後の同順位\tかきくけこさ\n";

    let rendered = prepare_japanese_phrases(source).unwrap();

    assert_eq!(
        rendered,
        "先の同順位\tあいうえおか\t1\tcore\n後の同順位\tかきくけこさ\t2\tcore\n長い表記\tあいうえおかき\t3\tcore\n"
    );
}

#[test]
fn japanese_phrase_validation_reports_the_bad_source_line() {
    for (source, expected_line) in [
        ("有効な句\tあいうえおか\n短すぎる\tあいうえお\n", 2),
        ("長すぎる句\tあいうえおかきくけこさしすせそたちつてとな\n", 1),
        ("ASCII1\tあいうえおか\n", 1),
        ("空 白\tあいうえおか\n", 1),
        ("句、読点\tあいうえおか\n", 1),
        ("不正な読み\tあいうえおA\n", 1),
        ("列不足だけ\n", 1),
    ] {
        let error = prepare_japanese_phrases(source).unwrap_err().to_string();
        assert!(error.contains(&format!("line {expected_line}")), "{error}");
    }
}

#[test]
fn japanese_phrase_validation_rejects_duplicate_surfaces_and_readings() {
    for source in [
        "同じ表示\tあいうえおか\n同じ表示\tかきくけこさ\n",
        "最初の表示\tあいうえおか\n別の表示\tあいうえおか\n",
    ] {
        let error = prepare_japanese_phrases(source).unwrap_err().to_string();
        assert!(error.contains("line 2"), "{error}");
    }
}

#[test]
fn bdf_parser_normalizes_rows_to_the_high_bits_of_u16() {
    let glyphs = parse_bdf(HALF_12.as_bytes(), FontKind::JisX0201, 6, 12).unwrap();
    let a = glyphs.get(&'A').unwrap();

    assert_eq!(a.width, 6);
    assert_eq!(a.rows.len(), 12);
    assert_eq!(a.rows[1], 0x2000);
    assert_eq!(a.rows[5], 0xf800);
}

#[test]
fn jis_codes_map_to_the_expected_unicode_characters() {
    assert_eq!(jis_x0208_to_char(0x2422), Some('あ'));
    assert_eq!(jis_x0208_to_char(0x3441), Some('漢'));

    let half = parse_bdf(HALF_12.as_bytes(), FontKind::JisX0201, 6, 12).unwrap();
    assert!(half.contains_key(&'A'));
}

#[test]
fn outline_font_round_trips_every_command_and_metric() {
    let font = OutlineFont {
        units_per_em: 1_000,
        ascender: 880,
        descender: -120,
        glyphs: [
            (
                'A',
                OutlineGlyph {
                    advance: 600,
                    commands: vec![
                        OutlineCommand::MoveTo(10, 20),
                        OutlineCommand::LineTo(40, 80),
                        OutlineCommand::QuadTo(50, 90, 60, 20),
                        OutlineCommand::CurveTo(70, 10, 80, -10, 90, 0),
                        OutlineCommand::Close,
                    ],
                },
            ),
            (
                ' ',
                OutlineGlyph {
                    advance: 500,
                    commands: Vec::new(),
                },
            ),
        ]
        .into_iter()
        .collect(),
    };

    let encoded = encode_outline_font(&font).unwrap();
    let decoded = decode_outline_font(&encoded).unwrap();

    assert_eq!(decoded, font);
}

#[test]
fn outline_font_gzip_is_byte_for_byte_deterministic() {
    let font = OutlineFont {
        units_per_em: 2_048,
        ascender: 1_800,
        descender: -500,
        glyphs: [(
            '亜',
            OutlineGlyph {
                advance: 2_048,
                commands: vec![
                    OutlineCommand::MoveTo(100, 200),
                    OutlineCommand::LineTo(1_900, 200),
                    OutlineCommand::Close,
                ],
            },
        )]
        .into_iter()
        .collect(),
    };
    let encoded = encode_outline_font(&font).unwrap();

    assert_eq!(gzip_deterministic(&encoded).unwrap(), gzip_deterministic(&encoded).unwrap());
}

#[test]
fn font_generation_is_byte_for_byte_deterministic() {
    let source = TempDir::new().unwrap();
    let bdf = source.path().join("bdf");
    std::fs::create_dir(&bdf).unwrap();
    std::fs::write(bdf.join("shnmk12.bdf"), FULL_12).unwrap();
    std::fs::write(bdf.join("shnm6x12r.bdf"), HALF_12).unwrap();
    std::fs::write(bdf.join("shnmk16.bdf"), blank_bdf(16, 16, &[9250, 13377])).unwrap();
    std::fs::write(bdf.join("shnm8x16r.bdf"), blank_bdf(8, 16, &[32, 65])).unwrap();
    std::fs::write(source.path().join("LICENSE"), b"license\n").unwrap();
    std::fs::write(source.path().join("AUTHORS"), b"authors\n").unwrap();
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    let first_report = generate_fonts(&FontGenerationOptions {
        source: source.path().to_owned(),
        out: first.path().to_owned(),
    })
    .unwrap();
    let second_report = generate_fonts(&FontGenerationOptions {
        source: source.path().to_owned(),
        out: second.path().to_owned(),
    })
    .unwrap();

    assert_eq!(first_report, second_report);
    assert_eq!(
        std::fs::read(first.path().join("shinonome12.bin.gz")).unwrap(),
        std::fs::read(second.path().join("shinonome12.bin.gz")).unwrap()
    );
    assert_eq!(
        std::fs::read(first.path().join("shinonome16.bin.gz")).unwrap(),
        std::fs::read(second.path().join("shinonome16.bin.gz")).unwrap()
    );
}

#[test]
fn font_generation_rejects_an_archive_with_the_wrong_sha256() {
    let root = TempDir::new().unwrap();
    let source = root.path().join("shinonome-0.9.11");
    let bdf = source.join("bdf");
    std::fs::create_dir_all(&bdf).unwrap();
    std::fs::write(bdf.join("shnmk12.bdf"), FULL_12).unwrap();
    std::fs::write(bdf.join("shnm6x12r.bdf"), HALF_12).unwrap();
    std::fs::write(bdf.join("shnmk16.bdf"), blank_bdf(16, 16, &[9250, 13377])).unwrap();
    std::fs::write(bdf.join("shnm8x16r.bdf"), blank_bdf(8, 16, &[32, 65])).unwrap();
    std::fs::write(source.join("LICENSE"), b"license\n").unwrap();
    std::fs::write(source.join("AUTHORS"), b"authors\n").unwrap();
    std::fs::write(root.path().join(gen_data::SHINONOME_ARCHIVE), b"not the upstream archive").unwrap();
    let out = TempDir::new().unwrap();

    let error = generate_fonts(&FontGenerationOptions {
        source: source.clone(),
        out: out.path().to_owned(),
    })
    .unwrap_err();
    assert!(error.to_string().contains(gen_data::SHINONOME_ARCHIVE_SHA256), "{error:#}");
    assert!(!out.path().join("shinonome12.bin.gz").exists());

    std::fs::remove_file(root.path().join(gen_data::SHINONOME_ARCHIVE)).unwrap();
    let report = generate_fonts(&FontGenerationOptions {
        source,
        out: out.path().to_owned(),
    })
    .unwrap();
    assert_eq!(report.input_hashes.len(), 6);
    assert_eq!(report.input_hashes[4].0, "LICENSE");
}

#[test]
fn shinonome_notice_update_is_utf8_and_idempotent() {
    let source = TempDir::new().unwrap();
    std::fs::write(source.path().join("LICENSE"), b"Public Domain license\n").unwrap();
    std::fs::write(source.path().join("AUTHORS"), b"Font authors\n").unwrap();
    let notice = source.path().join("NOTICE");
    std::fs::write(&notice, "Existing notices\n").unwrap();

    update_shinonome_notice(source.path(), &notice).unwrap();
    let once = std::fs::read(&notice).unwrap();
    update_shinonome_notice(source.path(), &notice).unwrap();
    let twice = std::fs::read(&notice).unwrap();

    assert_eq!(once, twice);
    let text = String::from_utf8(twice).unwrap();
    assert!(text.contains("http://openlab.ring.gr.jp/efont/shinonome/"));
    assert!(text.contains("Public Domain license"));
    assert!(text.contains("Font authors"));
}

#[test]
fn shinonome_manifest_update_records_counts_hashes_and_is_idempotent() {
    let directory = TempDir::new().unwrap();
    let font_dir = directory.path().join("font");
    std::fs::create_dir(&font_dir).unwrap();
    std::fs::write(font_dir.join("shinonome12.bin.gz"), b"font12").unwrap();
    std::fs::write(font_dir.join("shinonome16.bin.gz"), b"font16").unwrap();
    std::fs::write(directory.path().join("NOTICE"), b"notice").unwrap();
    let manifest = directory.path().join("manifest.toml");
    std::fs::write(
        &manifest,
        "generated_date = \"2026-11-15\"\n\n[[outputs]]\npath = \"NOTICE\"\ncount = 1\nsha256 = \"old\"\n",
    )
    .unwrap();
    let report = gen_data::FontGenerationReport {
        dot12_glyphs: 7_037,
        dot16_glyphs: 7_037,
        fullwidth_glyphs: 6_879,
        halfwidth_glyphs: 158,
        input_hashes: vec![("bdf/shnmk12.bdf".to_owned(), "abc123".to_owned())],
    };

    update_shinonome_manifest(&manifest, &font_dir, &report).unwrap();
    let once = std::fs::read(&manifest).unwrap();
    update_shinonome_manifest(&manifest, &font_dir, &report).unwrap();
    let twice = std::fs::read(&manifest).unwrap();

    assert_eq!(once, twice);
    let text = String::from_utf8(twice).unwrap();
    assert!(text.contains("shinonome-0.9.11p1.tar.bz2"));
    assert!(text.contains("font/shinonome12.bin.gz"));
    assert!(text.contains("count = 7037"));
    assert!(!text.contains("sha256 = \"old\""));
    assert!(text.contains("path = \"shinonome/shinonome-0.9.11/bdf/shnmk12.bdf\"\nsha256 = \"abc123\""));
}

#[test]
fn both_font_blocks_are_restored_after_data_outputs_are_rewritten() {
    let directory = TempDir::new().unwrap();
    let shinonome = directory.path().join("shinonome-0.9.11");
    std::fs::create_dir(&shinonome).unwrap();
    std::fs::write(shinonome.join("LICENSE"), b"Public Domain license\n").unwrap();
    std::fs::write(shinonome.join("AUTHORS"), b"Font authors\n").unwrap();
    let bizud = directory.path().join("morisawa-biz-ud-gothic-1.051");
    std::fs::create_dir(&bizud).unwrap();
    std::fs::write(bizud.join("OFL.txt"), b"SIL OPEN FONT LICENSE Version 1.1\n").unwrap();
    let font_dir = directory.path().join("font");
    std::fs::create_dir(&font_dir).unwrap();
    std::fs::write(font_dir.join("shinonome12.bin.gz"), b"font12").unwrap();
    std::fs::write(font_dir.join("shinonome16.bin.gz"), b"font16").unwrap();
    std::fs::write(font_dir.join("bizud-gothic.outl.gz"), b"outline").unwrap();
    let notice = directory.path().join("NOTICE");
    let manifest = directory.path().join("manifest.toml");
    let base_notice = "Generated data notices\n";
    let base_manifest = "generated_date = \"2026-11-15\"\n\n[[outputs]]\npath = \"NOTICE\"\ncount = 1\nsha256 = \"old\"\n";
    std::fs::write(&notice, base_notice).unwrap();
    std::fs::write(&manifest, base_manifest).unwrap();
    let shinonome_report = gen_data::FontGenerationReport {
        dot12_glyphs: 7_037,
        dot16_glyphs: 7_037,
        fullwidth_glyphs: 6_879,
        halfwidth_glyphs: 158,
        input_hashes: vec![("LICENSE".to_owned(), "license-hash".to_owned())],
    };
    let bizud_report = BizUdGenerationReport {
        requested_glyphs: 6_974,
        glyphs: 6_950,
        missing_glyphs: 24,
        uncompressed_bytes: 123,
        compressed_bytes: 45,
        input_hashes: vec![
            ("fonts/ttf/BIZUDGothic-Regular.ttf".to_owned(), "ttf-hash".to_owned()),
            ("OFL.txt".to_owned(), "ofl-hash".to_owned()),
        ],
    };

    update_shinonome_notice(&shinonome, &notice).unwrap();
    update_bizud_notice(&bizud, &notice).unwrap();
    update_shinonome_manifest(&manifest, &font_dir, &shinonome_report).unwrap();
    update_bizud_manifest(&manifest, &font_dir, &bizud_report).unwrap();
    let once_notice = std::fs::read(&notice).unwrap();
    let once_manifest = std::fs::read(&manifest).unwrap();
    update_shinonome_notice(&shinonome, &notice).unwrap();
    update_bizud_notice(&bizud, &notice).unwrap();
    update_shinonome_manifest(&manifest, &font_dir, &shinonome_report).unwrap();
    update_bizud_manifest(&manifest, &font_dir, &bizud_report).unwrap();
    assert_eq!(std::fs::read(&notice).unwrap(), once_notice);
    assert_eq!(std::fs::read(&manifest).unwrap(), once_manifest);
    let text = std::str::from_utf8(&once_notice).unwrap();
    assert!(text.contains("Shinonome bitmap fonts"));
    assert!(text.contains("BIZ UD Gothic"));
    assert!(text.contains("SIL OPEN FONT LICENSE Version 1.1"));
    let text = std::str::from_utf8(&once_manifest).unwrap();
    assert!(text.contains("font/shinonome12.bin.gz"));
    assert!(text.contains("font/bizud-gothic.outl.gz"));

    // Simulate the data command replacing both files, then run both font
    // updates again. Both font blocks must come back.
    std::fs::write(&notice, base_notice).unwrap();
    std::fs::write(&manifest, base_manifest).unwrap();
    update_shinonome_notice(&shinonome, &notice).unwrap();
    update_bizud_notice(&bizud, &notice).unwrap();
    update_shinonome_manifest(&manifest, &font_dir, &shinonome_report).unwrap();
    update_bizud_manifest(&manifest, &font_dir, &bizud_report).unwrap();
    assert_eq!(std::fs::read(&notice).unwrap(), once_notice);
    assert_eq!(std::fs::read(&manifest).unwrap(), once_manifest);
}

#[test]
fn aozora_work_table_lists_only_cited_works_in_id_order() {
    let works = [
        ("000752", "夏目漱石", "坊っちゃん"),
        ("000081", "宮沢賢治", "やまなし"),
        ("000753", "夏目漱石", "文鳥"),
    ];
    let used = BTreeSet::from(["000753", "000081"]);
    assert_eq!(render_aozora_works(works, &used), "000081\t宮沢賢治\tやまなし\n000753\t夏目漱石\t文鳥\n");
    assert_eq!(render_aozora_works(works, &BTreeSet::new()), "");
}

#[test]
fn aozora_work_table_is_built_from_the_selection_and_sentences() {
    let selection = "000752\t夏目漱石\t坊っちゃん\t真先芳秋\thttps://example.invalid/752.zip\n\
                     000081\t宮沢賢治\tやまなし\t土屋隆\thttps://example.invalid/81.zip\n";
    let sentences = "親譲りの無鉄砲で小供の時から損ばかりしている。\tおやゆずりのむてっぽうでこどものときからそんばかりしている。\t000752\t夏目漱石\text\n\
                     親譲りの無鉄砲で小供の時から損ばかりしていた。\tおやゆずりのむてっぽうでこどものときからそんばかりしていた。\t000752\t夏目漱石\tcore\n";
    assert_eq!(
        aozora_works_from_selection(selection, sentences).unwrap(),
        "000752\t夏目漱石\t坊っちゃん\n"
    );

    let unknown = "ある文。\tあるぶん。\t999999\t誰か\text\n";
    let error = aozora_works_from_selection(selection, unknown).unwrap_err().to_string();
    assert!(error.contains("999999"), "{error}");
    assert!(aozora_works_from_selection("000752\t夏目漱石\n", sentences).is_err());
}

#[test]
fn bundled_aozora_work_table_matches_the_selection() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let selection = std::fs::read_to_string(root.join("sources/aozora_selection.tsv")).unwrap();
    let mut sentences = String::new();
    std::io::Read::read_to_string(
        &mut flate2::read::GzDecoder::new(std::fs::File::open(root.join("../../data/ja_sentences.tsv.gz")).unwrap()),
        &mut sentences,
    )
    .unwrap();
    let expected = aozora_works_from_selection(&selection, &sentences).unwrap();
    let bundled = std::fs::read(root.join("../../data/aozora_works.tsv.gz")).unwrap();
    assert_eq!(
        bundled,
        gzip_deterministic(expected.as_bytes()).unwrap(),
        "regenerate data/aozora_works.tsv.gz"
    );
}

/// A valid hiragana-only sentence of `length` characters (including `。`),
/// distinct for each `seed`; its reading is the text itself.
fn kana_sentence(seed: usize, length: usize) -> String {
    let mut text = (0..length - 1)
        .map(|index| char::from_u32(0x3042 + ((seed * 7 + index * 3 + seed / 11) % 77) as u32).unwrap())
        .collect::<String>();
    text.push('。');
    text
}

fn passage_sentence(seed: usize, line: usize) -> PassageSentence {
    let text = kana_sentence(seed, 20);
    PassageSentence {
        reading: Some(text.clone()),
        text,
        line,
        start: seed * 100,
    }
}

fn no_overrides() -> std::collections::BTreeMap<String, Option<String>> {
    std::collections::BTreeMap::new()
}

#[test]
fn passage_runs_are_cut_into_chunks_of_three_to_five_sentences() {
    assert_eq!(chunk_run(0), vec![]);
    assert_eq!(chunk_run(2), vec![]);
    assert_eq!(chunk_run(3), vec![(0, 3)]);
    assert_eq!(chunk_run(5), vec![(0, 5)]);
    assert_eq!(chunk_run(6), vec![(0, 3), (3, 3)]);
    assert_eq!(chunk_run(7), vec![(0, 4), (4, 3)]);
    assert_eq!(chunk_run(11), vec![(0, 4), (4, 4), (8, 3)]);
    for len in 3..200 {
        let chunks = chunk_run(len);
        assert_eq!(chunks.iter().map(|(_, size)| size).sum::<usize>(), len);
        assert!(chunks.iter().all(|(_, size)| (3..=5).contains(size)), "{len}: {chunks:?}");
    }
}

#[test]
fn passages_join_consecutive_sentences_without_a_separator() {
    let sentences = (0..4).map(|seed| passage_sentence(seed, 0)).collect::<Vec<_>>();
    let passages = select_work_passages("000752", "夏目漱石", &sentences, &no_overrides());
    assert_eq!(passages.len(), 1);
    let expected = sentences.iter().map(|sentence| sentence.text.as_str()).collect::<String>();
    assert_eq!(passages[0].text, expected);
    assert_eq!(passages[0].reading, expected);
    assert_eq!(passages[0].sentences, 4);
    assert_eq!(passages[0].start, 0);
    assert_eq!(render_ja_passages(&passages), format!("{expected}\t{expected}\t000752\t夏目漱石\text\n"));

    let two = (0..2).map(|seed| passage_sentence(seed, 0)).collect::<Vec<_>>();
    assert!(select_work_passages("000752", "夏目漱石", &two, &no_overrides()).is_empty());
}

#[test]
fn passages_break_at_rejected_sentences_unknown_readings_and_paragraphs() {
    // Too short, ASCII, unbalanced quote, and an unknown reading each cut the run.
    for breaker in [
        PassageSentence {
            text: "短い。".to_owned(),
            reading: Some("みじかい。".to_owned()),
            line: 0,
            start: 350,
        },
        PassageSentence {
            text: format!("ABC{}", kana_sentence(90, 16)),
            reading: Some(kana_sentence(90, 16)),
            line: 0,
            start: 350,
        },
        PassageSentence {
            text: format!("「{}", kana_sentence(91, 16)),
            reading: Some(format!("「{}", kana_sentence(91, 16))),
            line: 0,
            start: 350,
        },
        PassageSentence {
            text: kana_sentence(92, 20),
            reading: None,
            line: 0,
            start: 350,
        },
    ] {
        let mut sentences = (0..3).map(|seed| passage_sentence(seed, 0)).collect::<Vec<_>>();
        sentences.push(breaker);
        sentences.extend((4..6).map(|seed| passage_sentence(seed, 0)));
        let passages = select_work_passages("000001", "作者", &sentences, &no_overrides());
        assert_eq!(passages.len(), 1, "{:?}", sentences[3]);
        assert_eq!(passages[0].sentences, 3);
        assert_eq!(passages[0].start, 0);
    }

    // Sentences of different paragraphs are not joined.
    let mut sentences = (0..3).map(|seed| passage_sentence(seed, 0)).collect::<Vec<_>>();
    sentences.extend((3..5).map(|seed| passage_sentence(seed, 1)));
    let passages = select_work_passages("000001", "作者", &sentences, &no_overrides());
    assert_eq!(passages.len(), 1);
    assert_eq!(passages[0].sentences, 3);
}

#[test]
fn passages_respect_the_length_limit_by_dropping_trailing_sentences() {
    let sentences = (0..5)
        .map(|seed| {
            let text = kana_sentence(seed, 45);
            PassageSentence {
                reading: Some(text.clone()),
                text,
                line: 0,
                start: seed * 100,
            }
        })
        .collect::<Vec<_>>();
    let passages = select_work_passages("000001", "作者", &sentences, &no_overrides());
    assert_eq!(passages.len(), 1);
    assert_eq!(passages[0].sentences, 3);
    assert!(passages[0].text.chars().count() <= JA_PASSAGE_MAX_CHARS);
    assert_eq!(passages[0].text.chars().count(), 135);
}

#[test]
fn passages_per_work_are_capped_deterministically_and_ordered_by_position() {
    // Forty separate paragraphs of three sentences each.
    let sentences = (0..120).map(|seed| passage_sentence(seed, seed / 3)).collect::<Vec<_>>();
    let first = select_work_passages("000081", "宮沢賢治", &sentences, &no_overrides());
    let second = select_work_passages("000081", "宮沢賢治", &sentences, &no_overrides());
    assert_eq!(first.len(), JA_PASSAGES_PER_WORK);
    assert_eq!(first, second);
    assert!(first.windows(2).all(|pair| pair[0].start < pair[1].start));
    let texts = first.iter().map(|passage| passage.text.clone()).collect::<BTreeSet<_>>();
    assert_eq!(texts.len(), JA_PASSAGES_PER_WORK);
    // The seed depends on the work, so another work may pick other paragraphs,
    // but it is still capped.
    assert_eq!(
        select_work_passages("000082", "宮沢賢治", &sentences, &no_overrides()).len(),
        JA_PASSAGES_PER_WORK
    );
}

#[test]
fn duplicate_passages_are_removed_within_and_across_works() {
    let mut sentences = (0..3).map(|seed| passage_sentence(seed, 0)).collect::<Vec<_>>();
    sentences.extend((0..3).map(|seed| {
        let mut sentence = passage_sentence(seed, 1);
        sentence.start += 10_000;
        sentence
    }));
    let one = select_work_passages("000002", "作者", &sentences, &no_overrides());
    assert_eq!(one.len(), 1);
    let other = select_work_passages("000001", "別の作者", &sentences[..3], &no_overrides());
    let merged = merge_passages([one, other]);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].source_id, "000002");
}

#[test]
fn sentence_overrides_apply_to_passage_sentences() {
    let sentences = (0..6).map(|seed| passage_sentence(seed, 0)).collect::<Vec<_>>();

    let mut excluded = no_overrides();
    excluded.insert(sentences[2].text.clone(), None);
    let passages = select_work_passages("000001", "作者", &sentences, &excluded);
    assert_eq!(passages.len(), 1);
    assert_eq!(passages[0].start, sentences[3].start);
    assert!(!passages[0].text.contains(&sentences[2].text));

    let replacement = kana_sentence(99, 20);
    let mut replaced = no_overrides();
    replaced.insert(sentences[1].text.clone(), Some(replacement.clone()));
    let passages = select_work_passages("000001", "作者", &sentences[..3], &replaced);
    assert_eq!(passages.len(), 1);
    assert_eq!(passages[0].reading, format!("{}{}{}", sentences[0].text, replacement, sentences[2].text));

    // An override whose reading breaks the reading rules also cuts the run.
    let mut invalid = no_overrides();
    invalid.insert(sentences[1].text.clone(), Some("ABC".to_owned()));
    assert!(select_work_passages("000001", "作者", &sentences[..3], &invalid).is_empty());
}

#[test]
fn passage_manifest_update_sets_filter_output_and_notice_hash_idempotently() {
    let temp = TempDir::new().unwrap();
    let manifest = temp.path().join("manifest.toml");
    let notice = temp.path().join("NOTICE");
    std::fs::write(&notice, "notice\n").unwrap();
    std::fs::write(
        &manifest,
        "generated_date = \"2026-01-01\"\n\n[filters]\nja_sentences = \"s\"\nja_other = \"o\"\n\n[[inputs]]\npath = \"a\"\nsha256 = \"0\"\n\n[[outputs]]\npath = \"ja_sentences.tsv.gz\"\ncount = 1\nsha256 = \"1\"\n\n[[outputs]]\npath = \"NOTICE\"\ncount = 1\nsha256 = \"old\"\n",
    )
    .unwrap();
    update_passage_manifest(&manifest, 42, "abc").unwrap();
    let once = std::fs::read_to_string(&manifest).unwrap();
    assert!(
        once.contains(&format!("ja_sentences = \"s\"\nja_passages = \"{JA_PASSAGES_FILTER}\"\nja_other = \"o\"")),
        "{once}"
    );
    assert!(
        once.contains("path = \"ja_sentences.tsv.gz\"\ncount = 1\nsha256 = \"1\"\n\n[[outputs]]\npath = \"ja_passages.tsv.gz\"\ncount = 42\nsha256 = \"abc\"\n\n[[outputs]]\npath = \"NOTICE\""),
        "{once}"
    );
    assert!(!once.contains("sha256 = \"old\""), "{once}");
    update_passage_manifest(&manifest, 42, "abc").unwrap();
    assert_eq!(std::fs::read_to_string(&manifest).unwrap(), once);
    update_passage_manifest(&manifest, 7, "def").unwrap();
    let changed = std::fs::read_to_string(&manifest).unwrap();
    assert_eq!(changed.matches("ja_passages").count(), 2, "{changed}");
    assert!(changed.contains("count = 7\nsha256 = \"def\""));
}

use std::collections::BTreeSet;

use gen_data::{
    BizUdGenerationReport, FontGenerationOptions, FontKind, OutlineCommand, OutlineFont, OutlineGlyph, decode_outline_font, encode_outline_font,
    extract_aozora, extract_english_body, generate_fonts, gzip_deterministic, jis_x0208_to_char, load_scowl_words, parse_bdf, prepare_english_phrase,
    prepare_english_sentence, prepare_japanese_phrases, stable_sample, update_bizud_manifest, update_bizud_notice, update_shinonome_manifest,
    update_shinonome_notice,
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

//! JSON Lines import and export for portable `.ttrec` records.

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::engine::session::{ItemLog, KeystrokeLog, SessionLog};
use crate::error::{AppError, AppResult};
use crate::record::{SessionHeader, SessionRecord};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct SessionLine {
    #[serde(rename = "type")]
    line_type: String,
    format_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_us: Option<u64>,
    #[serde(flatten)]
    header: SessionHeader,
}

#[derive(Serialize, Deserialize)]
struct ItemLine {
    #[serde(rename = "type")]
    line_type: String,
    #[serde(flatten)]
    item: ItemLog,
}

#[derive(Serialize, Deserialize)]
struct KeyLine {
    #[serde(rename = "type")]
    line_type: String,
    #[serde(flatten)]
    key: KeystrokeLog,
}

enum Event<'a> {
    Item(&'a ItemLog),
    Key(&'a KeystrokeLog),
}

pub fn write(record: &SessionRecord, mut writer: impl Write) -> AppResult<()> {
    write_line(
        &mut writer,
        &SessionLine {
            line_type: "session".to_owned(),
            format_version: FORMAT_VERSION,
            duration_us: Some(record.log.duration_us),
            header: record.header.clone(),
        },
    )?;

    let mut events = Vec::with_capacity(record.log.items.len() + record.log.keystrokes.len());
    for item in &record.log.items {
        events.push((item.shown_us, 0u8, item.idx, Event::Item(item)));
    }
    for key in &record.log.keystrokes {
        events.push((key.t_us, 1u8, key.seq, Event::Key(key)));
    }
    events.sort_by_key(|(time, kind, sequence, _)| (*time, *kind, *sequence));
    for (_, _, _, event) in events {
        match event {
            Event::Item(item) => write_line(
                &mut writer,
                &ItemLine {
                    line_type: "item".to_owned(),
                    item: item.clone(),
                },
            )?,
            Event::Key(key) => write_line(
                &mut writer,
                &KeyLine {
                    line_type: "key".to_owned(),
                    key: key.clone(),
                },
            )?,
        }
    }
    Ok(())
}

pub fn read(reader: impl BufRead) -> AppResult<SessionRecord> {
    let mut header = None;
    let mut items = Vec::new();
    let mut keystrokes = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line.map_err(|error| AppError::Record(format!("line {line_number}: {error}")))?;
        let value: serde_json::Value = serde_json::from_str(&line).map_err(|error| AppError::Record(format!("line {line_number}: {error}")))?;
        let line_type = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| AppError::Record(format!("line {line_number}: missing type")))?;
        match line_type {
            "session" if line_number == 1 => {
                let parsed: SessionLine = serde_json::from_value(value).map_err(|error| AppError::Record(format!("line {line_number}: {error}")))?;
                if parsed.format_version != FORMAT_VERSION {
                    return Err(AppError::Record(format!(
                        "line {line_number}: unsupported format_version {}",
                        parsed.format_version
                    )));
                }
                header = Some((parsed.header, parsed.duration_us));
            }
            "item" if line_number > 1 => {
                let parsed: ItemLine = serde_json::from_value(value).map_err(|error| AppError::Record(format!("line {line_number}: {error}")))?;
                items.push(parsed.item);
            }
            "key" if line_number > 1 => {
                let parsed: KeyLine = serde_json::from_value(value).map_err(|error| AppError::Record(format!("line {line_number}: {error}")))?;
                keystrokes.push(parsed.key);
            }
            "session" => return Err(AppError::Record(format!("line {line_number}: duplicate session header"))),
            other => return Err(AppError::Record(format!("line {line_number}: unexpected type {other}"))),
        }
    }
    let (header, duration_us) = header.ok_or_else(|| AppError::Record("line 1: missing session header".to_owned()))?;
    items.sort_by_key(|item| item.idx);
    keystrokes.sort_by_key(|key| key.seq);
    Ok(SessionRecord {
        log: SessionLog {
            duration_us: duration_us.unwrap_or_else(|| header.duration_ms.max(0) as u64 * 1_000),
            error_mode: if header.error_mode == "advance" {
                crate::config::ErrorMode::Advance
            } else {
                crate::config::ErrorMode::Stop
            },
            items,
            keystrokes,
        },
        header,
    })
}

fn write_line(writer: &mut impl Write, value: &impl Serialize) -> AppResult<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

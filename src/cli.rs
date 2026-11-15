//! Command-line interface shared by the binary and integration tests.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::i18n::Lang;

#[derive(Debug, Default, Parser)]
#[command(name = "rs-typing-trainer")]
pub struct Cli {
    /// Display language. Overrides the configuration file.
    #[arg(long, global = true, value_parser = parse_lang)]
    pub lang: Option<Lang>,

    /// Configuration file to read instead of the default search paths.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, PartialEq, Subcommand)]
pub enum Command {
    /// Start a typing session.
    Play,

    /// Replay a typing session record.
    Replay {
        /// Record file to replay.
        file: PathBuf,
        /// Override every keystroke interval, in milliseconds.
        #[arg(long, conflicts_with = "speed")]
        interval: Option<u64>,
        /// Replay recorded timestamps at this speed.
        #[arg(long, default_value_t = 1.0, value_parser = parse_positive_speed)]
        speed: f64,
        /// Start playback paused.
        #[arg(long)]
        paused: bool,
        /// Exit as soon as the final keystroke is applied.
        #[arg(long)]
        exit_on_end: bool,
        /// Print a record summary without opening the TUI.
        #[arg(long)]
        print: bool,
    },

    /// Show typing statistics.
    Stats {
        /// Emit machine-readable CSV for one table.
        #[arg(long, value_enum, conflicts_with_all = ["summary", "grades", "coach"])]
        csv: Option<StatsCsv>,
        /// Print the same rule-based summary shown in the first analysis tab.
        #[arg(long, conflicts_with_all = ["grades", "coach"])]
        summary: bool,
        /// Print absolute S-to-G skill grades.
        #[arg(long)]
        grades: bool,
        /// Print the same personalized report shown in the Coach tab.
        #[arg(long, conflicts_with_all = ["summary", "grades"])]
        coach: bool,
        /// Include sessions on or after this date (YYYY-MM-DD).
        #[arg(long)]
        since: Option<String>,
        /// Include only this session mode.
        #[arg(long)]
        mode: Option<String>,
    },

    /// Export one saved session as JSON Lines.
    Export {
        /// Numeric session id from the history database.
        session_id: i64,
        /// Output file. Standard output is used when omitted.
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum StatsCsv {
    Sessions,
    Keys,
    Bigrams,
    Fingers,
    Errors,
    Confusion,
    Kana,
    Spelling,
    Words,
    Form,
    Curve,
    Changepoints,
    Regressions,
    Habits,
    Prescriptions,
    Reviews,
    Grades,
    GradeHistory,
}

impl Cli {
    pub fn effective_lang(&self, configured: Lang) -> Lang {
        self.lang.unwrap_or(configured)
    }
}

fn parse_lang(value: &str) -> Result<Lang, String> {
    Lang::from_str(value).ok_or_else(|| "language must be 'en' or 'ja'".to_string())
}

fn parse_positive_speed(value: &str) -> Result<f64, String> {
    let speed = value.parse::<f64>().map_err(|_| "speed must be a positive number".to_owned())?;
    if speed.is_finite() && speed > 0.0 {
        Ok(speed)
    } else {
        Err("speed must be a positive finite number".to_owned())
    }
}

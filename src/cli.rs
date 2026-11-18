//! Command-line interface shared by the binary and integration tests.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::graphics::GraphicsPreference;
use crate::i18n::Lang;

#[derive(Debug, Default, Parser)]
#[command(name = "rs-typing-trainer")]
#[command(group = clap::ArgGroup::new("json_output").multiple(true))]
pub struct Cli {
    /// Display language. Overrides the configuration file.
    #[arg(long, global = true, value_parser = parse_lang)]
    pub lang: Option<Lang>,

    /// Configuration file to read instead of the default search paths.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    /// Override terminal image protocol selection.
    #[arg(long, global = true, value_enum)]
    pub graphics: Option<GraphicsPreference>,

    /// Print today's practice menu from the stats command.
    #[arg(long = "menu", global = true, group = "json_output")]
    pub stats_menu: bool,

    /// Print the skill records of the practice menu's check steps and their
    /// trends from the stats command.
    #[arg(long = "skills", global = true, group = "json_output", conflicts_with = "stats_menu")]
    pub stats_skills: bool,

    /// Recompute every stored skill record from the keystroke logs.
    #[arg(long = "rebuild-skills", global = true, conflicts_with_all = ["stats_menu", "stats_skills"])]
    pub rebuild_skills: bool,

    /// Emit the practice menu or the skill records as machine-readable JSON.
    #[arg(long, global = true, requires = "json_output")]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, PartialEq, Subcommand)]
pub enum Command {
    /// Start a typing session.
    Play,

    /// Detect terminal image support or show a test image.
    Graphics {
        /// Show a temporary image-protocol test screen.
        #[arg(long)]
        demo: bool,
        /// Maximum number of seconds to show the demo.
        #[arg(long, default_value_t = 3)]
        seconds: u64,
    },

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
        /// Emit machine-readable CSV for one table. With `--skills`, give
        /// `--csv` without a table to emit the skill series.
        #[arg(
            long,
            value_enum,
            num_args = 0..=1,
            conflicts_with_all = ["summary", "grades", "coach", "stats_menu", "rebuild_skills"]
        )]
        csv: Option<Option<StatsCsv>>,
        /// Print the same rule-based summary shown in the first analysis tab.
        #[arg(long, conflicts_with_all = ["grades", "coach", "stats_menu", "stats_skills", "rebuild_skills"])]
        summary: bool,
        /// Print absolute S-to-G skill grades.
        #[arg(long, conflicts_with_all = ["stats_menu", "stats_skills", "rebuild_skills"])]
        grades: bool,
        /// Print the same personalized report shown in the Coach tab.
        #[arg(long, conflicts_with_all = ["summary", "grades", "stats_menu", "stats_skills", "rebuild_skills"])]
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
    /// Checks that clap cannot express: `--csv` without a table is only for
    /// `--skills`, a table name conflicts with `--skills`, and `--csv` and
    /// `--json` cannot be combined.
    pub fn validate(&self) -> Result<(), String> {
        let Some(Command::Stats { csv: Some(table), .. }) = &self.command else {
            return Ok(());
        };
        if self.json {
            return Err("--csv and --json cannot be used together".to_owned());
        }
        match (table, self.stats_skills) {
            (Some(_), true) => Err("--skills takes --csv without a table name".to_owned()),
            (None, false) => Err("--csv needs a table name (or use it with --skills)".to_owned()),
            _ => Ok(()),
        }
    }

    /// [`Parser::try_parse_from`] followed by [`Cli::validate`]; the binary
    /// parses its arguments this way.
    pub fn try_parse_validated<I, T>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let cli = <Self as Parser>::try_parse_from(args).map_err(|error| error.to_string())?;
        cli.validate()?;
        Ok(cli)
    }

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

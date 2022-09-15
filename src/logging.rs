use flexi_logger::Record;
use std::path::PathBuf;
use structopt::StructOpt;
use thiserror::Error;

#[derive(StructOpt, Debug, Clone)]
pub struct Opt {
    #[structopt(help = "Logging file for debugging", long = "log-file")]
    pub log_file: Option<String>,

    #[structopt(help = "Directory for rotated log files", long = "log-dir")]
    pub log_dir: Option<PathBuf>,

    #[structopt(help = "Logging level for debugging (info/debug)", long = "log-level")]
    pub log_level: Option<String>,

    #[structopt(help = "Disable stderr-logging", long = "no-stderr-logging")]
    pub stderr_logging_disable: bool,

    #[structopt(
        help = "Max uncompressed log content in MB",
        long = "max-log-size",
        default_value = "128"
    )]
    pub max_log_size: u64,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid logging level")]
    InvalidLoggingLevel,

    #[error("Io error; {0}")]
    IoError(#[from] std::io::Error),

    #[error("Error setting logger: {0}")]
    SetLogger(#[from] log::SetLoggerError),

    #[error("FlexiLogger error: {0}")]
    FlexiLogger(#[from] flexi_logger::FlexiLoggerError),
}

pub type FilterFunction = fn(&mut String, record: &log::Record) -> bool;

static mut FILTER_FUNC: FilterFunction = empty_filter;

fn my_minimal_console_formatting(
    w: &mut dyn std::io::Write,
    _now: &mut flexi_logger::DeferredNow,
    record: &log::Record,
) -> Result<(), std::io::Error> {
    use flexi_logger::style;

    let level = record.level();
    let low = ansi_term::Colour::RGB(110, 110, 110);
    let mut filename = record.file().unwrap_or("<unnamed>");
    if let Some(idx) = filename.rfind("/") {
        filename = &filename[idx + 1..];
    }
    if filename.ends_with(".rs") {
        filename = &filename[..filename.len() - 3];
    }

    // FIXME use _now.
    let now = chrono::Local::now();
    let mut args = record.args().to_string();

    if !unsafe { FILTER_FUNC }(&mut args, &record) {
        return Ok(());
    }

    write!(
        w,
        "{} {} {}:{} {}",
        low.paint(now.format("%H:%M:%S%.3f").to_string()),
        style(level).paint(format!("{:5}", record.level())),
        low.paint(format!("{:>14}", filename)),
        low.paint(format!("{:5}", record.line().unwrap_or(0).to_string())),
        args,
    )
}

pub fn empty_filter(_msg: &mut String, _record: &Record) -> bool {
    true
}

pub fn activate(opt: &Opt, console_filter_func: FilterFunction) -> Result<(), Error> {
    use flexi_logger::*;

    unsafe {
        FILTER_FUNC = console_filter_func;
    }

    let mut logger = if let Some(log_level) = &opt.log_level {
        match log_level.as_str() {
            "trace" => Logger::try_with_str(log_level.as_str()),
            "debug" => Logger::try_with_str(log_level.as_str()),
            "info" => Logger::try_with_str(log_level.as_str()),
            "warn" => Logger::try_with_str(log_level.as_str()),
            "error" => Logger::try_with_str(log_level.as_str()),
            _ => return Err(Error::InvalidLoggingLevel),
        }
    } else {
        Logger::try_with_str("info")
    }?;

    logger = logger.set_palette("b1;3;2;4;6".to_owned());

    if let Some(log_file) = &opt.log_file {
        logger = logger
            .write_mode(WriteMode::Async)
            .format_for_files(flexi_logger::detailed_format)
            .log_to_file(FileSpec::try_from(log_file)?);
        if !opt.stderr_logging_disable {
            logger = logger.print_message();
        }
    };

    if let Some(log_dir) = &opt.log_dir {
        use flexi_logger::*;
        let nr_files = 8;

        logger = logger
            .write_mode(WriteMode::Async)
            .log_to_file(FileSpec::default()
                    .directory(log_dir) // create files in folder ./log_files
                    .basename("ksite"))
            .rotate(
                Criterion::Size(opt.max_log_size * 0x100000 / nr_files),
                Naming::Timestamps,
                Cleanup::KeepLogFiles(nr_files as usize),
            )
            .print_message()
            .format_for_files(flexi_logger::detailed_format);
    };

    if !opt.stderr_logging_disable {
        logger = logger
            .adaptive_format_for_stderr(AdaptiveFormat::Detailed)
            .format_for_stderr(my_minimal_console_formatting)
            .duplicate_to_stderr(if let Some(log_level) = &opt.log_level {
                match log_level.as_str() {
                    "trace" => Duplicate::Trace,
                    "debug" => Duplicate::Debug,
                    "info" => Duplicate::Info,
                    "warn" => Duplicate::Warn,
                    "error" => Duplicate::Error,
                    _ => return Err(Error::InvalidLoggingLevel),
                }
            } else {
                Duplicate::Trace
            });
    }

    let x = logger.start()?;
    std::mem::forget(x);

    Ok(())
}

use std::io::{self, Write, BufRead};
use std::collections::HashMap;
use termcolor::{BufferWriter, WriteColor, ColorChoice, Color, ColorSpec};
use anyhow::Result;
use serde_json::Value;
use structopt::StructOpt;

mod parser;
mod cli;
mod filters;

use cli::*;
use filters::Filter;

#[derive(Debug, StructOpt)]
#[structopt(name = "jaxe", about = "A j[son] [pick]axe!")]
pub(crate) struct Opt {
    /// Fields to extract, default to extracting all fields
    #[structopt(short, long, default_value)]
    extract: MultOpt<String>,

    /// Fields to omit
    #[structopt(short, long, default_value)]
    omit: MultOpt<String>,

    /// Do not print non-json lines
    #[structopt(short = "j", long)]
    no_omit_json: bool,

    /// Filter by. See parse language
    #[structopt(short = "f", long)]
    filter: Vec<String>,

    /// Use jq filters
    #[cfg(feature = "jq")]
    #[structopt(long)]
    jq: bool,

    /// level keys. The first of these keys in the json line will be used as the level of the log line and formatted at the start of the line.
    #[structopt(short, long)]
    level: Vec<String>,

    /// Time keys. The first of these keys in the json line will be used as the date of the log line and formatted after the level.
    #[structopt(short, long)]
    time: Vec<String>,

    /// Disable colors
    #[structopt(short, long)]
    no_colors: bool,
}

fn level_to_color(level: &str) -> Color {
    match level {
        "TRACE" => Color::Magenta,
        "DEBUG" => Color::Blue,
        "INFO" => Color::Green,
        "WARN" => Color::Yellow,
        "ERROR" => Color::Red,
        _ => Color::Red
    }
}




fn write_formatted_line(opts: &Opt, line: Value, filters: &mut filters::Filters, output: &termcolor::BufferWriter) -> Result<()> {
    if ! filters.apply(&line)? {
        return Ok(())
    }

    let mut json = serde_json::from_value::<HashMap<String, Value>>(line)?;

    for key in opts.omit.0.iter() {
        log::debug!("Not writing key {} due to --omit", key);
        json.remove(key);
    }

    let mut buffer = output.buffer();

    for key in &opts.level {
        if let Some(level) = json.get(key).and_then(|s| s.as_str()) {
            buffer.set_color(ColorSpec::new().set_fg(Some(level_to_color(level))))?;
            write!(&mut buffer, "{}", level.chars().nth(0).unwrap_or('?'))?;
            buffer.set_color(ColorSpec::new().set_fg(None))?;
            write!(&mut buffer, "|")?;
            json.remove(key);

            break;
        }
    }

    for key in &opts.time {
        if let Some(at) = json.get(key).and_then(|s| s.as_str()) {
            buffer.set_color(ColorSpec::new().set_fg(None))?;
            write!(&mut buffer, "{}|", at)?;
            json.remove(key);
            break;
        }
    }

    let mut keys: Vec<&String> = json.keys().collect();
    keys.sort();

    // TODO: Extract should also support jq style expressions
    for key in keys {
        if ! opts.extract.0.is_empty() && ! opts.extract.0.contains(key) {
            log::debug!("Not writing key {} due to --extract", key);
            continue;
        }

        let value: &Value = json.get(key).unwrap();
        buffer.set_color(ColorSpec::new().set_fg(Some(Color::Blue)))?;

        write!(&mut buffer, "{}", key)?;

        if let Some(n) = value.as_str().and_then(|s| s.parse::<u64>().ok()) {
            buffer.set_color(ColorSpec::new().set_fg(None).set_dimmed(true))?;
            write!(&mut buffer, "=")?;
            buffer.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_dimmed(true))?;
            write!(&mut buffer, "{} ", n)?;
        } else if let Some(s) = value.as_str() {
            buffer.set_color(ColorSpec::new().set_fg(None).set_dimmed(true))?;
            write!(&mut buffer, "=")?;
            buffer.set_color(ColorSpec::new().set_fg(None).set_dimmed(false))?;
            write!(&mut buffer, "{} ", s)?;
        } else {
            buffer.set_color(ColorSpec::new().set_fg(None).set_dimmed(true))?;
            write!(&mut buffer, "=")?;
            buffer.set_color(ColorSpec::new().set_fg(None))?;
            write!(&mut buffer, "{} ", value)?;
        }
    }

    writeln!(&mut buffer, "")?;
    output.print(&buffer)?;

    Ok(())
}


fn main() -> io::Result<()> {
    pretty_env_logger::init();

    let mut opts = Opt::from_args();

    if opts.time.is_empty() {
        opts.time.push("time".to_owned());
        opts.time.push("at".to_owned());
    }

    if opts.level.is_empty() {
        opts.level.push("level".to_owned());
    }

    if let Some(e) = std::env::var("JAXE_OMIT").ok()  {
        opts.omit = MultOpt(e.split(",").map(|s| s.to_owned()).collect());
    }

    if let Some(e) = std::env::var("JAXE_FILTER").ok()  {
        opts.filter = vec![e.to_owned()];
    }

    let mut line_buffer = String::new();
    let stdin = io::stdin();
    let mut handle =  stdin.lock();

    let bufwtr = if opts.no_colors {
        BufferWriter::stdout(ColorChoice::Never)
    } else {
        BufferWriter::stdout(ColorChoice::Auto)
    };

    let mut filters = filters::Filters::from_opts(&opts);

    loop {
        match handle.read_line(&mut line_buffer) {
            Err(_) | Ok(0) => {
                log::debug!("Finished");
                break;
            },
            Ok(c) =>
                log::debug!("read {} bytes", c)
        }

        match serde_json::from_str(&line_buffer) {
            Ok(json) =>
                write_formatted_line(&opts, json, &mut filters, &bufwtr).unwrap(),
            Err(err) => {
                log::debug!("Could not parse line as json: {:?}", err);

                if ! opts.no_omit_json {
                    let mut obuf = bufwtr.buffer();
                    obuf.set_color(ColorSpec::new().set_fg(Some(Color::White)).set_dimmed(true))?;

                    write!(&mut obuf, "{}", line_buffer)?;

                    bufwtr.print(&mut obuf)?;
                }
            }
        }

        line_buffer.clear()
    }

    Ok(())
}

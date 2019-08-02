use csv;
use failure::Error;
use number_prefix::NumberPrefix;
use regex::Regex;
use std::{collections::HashMap, fs, io, path::PathBuf};
use structopt::StructOpt;
use termion::{input::MouseTerminal, raw::IntoRawMode, screen::AlternateScreen};
use tui::{
    backend::TermionBackend,
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Row, Table, Widget},
    Terminal,
};
use walkdir::WalkDir;

#[derive(StructOpt, Debug)]
#[structopt(name = "wbf", about = "What big file?")]
struct Opt {
    /// Path to search
    #[structopt(short, long, parse(from_os_str))]
    path: PathBuf,

    /// Depth to search (value of `0` is recursive)
    #[structopt(short = "d", long)]
    depth: Option<usize>,

    /// Disable symbolic links
    #[structopt(short = "s", long)]
    disable_symlinks: bool,

    /// Filter (regex)
    #[structopt(short, long)]
    filter: Option<String>,

    /// Minimum file size to search for in bytes
    #[structopt(short, long)]
    min_size: Option<u64>,

    /// Output CSV file
    #[structopt(short, long, parse(from_os_str))]
    output_file: Option<PathBuf>,
}

fn main() -> Result<(), Error> {
    let opt = Opt::from_args();
    // Terminal initialization
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let regex_opt = opt
        .filter
        .clone()
        .map(|filter| Regex::new(&filter).expect("Regex is invalid!"));
    let walker = WalkDir::new(&opt.path)
        .follow_links(!opt.disable_symlinks)
        .into_iter()
        .filter_entry(|entry| {
            if let Some(regex) = &regex_opt {
                if let Some(path) = entry.path().to_str() {
                    !regex.is_match(path)
                } else {
                    false
                }
            } else {
                true
            }
        });

    let mut total = 0;
    let mut entries = HashMap::new();
    for entry_res in walker {
        if let Ok(entry) = entry_res {
            // Break when we get too deep
            if let Some(depth) = opt.depth {
                if depth > 0 && entry.depth() > depth {
                    break;
                }
            }

            // Skip directories
            if entry.file_type().is_dir() {
                continue;
            }

            // Resolve symlinks
            let path_is_symlink = entry.path_is_symlink();
            let symlink_realpath;
            let mut realpath = entry.path();
            if path_is_symlink {
                if let Ok(path_buf) = fs::read_link(entry.path()) {
                    symlink_realpath = path_buf;
                    realpath = symlink_realpath.as_path()
                }
            };

            if let (Some(path), Ok(metadata)) = (realpath.to_str(), entry.metadata()) {
                let size = metadata.len();
                if size < opt.min_size.unwrap_or(0) {
                    continue;
                }

                total += size;
                entries.insert(String::from(path), size);
            }
        }

        terminal.draw(|mut f| {
            // let selected_style = Style::default().fg(Color::Yellow).modifier(Modifier::BOLD);
            let normal_style = Style::default().fg(Color::White);
            let header = ["File", "Size", "Percentage of Total"];
            let mut sorted_entries = entries.iter().collect::<Vec<_>>();
            sorted_entries.sort_by(|(.., a_size_bytes), (.., b_size_bytes)| {
                (**b_size_bytes).partial_cmp(&**a_size_bytes).unwrap()
            });
            let rows = sorted_entries.iter().map(|(path, size_bytes)| {
                let size_human_readable = match NumberPrefix::decimal(**size_bytes as f64) {
                    NumberPrefix::Standalone(bytes) => format!("{} B", bytes),
                    NumberPrefix::Prefixed(prefix, n) => format!("{:.*} {}B", 2, n, prefix),
                };

                Row::StyledData(
                    vec![
                        String::from(&(*path).clone()),
                        size_human_readable,
                        format!("{:.*}%", 2, (**size_bytes as f64 / total as f64) * 100_f64),
                    ]
                    .into_iter(),
                    normal_style,
                )
            });

            let rects = Layout::default()
                .constraints([Constraint::Percentage(100)].as_ref())
                .margin(5)
                .split(f.size());
            Table::new(header.iter(), rows)
                .block(Block::default().borders(Borders::ALL).title("Table"))
                .widths(&[200, 10, 10])
                .render(&mut f, rects[0]);
        })?
    }

    if let Some(output_file) = opt.output_file {
        let mut wtr = csv::Writer::from_path(output_file)?;
        let mut sorted_entries = entries.iter().collect::<Vec<_>>();
        sorted_entries.sort_by(|(.., a_size_bytes), (.., b_size_bytes)| {
            (**b_size_bytes).partial_cmp(&**a_size_bytes).unwrap()
        });
        sorted_entries.iter().for_each(|(path, size_bytes)| {
            let size_human_readable = match NumberPrefix::decimal(**size_bytes as f64) {
                NumberPrefix::Standalone(bytes) => format!("{} B", bytes),
                NumberPrefix::Prefixed(prefix, n) => format!("{:.*} {}B", 2, n, prefix),
            };

            wtr.write_record(&[
                String::from(&(*path).clone()),
                size_human_readable,
                format!("{:.*}%", 2, (**size_bytes as f64 / total as f64) * 100_f64),
            ])
            .unwrap();
        });

        wtr.flush()?;
    }

    Ok(())
}

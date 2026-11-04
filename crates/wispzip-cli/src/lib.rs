use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Parser, Subcommand, ValueEnum};
use wispzip_core as core;
use wispzip_core::{CompressOptions, ExtractOptions, Format, NameEncoding};

#[derive(Parser)]
#[command(
    name = "wispzip",
    version,
    about = "wispzip - cross-platform file archiver",
    long_about = "wispzip - cross-platform file archiver\n\nCompress to zip/7z/tar/tar.gz/tar.bz2/tar.xz/tar.zst/gz/zst/xz/bz2,\nextract those plus rar (read-only). Format is inferred from the file\nextension. Optional byte-level split volumes (--split 100M)."

)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compress files or directories into an archive.
    #[command(alias = "c")]
    Compress {
        /// Output archive path; the extension picks the format (e.g.
        /// out.tar.gz). Omit to write <first-input>.zip next to the input.
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Input files and/or directories.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Compression level (zip/gz/xz/bz2/7z: 0-9, zstd: 1-22).
        #[arg(short, long, default_value_t = 6)]
        level: u8,
        /// Password (zip / 7z: AES-256).
        #[arg(short, long)]
        password: Option<String>,
        /// Store entries without compressing (zip only).
        #[arg(long)]
        store: bool,
        /// Emit fixed-size volumes instead of one file, e.g. 100M, 1G, 500K.
        #[arg(long, value_name = "SIZE")]
        split: Option<String>,
    },
    /// Extract an archive into a directory.
    #[command(alias = "x")]
    Extract {
        /// Archive to extract.
        archive: PathBuf,
        /// Destination directory (default: <archive-name>/ next to the archive).
        #[arg(short = 'o', long)]
        dest: Option<PathBuf>,
        /// Extract into the archive's own folder instead of a subdirectory.
        #[arg(long)]
        here: bool,
        /// What to do when files already exist in the destination.
        #[arg(long, value_enum, default_value_t = ConflictArg::Rename)]
        on_conflict: ConflictArg,
        /// Decoding for legacy non-UTF-8 entry names (zip).
        #[arg(long, value_enum, default_value_t = EncodingArg::Auto)]
        encoding: EncodingArg,
        /// Password (zip / 7z / rar).
        #[arg(short, long)]
        password: Option<String>,
        /// Extract only these entries (repeatable).
        #[arg(long)]
        entry: Vec<String>,
    },
    /// List the contents of an archive.
    #[command(alias = "l")]
    List {
        /// Archive to inspect.
        archive: PathBuf,
        /// Decoding for legacy non-UTF-8 entry names (zip).
        #[arg(long, value_enum, default_value_t = EncodingArg::Auto)]
        encoding: EncodingArg,
        /// Password (7z / rar with encrypted headers).
        #[arg(short, long)]
        password: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ConflictArg {
    Skip,
    Overwrite,
    Rename,
}

impl From<ConflictArg> for core::Conflict {
    fn from(v: ConflictArg) -> Self {
        match v {
            ConflictArg::Skip => core::Conflict::Skip,
            ConflictArg::Overwrite => core::Conflict::Overwrite,
            ConflictArg::Rename => core::Conflict::Rename,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum EncodingArg {
    Auto,
    Utf8,
    Gbk,
}

impl From<EncodingArg> for NameEncoding {
    fn from(v: EncodingArg) -> Self {
        match v {
            EncodingArg::Auto => NameEncoding::Auto,
            EncodingArg::Utf8 => NameEncoding::Utf8,
            EncodingArg::Gbk => NameEncoding::Gbk,
        }
    }
}

/// Terminal progress line: `[####------] 42.3% 12.3 MB/s file.txt`
struct TermProgress {
    start: Instant,
    last_line_len: usize,
    is_tty: bool,
}

impl TermProgress {
    fn new() -> Self {
        use std::io::IsTerminal;
        Self {
            start: Instant::now(),
            last_line_len: 0,
            is_tty: std::io::stderr().is_terminal(),
        }
    }

    fn show(&mut self, p: &core::Progress) -> bool {
        let speed = {
            let secs = self.start.elapsed().as_secs_f64().max(0.001);
            p.processed_bytes as f64 / secs
        };
        let mut line = String::new();
        if let Some(total) = p.total_bytes.filter(|t| *t > 0) {
            let frac = p.processed_bytes as f64 / total as f64;
            let filled = ((frac * 24.0) as usize).min(24);
            line.push('[');
            line.push_str(&"#".repeat(filled));
            line.push_str(&"-".repeat(24 - filled));
            line.push_str(&format!("] {:5.1}% {}", frac * 100.0, fmt_speed(speed)));
        } else {
            line.push_str(&format!(
                "[................] {:>9} {}",
                fmt_bytes(p.processed_bytes as f64),
                fmt_speed(speed)
            ));
        }
        if let Some(f) = &p.current_file {
            let name = f.rsplit('/').next().unwrap_or(f);
            line.push_str(&format!(" {name}"));
        }
        if self.is_tty {
            let pad = self.last_line_len.saturating_sub(line.len());
            eprint!("\r{line}{}", " ".repeat(pad));
            self.last_line_len = line.len();
        }
        true // keep running; Ctrl-C aborts the process
    }

    fn finish(&self) {
        if self.is_tty {
            eprintln!();
        }
    }
}

fn fmt_bytes(mut b: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut i = 0;
    while b >= 1024.0 && i < UNITS.len() - 1 {
        b /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b:.0} {}", UNITS[i])
    } else {
        format!("{b:.1} {}", UNITS[i])
    }
}

fn fmt_speed(bps: f64) -> String {
    format!("{}/s", fmt_bytes(bps))
}

fn default_dest(archive: &std::path::Path) -> PathBuf {
    let name = archive
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "archive".into());
    let stem = Format::from_archive_name(&name)
        .map(|f| name[..name.len() - f.extension().len() - 1].to_string())
        .unwrap_or(name);
    archive
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join(stem)
}

/// Parse CLI args and execute the command. Returns the process exit code.
pub fn run_cli(args: Vec<String>) -> i32 {
    let cli = match Cli::try_parse_from(&args) {
        Ok(c) => c,
        Err(e) => {
            e.print().ok();
            return if e.use_stderr() { 2 } else { 0 };
        }
    };

    let mut term = TermProgress::new();
    match cli.command {
        Command::Compress {
            out,
            inputs,
            level,
            password,
            store,
            split,
        } => {
            if inputs.is_empty() {
                eprintln!("wispzip: no input files given");
                return 2;
            }
            // -o omitted -> derive <first-input>.zip (right-click quick action)
            let out = out.unwrap_or_else(|| {
                let first = &inputs[0];
                let name = first
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "archive".into());
                first.parent().unwrap_or(Path::new(".")).join(format!("{name}.zip"))
            });
            let format = match core::infer_output_format(&out) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("wispzip: {e}");
                    return 2;
                }
            };
            let split = match split.as_deref().map(parse_size) {
                Some(Ok(bytes)) => Some(bytes),
                Some(Err(msg)) => {
                    eprintln!("wispzip: --split {msg}");
                    return 2;
                }
                None => None,
            };
            let opt = CompressOptions {
                format,
                level,
                store_only: store,
                password,
                split,
                exclude_hidden: false,
            };
            match core::compress(&inputs, &out, &opt, |p| term.show(p)) {
                Ok(stats) => {
                    term.finish();
                    println!(
                        "compressed {} file(s), {} -> {} ({:.1}%)",
                        stats.input_files,
                        fmt_bytes(stats.input_bytes as f64),
                        out.display(),
                        if stats.input_bytes > 0 {
                            stats.output_bytes as f64 * 100.0 / stats.input_bytes as f64
                        } else {
                            100.0
                        }
                    );
                    0
                }
                Err(e) => {
                    term.finish();
                    eprintln!("wispzip: {e}");
                    1
                }
            }
        }
        Command::Extract {
            archive,
            dest,
            here,
            on_conflict,
            encoding,
            password,
            entry,
        } => {
            let dest = dest.unwrap_or_else(|| {
                if here {
                    archive
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| PathBuf::from("."))
                } else {
                    default_dest(&archive)
                }
            });
            let opt = ExtractOptions {
                dest,
                entries: (!entry.is_empty()).then_some(entry),
                conflict: on_conflict.into(),
                encoding: encoding.into(),
                password,
                strip_prefix: None,
            };
            match core::extract(&archive, &opt, |p| term.show(p)) {
                Ok(stats) => {
                    term.finish();
                    println!(
                        "extracted {} file(s), {} -> {}{}",
                        stats.files,
                        fmt_bytes(stats.bytes as f64),
                        stats.dest.display(),
                        if stats.skipped > 0 {
                            format!(" ({} skipped)", stats.skipped)
                        } else {
                            String::new()
                        }
                    );
                    0
                }
                Err(e) => {
                    term.finish();
                    eprintln!("wispzip: {e}");
                    1
                }
            }
        }
        Command::List {
            archive,
            encoding,
            password,
        } => match core::list_archive(&archive, encoding.into(), password.as_deref()) {
            Ok(entries) => {
                println!("{:>10}  {:>10}  NAME", "SIZE", "PACKED");
                for e in &entries {
                    let kind = if e.is_dir { "d" } else { "-" };
                    println!(
                        "{kind}{:>9}  {:>10}  {}",
                        if e.size == 0 {
                            "-".to_string()
                        } else {
                            fmt_bytes(e.size as f64)
                        },
                        fmt_bytes(e.compressed_size as f64),
                        e.name
                    );
                }
                0
            }
            Err(e) => {
                eprintln!("wispzip: {e}");
                1
            }
        },
    }
}

/// Parse a human byte size: "500K", "100M", "1G", or plain bytes.
fn parse_size(s: &str) -> Result<u64, String> {
    let t = s.trim().trim_end_matches('B').trim_end_matches('b');
    let (digits, mult) = match t.chars().last() {
        Some(c) if c.is_ascii_digit() => (t, 1u64),
        Some(c) => {
            let mult = match c.to_ascii_uppercase() {
                'K' => 1024,
                'M' => 1024 * 1024,
                'G' => 1024 * 1024 * 1024,
                'T' => 1024u64 * 1024 * 1024 * 1024,
                other => return Err(format!("invalid size suffix '{other}' in \"{s}\"")),
            };
            (t[..t.len() - 1].trim(), mult)
        }
        None => return Err("size must not be empty".into()),
    };
    digits
        .parse::<u64>()
        .map(|n| n * mult)
        .map_err(|_| format!("size \"{s}\" is not a valid number"))
}

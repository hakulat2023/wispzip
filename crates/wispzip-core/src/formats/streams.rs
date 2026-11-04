//! Single-file streams: gz / zst / xz / bz2. Each archive wraps exactly one
//! file's bytes; the extracted name is the archive name minus the suffix.

use std::fs::File;
use std::io::{self, BufReader, BufWriter};
use std::path::Path;
use std::time::SystemTime;

use crate::compress::{CompressOptions, PlanEntry};
use crate::error::{io_err, Result};
use crate::extract::{set_file_metadata, ExtractOptions, ExtractStats, TargetAction};
use crate::format::Format;
use crate::progress::{system_time_from_unix, CountingReader, CountingWriter, Progress, Shared};

// ---- write -----------------------------------------------------------------

pub(crate) fn write<F>(
    format: Format,
    entries: &[PlanEntry],
    sink: CountingWriter<BufWriter<File>, F>,
    opt: &CompressOptions,
    shared: &Shared<F>,
) -> Result<()>
where
    F: FnMut(&Progress) -> bool,
{
    let e = &entries[0];
    {
        let mut r = shared.borrow_mut();
        r.current = Some(e.name.clone());
        r.report(true);
    }
    let f = File::open(&e.src).map_err(|er| io_err(er, &e.src))?;
    let mut cr = CountingReader::new(BufReader::new(f), shared.clone());
    match format {
        Format::Gz => {
            let mut enc =
                flate2::write::GzEncoder::new(sink, flate2::Compression::new(opt.level.min(9) as u32));
            io::copy(&mut cr, &mut enc).map_err(|er| io_err(er, &e.src))?;
            enc.finish().map_err(|er| io_err(er, &e.src))?;
        }
        Format::Zst => {
            let mut enc = zstd::stream::write::Encoder::new(sink, opt.level.clamp(1, 22) as i32)
                .map_err(|er| io_err(er, &e.src))?;
            io::copy(&mut cr, &mut enc).map_err(|er| io_err(er, &e.src))?;
            enc.finish().map_err(|er| io_err(er, &e.src))?;
        }
        Format::Xz => {
            let mut enc = xz2::write::XzEncoder::new(sink, opt.level.min(9) as u32);
            io::copy(&mut cr, &mut enc).map_err(|er| io_err(er, &e.src))?;
            enc.finish().map_err(|er| io_err(er, &e.src))?;
        }
        Format::Bz2 => {
            let mut enc = bzip2::write::BzEncoder::new(
                sink,
                bzip2::Compression::new(opt.level.clamp(1, 9) as u32),
            );
            io::copy(&mut cr, &mut enc).map_err(|er| io_err(er, &e.src))?;
            enc.finish().map_err(|er| io_err(er, &e.src))?;
        }
        _ => unreachable!("streams::write only handles gz/zst/xz/bz2"),
    }
    {
        let mut r = shared.borrow_mut();
        r.files_done += 1;
        r.report(true);
    }
    Ok(())
}

// ---- extract ---------------------------------------------------------------

pub(crate) fn extract<F>(
    archive: &Path,
    opt: &ExtractOptions,
    format: Format,
    shared: &Shared<F>,
    stats: &mut ExtractStats,
) -> Result<()>
where
    F: FnMut(&Progress) -> bool,
{
    let out_name = output_name(archive);
    let target = match opt.resolve(&out_name)? {
        TargetAction::Write(t) => t,
        TargetAction::Skip => {
            stats.skipped += 1;
            return Ok(());
        }
    };

    let f = File::open(archive).map_err(|e| io_err(e, archive))?;
    let br = CountingReader::new(BufReader::new(f), shared.clone());
    let out = File::create(&target).map_err(|e| io_err(e, &target))?;
    let mut cw = CountingWriter::new(BufWriter::new(out), shared.clone());
    let mut gz_mtime: Option<SystemTime> = None;
    let copied = match format {
        Format::Gz => {
            let mut dec = flate2::read::MultiGzDecoder::new(br);
            if let Some(h) = dec.header() {
                let mt = h.mtime();
                if mt > 0 {
                    gz_mtime = Some(system_time_from_unix(mt as u64));
                }
            }
            io::copy(&mut dec, &mut cw)
        }
        Format::Zst => {
            let mut dec = zstd::stream::read::Decoder::new(br).map_err(|e| io_err(e, archive))?;
            io::copy(&mut dec, &mut cw)
        }
        Format::Xz => {
            let mut dec = xz2::read::XzDecoder::new_multi_decoder(br);
            io::copy(&mut dec, &mut cw)
        }
        Format::Bz2 => {
            let mut dec = bzip2::read::BzDecoder::new(br);
            io::copy(&mut dec, &mut cw)
        }
        _ => unreachable!("streams::extract only handles gz/zst/xz/bz2"),
    };
    let out_file = match cw.into_inner().into_inner() {
        Ok(f) => f,
        Err(e) => return Err(io_err(e.into_error(), &target)),
    };
    match copied {
        Ok(nb) => stats.bytes += nb,
        Err(e) => return Err(io_err(e, &target)),
    }
    set_file_metadata(&out_file, gz_mtime, None);
    stats.files += 1;
    let mut r = shared.borrow_mut();
    r.files_done += 1;
    r.report(true);
    Ok(())
}

/// Output name for a bare stream: strip the known suffix, else `<name>.out`.
pub(crate) fn output_name(archive: &Path) -> String {
    let name = archive
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let lower = name.to_ascii_lowercase();
    for suffix in [".gz", ".zstd", ".zst", ".xz", ".bz2"] {
        if lower.ends_with(suffix) && name.len() > suffix.len() {
            return name[..name.len() - suffix.len()].to_string();
        }
    }
    format!("{name}.out")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_output_names() {
        assert_eq!(output_name(Path::new("log.txt.gz")), "log.txt");
        assert_eq!(output_name(Path::new("data.zst")), "data");
        assert_eq!(output_name(Path::new("data.zstd")), "data");
        assert_eq!(output_name(Path::new("backup.sql.xz")), "backup.sql");
        assert_eq!(output_name(Path::new("backup.sql.bz2")), "backup.sql");
        assert_eq!(output_name(Path::new("raw")), "raw.out");
    }
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use wispzip_core as core;
use wispzip_core::Phase;

/// Cancellation + pause handles for in-flight jobs, keyed by job id.
#[derive(Default, Clone)]
pub struct Jobs(Arc<Mutex<HashMap<u32, JobFlags>>>);

#[derive(Clone, Default)]
pub struct JobFlags {
    pub cancel: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
}

impl Jobs {
    fn cancel_of(&self, id: u32) -> Option<Arc<AtomicBool>> {
        self.0.lock().unwrap().get(&id).map(|f| f.cancel.clone())
    }
    fn pause_of(&self, id: u32) -> Option<Arc<AtomicBool>> {
        self.0.lock().unwrap().get(&id).map(|f| f.pause.clone())
    }
}

static NEXT_JOB: AtomicU32 = AtomicU32::new(1);

/// CLI-style launch intent: `wispzip-app c <path>` / `wispzip-app x <path>` from
/// shell context-menu entries. Captured before the webview loads so the UI
/// can jump straight into the matching confirm screen.
static LAUNCH_ARGS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Path of the file opened via Finder/doc open. On a cold start the Opened event may fire
/// before the frontend listener registers, so it is cached for take_opened_paths to claim later.
static PENDING_OPENED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CmdError {
    kind: String, // cancelled | password | unsafe_entry | unsupported | other
    message: String,
}

impl CmdError {
    fn other(msg: impl Into<String>) -> Self {
        Self {
            kind: "other".into(),
            message: msg.into(),
        }
    }
}

impl From<core::WispZipError> for CmdError {
    fn from(e: core::WispZipError) -> Self {
        let kind = match &e {
            core::WispZipError::Cancelled => "cancelled",
            core::WispZipError::PasswordRequired(_) => "password",
            core::WispZipError::UnsafeEntry(_) => "unsafe_entry",
            core::WispZipError::UnsupportedFormat(_) | core::WispZipError::UnknownFormat(_) => {
                "unsupported"
            }
            _ => "other",
        };
        Self {
            kind: kind.into(),
            message: e.to_string(),
        }
    }
}

fn phase_str(p: Phase) -> &'static str {
    match p {
        Phase::Prepare => "prepare",
        Phase::Compress => "compress",
        Phase::Extract => "extract",
        Phase::Finish => "finish",
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    job_id: u32,
    phase: &'static str,
    current_file: Option<String>,
    processed_bytes: u64,
    total_bytes: Option<u64>,
    output_bytes: u64,
    files_done: u64,
    files_total: Option<u64>,
}

impl ProgressEvent {
    fn new(job_id: u32, p: &core::Progress) -> Self {
        Self {
            job_id,
            phase: phase_str(p.phase),
            current_file: p.current_file.clone(),
            processed_bytes: p.processed_bytes,
            total_bytes: p.total_bytes,
            output_bytes: p.output_bytes,
            files_done: p.files_done,
            files_total: p.files_total,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobDoneEvent {
    job_id: u32,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<CmdError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
}

fn spawn_job<F>(app: AppHandle, jobs: &Jobs, work: F) -> u32
where
    F: FnOnce(u32, &JobFlags) -> Result<serde_json::Value, core::WispZipError>
        + Send
        + 'static,
{
    let job_id = NEXT_JOB.fetch_add(1, Ordering::SeqCst);
    let flags = JobFlags {
        cancel: core::new_cancel_flag(),
        pause: Arc::new(AtomicBool::new(false)),
    };
    jobs.0.lock().unwrap().insert(job_id, flags.clone());
    let jobs2 = jobs.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = work(job_id, &flags);
        jobs2.0.lock().unwrap().remove(&job_id);
        let event = match result {
            Ok(v) => JobDoneEvent {
                job_id,
                ok: true,
                error: None,
                result: Some(v),
            },
            Err(e) => {
                let _ = app.emit(
                    "wispzip:progress",
                    ProgressEvent::new(
                        job_id,
                        &core::Progress {
                            phase: Phase::Finish,
                            current_file: None,
                            processed_bytes: 0,
                            total_bytes: None,
                            output_bytes: 0,
                            files_done: 0,
                            files_total: None,
                        },
                    ),
                );
                JobDoneEvent {
                    job_id,
                    ok: false,
                    error: Some(e.into()),
                    result: None,
                }
            }
        };
        let _ = app.emit("wispzip:done", event);
    });
    job_id
}

// ---- commands ---------------------------------------------------------------

#[tauri::command]
async fn detect_archive(path: String) -> Result<serde_json::Value, CmdError> {
    let p = PathBuf::from(&path);
    tauri::async_runtime::spawn_blocking(move || {
        let (format, class) = core::detect(&p)?;
        let class = match class {
            core::Classification::Archive => "archive",
            core::Classification::SingleStream => "single_stream",
        };
        Ok(serde_json::json!({
            "format": serde_json::to_value(format).unwrap_or_default(),
            "classification": class,
            "supportsPassword": format.supports_password(),
            "levelRange": { "min": format.level_range().0, "max": format.level_range().1 },
        }))
    })
    .await
    .map_err(|e| CmdError::other(e.to_string()))?
}

#[tauri::command]
async fn list_archive(
    path: String,
    encoding: Option<String>,
    password: Option<String>,
) -> Result<Vec<core::EntryInfo>, CmdError> {
    let p = PathBuf::from(&path);
    tauri::async_runtime::spawn_blocking(move || {
        let enc = match encoding.as_deref() {
            Some("utf8") => core::NameEncoding::Utf8,
            Some("gbk") => core::NameEncoding::Gbk,
            _ => core::NameEncoding::Auto,
        };
        core::list_archive(&p, enc, password.as_deref()).map_err(Into::into)
    })
    .await
    .map_err(|e| CmdError::other(e.to_string()))?
}

/// Progress callback: emits to the webview, honors cancel and suspends the
/// worker while the job is paused.
fn make_progress(
    progress_app: AppHandle,
    job_id: u32,
    flags: &JobFlags,
) -> impl FnMut(&core::Progress) -> bool + '_ {
    move |p: &core::Progress| {
        let _ = progress_app.emit("wispzip:progress", ProgressEvent::new(job_id, p));
        while flags.pause.load(Ordering::SeqCst) && !flags.cancel.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(120));
        }
        !flags.cancel.load(Ordering::SeqCst)
    }
}

#[tauri::command]
fn scan_inputs(
    inputs: Vec<String>,
    exclude_hidden: Option<bool>,
) -> Result<serde_json::Value, CmdError> {
    let inputs: Vec<PathBuf> = inputs.into_iter().map(PathBuf::from).collect();
    let st = core::scan(&inputs, exclude_hidden.unwrap_or(false))?;
    Ok(serde_json::json!({
        "files": st.files,
        "dirs": st.dirs,
        "bytes": st.bytes,
        "skipped": st.skipped,
    }))
}

#[tauri::command]
fn launch_args() -> Vec<String> {
    LAUNCH_ARGS.get().cloned().unwrap_or_default()
}

/// Claim and clear the cached event paths (called once after the frontend listener registers).
#[tauri::command]
fn take_opened_paths() -> Vec<String> {
    let mut guard = PENDING_OPENED.lock().unwrap();
    let v = std::mem::take(&mut *guard);
    v
}

#[tauri::command]
fn pause_job(jobs: State<'_, Jobs>, job_id: u32) -> bool {
    jobs.pause_of(job_id)
        .map(|p| {
            p.store(true, Ordering::SeqCst);
            true
        })
        .unwrap_or(false)
}

#[tauri::command]
fn resume_job(jobs: State<'_, Jobs>, job_id: u32) -> bool {
    jobs.pause_of(job_id)
        .map(|p| {
            p.store(false, Ordering::SeqCst);
            true
        })
        .unwrap_or(false)
}

#[tauri::command]
async fn start_compress(
    app: AppHandle,
    jobs: State<'_, Jobs>,
    inputs: Vec<String>,
    out: String,
    level: Option<u8>,
    store_only: Option<bool>,
    password: Option<String>,
    split_bytes: Option<u64>,
    exclude_hidden: Option<bool>,
) -> Result<u32, CmdError> {
    let inputs: Vec<PathBuf> = inputs.into_iter().map(PathBuf::from).collect();
    let out = PathBuf::from(&out);
    let app2 = app.clone();
    let opt = core::CompressOptions {
        format: core::infer_output_format(&out)?,
        level: level.unwrap_or(6),
        store_only: store_only.unwrap_or(false),
        password,
        split: split_bytes,
        exclude_hidden: exclude_hidden.unwrap_or(false),
    };
    let jobs = jobs.inner().clone();
    let job_id = spawn_job(app, &jobs, move |job_id, flags| {
        let stats = core::compress(&inputs, &out, &opt, make_progress(app2, job_id, &flags))?;
        Ok(serde_json::json!({
            "inputFiles": stats.input_files,
            "inputBytes": stats.input_bytes,
            "outputBytes": stats.output_bytes,
            "skipped": stats.skipped,
            "outputPath": stats.output_path.to_string_lossy(),
        }))
    });
    Ok(job_id)
}

#[tauri::command]
async fn start_extract(
    app: AppHandle,
    jobs: State<'_, Jobs>,
    archive: String,
    dest: String,
    entries: Option<Vec<String>>,
    conflict: Option<String>,
    encoding: Option<String>,
    password: Option<String>,
    strip_prefix: Option<String>,
) -> Result<u32, CmdError> {
    let opt = core::ExtractOptions {
        dest: PathBuf::from(&dest),
        entries,
        conflict: match conflict.as_deref() {
            Some("skip") => core::Conflict::Skip,
            Some("overwrite") => core::Conflict::Overwrite,
            _ => core::Conflict::Rename,
        },
        encoding: match encoding.as_deref() {
            Some("utf8") => core::NameEncoding::Utf8,
            Some("gbk") => core::NameEncoding::Gbk,
            _ => core::NameEncoding::Auto,
        },
        password,
        strip_prefix,
    };
    let archive = PathBuf::from(&archive);
    let jobs = jobs.inner().clone();
    let app2 = app.clone();
    let job_id = spawn_job(app, &jobs, move |job_id, flags| {
        let stats = core::extract(&archive, &opt, make_progress(app2, job_id, &flags))?;
        Ok(serde_json::json!({
            "files": stats.files,
            "dirs": stats.dirs,
            "bytes": stats.bytes,
            "skipped": stats.skipped,
            "dest": stats.dest.to_string_lossy(),
        }))
    });
    Ok(job_id)
}

/// Install the Finder Quick Actions (services) that give wispzip right-click
/// Finder entries: files/folders get a compress-to-ZIP item, archives get an extract item.
/// Templates ship as resources with a __WISPZIP_BIN__ placeholder that is
/// rewritten to the running binary before install.
#[cfg(target_os = "macos")]
fn macos_install_services(app: &AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let res = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?;
    let candidates = [res.join("resources/services"), res.join("services")];
    let src = candidates.iter().find(|p| p.exists()).cloned().ok_or_else(|| {
        format!("service templates not found under {:?}", candidates)
    })?;
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    let dest_root = PathBuf::from(home).join("Library/Services");
    std::fs::create_dir_all(&dest_root).map_err(|e| e.to_string())?;

    for name in ["wispzip-compress.workflow", "wispzip-extract.workflow"] {
        let from = src.join(name);
        if !from.exists() {
            continue;
        }
        let to = dest_root.join(name);
        copy_dir_rewrite(&from, &to, &exe.to_string_lossy())?;
        // Register the workflow with LaunchServices so Finder picks it up
        // without a full reboot.  lsregister -f is more targeted than pbs.
        let lsregister = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
        let _ = std::process::Command::new(lsregister)
            .arg("-f")
            .arg(&to)
            .output();
    }
    // Also flush the pasteboard services cache as a fallback for older macOS.
    let _ = std::process::Command::new("/System/Library/CoreServices/pbs")
        .arg("-flush")
        .output();
    let _ = std::process::Command::new("/System/Library/CoreServices/pbs")
        .arg("-update")
        .output();
    Ok(())
}

#[cfg(target_os = "macos")]
fn copy_dir_rewrite(from: &std::path::Path, to: &std::path::Path, bin: &str) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let fpath = entry.path();
        let tpath = to.join(entry.file_name());
        if fpath.is_dir() {
            copy_dir_rewrite(&fpath, &tpath, bin)?;
        } else if fpath.extension().is_some_and(|e| e == "wflow" || e == "plist") {
            let content = std::fs::read_to_string(&fpath).map_err(|e| e.to_string())?;
            std::fs::write(&tpath, content.replace("__WISPZIP_BIN__", bin))
                .map_err(|e| e.to_string())?;
        } else {
            std::fs::copy(&fpath, &tpath).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
fn install_finder_services(app: AppHandle) -> Result<(), CmdError> {
    #[cfg(target_os = "macos")]
    {
        macos_install_services(&app).map_err(CmdError::other)?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err(CmdError::other("only supported on macOS"))
    }
}

#[tauri::command]
fn stat_path(path: String) -> Result<serde_json::Value, CmdError> {
    let p = PathBuf::from(&path);
    let meta = std::fs::metadata(&p).map_err(|e| core::WispZipError::Io { path: p.clone(), source: e })?;
    Ok(serde_json::json!({
        "size": meta.len(),
        "isDir": meta.is_dir(),
    }))
}

#[tauri::command]
fn cancel_job(jobs: State<'_, Jobs>, job_id: u32) -> bool {
    jobs.cancel_of(job_id)
        .map(|c| {
            c.store(true, Ordering::SeqCst);
            true
        })
        .unwrap_or(false)
}

#[tauri::command]
async fn pick_files(
    app: AppHandle,
    filters: Option<Vec<String>>,
) -> Result<Option<Vec<String>>, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut d = app.dialog().file();
        if let Some(exts) = filters {
            let ext_refs: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();
            d = d.add_filter("文件", &ext_refs);
        }
        Ok(d.blocking_pick_files()
            .map(|files| files.into_iter().map(|f| f.to_string()).collect()))
    })
    .await
    .map_err(|e| CmdError::other(e.to_string()))?
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Result<Option<String>, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .blocking_pick_folder()
            .map(|f| f.to_string()))
    })
    .await
    .map_err(|e| CmdError::other(e.to_string()))?
}

#[tauri::command]
async fn pick_save(
    app: AppHandle,
    file_name: String,
    filters: Option<Vec<String>>,
) -> Result<Option<String>, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut d = app.dialog().file().set_file_name(&file_name);
        if let Some(exts) = filters {
            let ext_refs: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();
            d = d.add_filter("文件", &ext_refs);
        }
        Ok(d.blocking_save_file().map(|f| f.to_string()))
    })
    .await
    .map_err(|e| CmdError::other(e.to_string()))?
}

#[tauri::command]
fn reveal_path(app: AppHandle, path: String) -> Result<(), CmdError> {
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| CmdError::other(e.to_string()))
}

#[tauri::command]
fn open_path(app: AppHandle, path: String) -> Result<(), CmdError> {
    app.opener()
        .open_path(&path, None::<&str>)
        .map_err(|e| CmdError::other(e.to_string()))
}

// ---- Archive browser: open a single entry / preview ---------------------------

const OPEN_TMP_ROOT: &str = "wispzip-open";
/// Text preview reads at most this many bytes (truncated flag tells the UI).
const PREVIEW_TEXT_MAX: u64 = 256 * 1024;
/// Images above this size are not inlined into the webview.
const PREVIEW_IMAGE_MAX: u64 = 32 * 1024 * 1024;

fn name_encoding(v: &Option<String>) -> core::NameEncoding {
    match v.as_deref() {
        Some("utf8") => core::NameEncoding::Utf8,
        Some("gbk") => core::NameEncoding::Gbk,
        _ => core::NameEncoding::Auto,
    }
}

fn tmp_unique_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir()
        .join(OPEN_TMP_ROOT)
        .join(format!("{}-{}", std::process::id(), nanos))
}

/// Extract a single entry into a fresh temp dir and return the produced file.
/// The core layer filters by entry name and stops early on selection, so even a huge archive
/// only decompresses up to the target entry; it never falls back to a full extraction.
fn extract_entry_to_temp(
    archive: &Path,
    entry: &str,
    encoding: core::NameEncoding,
    password: Option<&str>,
) -> Result<PathBuf, core::WispZipError> {
    let dir = tmp_unique_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| core::WispZipError::Io { path: dir.clone(), source: e })?;
    let opt = core::ExtractOptions {
        dest: dir.clone(),
        entries: Some(vec![entry.to_string()]),
        conflict: core::Conflict::Rename,
        encoding,
        password: password.map(|p| p.to_string()),
        strip_prefix: None,
    };
    let result = core::extract(archive, &opt, |_| true);
    match first_file_in(&dir) {
        Some(p) => Ok(p),
        None => {
            let err = result.err().unwrap_or(core::WispZipError::Other(
                "entry not found in archive".into(),
            ));
            let _ = std::fs::remove_dir_all(&dir);
            Err(err)
        }
    }
}

fn first_file_in(dir: &Path) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn is_image_ext(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "ico" => Some("image/x-icon"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewData {
    /// text | image | binary
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    mime: Option<String>,
    /// Text content (lossy) or base64 for images; empty for binary.
    content: String,
    truncated: bool,
    size: u64,
}

#[tauri::command]
async fn preview_entry(
    archive: String,
    entry: String,
    encoding: Option<String>,
    password: Option<String>,
) -> Result<PreviewData, CmdError> {
    let archive = PathBuf::from(&archive);
    tauri::async_runtime::spawn_blocking(move || {
        let path = extract_entry_to_temp(&archive, &entry, name_encoding(&encoding), password.as_deref())?;
        let io_err = |e: std::io::Error| CmdError::from(core::WispZipError::Io { path: path.clone(), source: e });
        let result = (|| -> Result<PreviewData, CmdError> {
            let size = std::fs::metadata(&path).map_err(io_err)?.len();
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some(mime) = is_image_ext(&name) {
                if size > PREVIEW_IMAGE_MAX {
                    return Ok(PreviewData { kind: "binary", mime: None, content: String::new(), truncated: false, size });
                }
                let bytes = std::fs::read(&path).map_err(io_err)?;
                return Ok(PreviewData {
                    kind: "image",
                    mime: Some(mime.into()),
                    content: base64::engine::general_purpose::STANDARD.encode(bytes),
                    truncated: false,
                    size,
                });
            }
            let mut f = std::io::BufReader::new(std::fs::File::open(&path).map_err(io_err)?);
            let mut buf = vec![0u8; PREVIEW_TEXT_MAX.min(size) as usize];
            let read = std::io::Read::read(&mut f, &mut buf).map_err(io_err)?;
            buf.truncate(read);
            if buf.contains(&0) {
                // binary payload: no text preview, UI offers open/extract instead
                return Ok(PreviewData { kind: "binary", mime: None, content: String::new(), truncated: false, size });
            }
            Ok(PreviewData {
                kind: "text",
                mime: None,
                content: String::from_utf8_lossy(&buf).into_owned(),
                truncated: size as usize > buf.len(),
                size,
            })
        })();
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(std::path::Path::new(&archive)));
        result
    })
    .await
    .map_err(|e| CmdError::other(e.to_string()))?
}

/// Double-click an entry: extract to a temp dir and hand it to the default app (temp files are cleaned up on next launch).
#[tauri::command]
async fn open_entry(
    app: AppHandle,
    archive: String,
    entry: String,
    encoding: Option<String>,
    password: Option<String>,
) -> Result<String, CmdError> {
    let archive = PathBuf::from(&archive);
    tauri::async_runtime::spawn_blocking(move || {
        let path = extract_entry_to_temp(&archive, &entry, name_encoding(&encoding), password.as_deref())?;
        app.opener()
            .open_path(path.to_string_lossy().into_owned(), None::<&str>)
            .map_err(|e| CmdError::other(e.to_string()))?;
        Ok(path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| CmdError::other(e.to_string()))?
}


#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Manager;

    // capture CLI intent before anything else
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let _ = LAUNCH_ARGS.set(argv.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // second launch: forward its intent to the running instance
            let _ = app.emit("wispzip:launch-args", argv[1..].to_vec());
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .manage(Jobs::default())
        .invoke_handler(tauri::generate_handler![
            detect_archive,
            list_archive,
            start_compress,
            start_extract,
            cancel_job,
            pick_files,
            pick_folder,
            pick_save,
            reveal_path,
            open_path,
            open_entry,
            preview_entry,
            stat_path,
            app_version,
            scan_inputs,
            launch_args,
            take_opened_paths,
            pause_job,
            resume_job,
            install_finder_services,
        ])
        .setup(|app| {
            // Leftover "open entry" temp files from the previous session; if an external app holds them, failing to delete is fine
            let _ = std::fs::remove_dir_all(std::env::temp_dir().join(OPEN_TMP_ROOT));
            #[cfg(target_os = "macos")]
            {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    let _ = macos_install_services(&handle);
                });
            }

            // macOS: top menu bar -- includes About / Settings / Quit.
            // Clicking "Settings..." sends a `wispzip:open-settings` event; Vue opens the settings dialog.
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
                let settings_item = MenuItem::with_id(
                    app,
                    "settings",
                    "设置…",
                    true,
                    Some("Cmd+,"),
                )?;
                let app_submenu = Submenu::with_id_and_items(
                    app,
                    "app",
                    "wispzip",
                    true,
                    &[
                        &PredefinedMenuItem::about(app, Some("关于 一压"), None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &settings_item,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::services(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::hide(app, None)?,
                        &PredefinedMenuItem::hide_others(app, None)?,
                        &PredefinedMenuItem::show_all(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::quit(app, None)?,
                    ],
                )?;
                let window_submenu = Submenu::with_id_and_items(
                    app,
                    "window",
                    "窗口",
                    true,
                    &[
                        &PredefinedMenuItem::close_window(app, Some("关闭窗口"))?,
                        &PredefinedMenuItem::minimize(app, None)?,
                        &PredefinedMenuItem::maximize(app, None)?,
                        &PredefinedMenuItem::fullscreen(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::bring_all_to_front(app, None)?,
                    ],
                )?;
                let app_menu = Menu::with_items(app, &[&app_submenu, &window_submenu])?;
                app.set_menu(app_menu)?;

                let menu_handle = app.handle().clone();
                app.on_menu_event(move |app_handle, ev| match ev.id.as_ref() {
                    "settings" => {
                        let _ = app_handle.emit("wispzip:open-settings", ());
                        if let Some(w) = menu_handle.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    _ => {}
                });
            }

            use tauri::menu::{Menu, MenuItem};
            use tauri::tray::TrayIconBuilder;
            let show = MenuItem::with_id(app, "show", "显示 一压", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出 一压", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::with_id("wispzip-tray")
                .icon(app.default_window_icon().expect("app icon missing").clone())
                .tooltip("WispZip")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, ev| match ev.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // closing the window keeps wispzip alive in the tray; quit via tray menu
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running wispzip")
        .run(|app, event| {
            // macOS: Finder file-association open + Dock icon click (brings back the window after close-to-tray)
            // Windows: file association goes through the launch-argument intent registered by the installer
            #[cfg(target_os = "macos")]
            match event {
                tauri::RunEvent::Reopen { .. } => {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
                tauri::RunEvent::Opened { urls } => {
                    let paths: Vec<String> = urls
                        .iter()
                        .filter_map(|u| u.to_file_path().ok())
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect();
                    if !paths.is_empty() {
                        PENDING_OPENED
                            .lock()
                            .unwrap()
                            .extend(paths.iter().cloned());
                        let _ = app.emit("wispzip:open-file", paths.clone());
                        // Cold-start race fallback: the listener registered late, so retry once after a delay
                        let h = app.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(1200));
                            let _ = h.emit("wispzip:open-file", paths);
                        });
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                }
                _ => {}
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (app, event);
            }
        });
}

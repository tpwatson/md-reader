mod assoc;
mod markdown;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use markdown::{Document, Loaded};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{Emitter, Manager};

struct ReaderState {
    current: Mutex<Option<PathBuf>>,
    watcher: Mutex<Option<RecommendedWatcher>>,
    pending: Mutex<Option<String>>,
}

impl ReaderState {
    fn new() -> Self {
        Self {
            current: Mutex::new(None),
            watcher: Mutex::new(None),
            pending: Mutex::new(None),
        }
    }
}

#[tauri::command]
fn open_path(app: tauri::AppHandle, path: String) -> Result<Document, String> {
    let loaded = markdown::load(&path)?;
    allow_assets(&app, &loaded);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(&format!("{} — MD Reader", loaded.document.name));
    }
    *app.state::<ReaderState>().current.lock().unwrap() = Some(PathBuf::from(&loaded.document.path));
    watch_file(&app, Path::new(&loaded.document.path));
    Ok(loaded.document)
}

#[tauri::command]
fn pending_open(state: tauri::State<'_, ReaderState>) -> Option<String> {
    state.pending.lock().unwrap().take()
}

#[tauri::command]
fn resolve_href(current_file: String, href: String) -> Result<String, String> {
    markdown::resolve_href(&current_file, &href)
}

#[tauri::command]
fn open_external(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let url = url.trim();
    let allowed = url.starts_with("https://")
        || url.starts_with("http://")
        || url.to_ascii_lowercase().starts_with("mailto:");
    if !allowed {
        return Err("Only web and mail links open outside the reader.".into());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn register_default() -> Result<String, String> {
    assoc::register_and_prompt()
}

fn allow_assets(app: &tauri::AppHandle, loaded: &Loaded) {
    let scope = app.asset_protocol_scope();
    let _ = scope.allow_directory(&loaded.directory, true);
    for image in &loaded.images {
        let _ = scope.allow_file(image);
        if let Some(parent) = image.parent() {
            let _ = scope.allow_directory(parent, false);
        }
    }
}

fn watch_file(app: &tauri::AppHandle, file: &Path) {
    let Some(parent) = file.parent() else {
        return;
    };
    if parent.as_os_str().is_empty() {
        return;
    }
    let expected_name = file.file_name().map(|name| name.to_os_string());
    let watched_path = file.to_string_lossy().into_owned();
    let app_handle = app.clone();
    let counter = Arc::new(AtomicU64::new(0));

    let watcher = RecommendedWatcher::new(
        move |result: Result<notify::Event, notify::Error>| {
            let Ok(event) = result else {
                return;
            };
            let Some(expected_name) = expected_name.as_ref() else {
                return;
            };
            let matched = event.paths.iter().any(|path| {
                path.file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case(expected_name))
            });
            if !matched {
                return;
            }
            if !matches!(
                event.kind,
                EventKind::Modify(_) | EventKind::Create(_) | EventKind::Any
            ) {
                return;
            }

            let ticket = counter.fetch_add(1, Ordering::SeqCst) + 1;
            let app_handle = app_handle.clone();
            let counter = Arc::clone(&counter);
            let watched_path = watched_path.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(180));
                if counter.load(Ordering::SeqCst) == ticket {
                    let _ = app_handle.emit("file-changed", watched_path);
                }
            });
        },
        Config::default(),
    );

    let Ok(mut watcher) = watcher else {
        return;
    };
    if watcher.watch(parent, RecursiveMode::NonRecursive).is_err() {
        return;
    }

    let state = app.state::<ReaderState>();
    let mut slot = state.watcher.lock().unwrap();
    *slot = None;
    *slot = Some(watcher);
}

fn file_from_args(args: &[String]) -> Option<String> {
    let exe = std::env::current_exe().ok();
    args.iter().find_map(|arg| {
        let trimmed = arg.trim().trim_matches('"');
        if trimmed.is_empty() || trimmed.starts_with('-') {
            return None;
        }
        let path = PathBuf::from(trimmed);
        if let Some(exe) = &exe {
            if same_file(&path, exe) {
                return None;
            }
        }
        path.is_file().then(|| path.to_string_lossy().into_owned())
    })
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => markdown::plain_path(left) == markdown::plain_path(right),
    }
}

fn reveal(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(desktop)]
fn attach_single_instance(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        if let Some(path) = file_from_args(&args) {
            if let Some(state) = app.try_state::<ReaderState>() {
                *state.pending.lock().unwrap() = Some(path.clone());
            }
            let _ = app.emit("open-file", path);
        }
        reveal(&app);
    }))
}

#[cfg(not(desktop))]
fn attach_single_instance(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    attach_single_instance(tauri::Builder::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(ReaderState::new())
        .setup(|app| {
            if let Some(path) = file_from_args(&std::env::args().collect::<Vec<_>>()) {
                *app.state::<ReaderState>().pending.lock().unwrap() = Some(path);
            }
            #[cfg(all(windows, not(debug_assertions)))]
            {
                let _ = assoc::register_current_exe();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_path,
            pending_open,
            resolve_href,
            open_external,
            register_default
        ])
        .run(tauri::generate_context!())
        .expect("error while running MD Reader");
}

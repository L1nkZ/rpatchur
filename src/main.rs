#![windows_subsystem = "windows"]

mod config;
mod grf;
mod patching;
mod thor;

use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, prelude::*, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;

use config::{parse_configuration, PatcherConfiguration};
use futures::executor::block_on;
use patching::{apply_patch_to_disk, apply_patch_to_grf, GrfPatchingMethod};
use serde::{Deserialize, Serialize};
use thor::{ThorArchive, ThorPatchList};
use tokio::{runtime, sync::mpsc};
use url::Url;
use web_view::{Content, Handle, WebView};

enum PatchingThreadCommand {
    Start,
    Cancel, // Canceled by the user
    Exit,   // Program is exiting
}

enum Interruption {
    Cancel, // Cancel current task
    Exit,   // Kill the worker
}
enum InterruptibleFnError {
    Err(String),               // An actual error
    Interrupted(Interruption), // An interruption
}

struct WebViewUserData {
    patcher_config: PatcherConfiguration,
    patching_thread_tx: mpsc::Sender<PatchingThreadCommand>,
}
impl Drop for WebViewUserData {
    fn drop(&mut self) {
        // Ask the patching thread to stop whenever WebViewUserData is dropped
        let _res = self
            .patching_thread_tx
            .try_send(PatchingThreadCommand::Exit);
    }
}

#[derive(Debug)]
struct PendingPatch {
    info: thor::ThorPatchInfo,
    local_file: File,
}

enum PatchingStatus {
    Ready,
    Error(String),                        // Error message
    DownloadInProgress(usize, usize),     // Downloaded, Total
    InstallationInProgress(usize, usize), // Installed, Total
}

#[derive(Serialize, Deserialize)]
struct PatcherCache {
    pub last_patch_index: usize,
}

fn main() {
    simple_logger::init().expect("Failed to initalize the logger");
    let mut tokio_rt = build_tokio_runtime().expect("Failed to build a tokio runtime");
    let config = match retrieve_configuration() {
        None => {
            log::error!("Failed to retrieve the patcher's configuration");
            return;
        }
        Some(v) => v,
    };
    // Create a channel to allow the webview's thread to communicate with the patching thread
    let (tx, rx) = mpsc::channel::<PatchingThreadCommand>(8);
    let webview = build_webview(config.clone(), tx).expect("Failed to build a web view");
    let patching_task = tokio_rt.spawn(patching_thread_routine(webview.handle(), rx, config));
    webview.run().unwrap();
    // Join the patching task from our synchronous function
    tokio_rt.block_on(async {
        if let Err(e) = patching_task.await {
            log::error!("Failed to join patching thread: {}", e);
        }
    });
}

/// Builds a tokio runtime with a threaded scheduler and a reactor
fn build_tokio_runtime() -> io::Result<runtime::Runtime> {
    runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .build()
}

fn retrieve_configuration() -> Option<PatcherConfiguration> {
    let patcher_name = get_patcher_name()?;
    let configuration_file_name = PathBuf::from(patcher_name).with_extension("json");
    // Read the JSON contents of the file as an instance of `PatcherConfiguration`.
    parse_configuration(configuration_file_name)
}

fn get_patcher_name() -> Option<OsString> {
    match env::current_exe() {
        Err(_) => None,
        Ok(v) => match v.file_stem() {
            Some(v) => Some(v.to_os_string()),
            None => None,
        },
    }
}

fn build_webview<'a>(
    config: PatcherConfiguration,
    patching_thread_tx: mpsc::Sender<PatchingThreadCommand>,
) -> web_view::WVResult<WebView<'a, WebViewUserData>> {
    web_view::builder()
        .title("RPatchur")
        .content(Content::Url(config.web.index_url.clone()))
        .size(config.window.width, config.window.height)
        .resizable(config.window.resizable)
        .user_data(WebViewUserData {
            patcher_config: config,
            patching_thread_tx,
        })
        .invoke_handler(|webview, arg| {
            match arg {
                "play" => handle_play(webview),
                "setup" => handle_setup(webview),
                "exit" => handle_exit(webview),
                "start_update" => handle_start_update(webview),
                "cancel_update" => handle_cancel_update(webview),
                "reset_cache" => handle_reset_cache(webview),
                _ => (),
            }
            Ok(())
        })
        .build()
}

/// Opens the configured client with arguments, if needed
fn handle_play(webview: &mut WebView<WebViewUserData>) {
    let client_exe: &String = &webview.user_data().patcher_config.play.path;
    let client_argument: &String = &webview.user_data().patcher_config.play.argument;
    if cfg!(target_os = "windows") {
        #[cfg(windows)]
        match windows::spawn_elevated_win32_process(client_exe, client_argument) {
            Ok(_) => log::trace!("Client started."),
            Err(e) => {
                log::warn!("Failed to start client: {}", e);
            }
        }
    } else {
        match Command::new(client_exe).arg(client_argument).spawn() {
            Ok(child) => log::trace!("Client started: pid={}", child.id()),
            Err(e) => {
                log::warn!("Failed to start client: {}", e);
            }
        }
    }
}

/// Opens the configured 'Setup' software
fn handle_setup(webview: &mut WebView<WebViewUserData>) {
    let setup_exe: &String = &webview.user_data().patcher_config.setup.path;
    let setup_argument: &String = &webview.user_data().patcher_config.setup.argument;
    if cfg!(target_os = "windows") {
        #[cfg(windows)]
        match windows::spawn_elevated_win32_process(setup_exe, setup_argument) {
            Ok(_) => log::trace!("Setup software started."),
            Err(e) => {
                log::warn!("Failed to start setup software: {}", e);
            }
        }
    } else {
        match Command::new(setup_exe).arg(setup_argument).spawn() {
            Ok(child) => log::trace!("Setup software started: pid={}", child.id()),
            Err(e) => {
                log::warn!("Failed to start setup software: {}", e);
            }
        }
    }
}

/// Exits the patcher cleanly
fn handle_exit(webview: &mut WebView<WebViewUserData>) {
    webview.exit();
}

/// Starts the update process
fn handle_start_update(webview: &mut WebView<WebViewUserData>) {
    if let Ok(_) = block_on(
        webview
            .user_data_mut()
            .patching_thread_tx
            .send(PatchingThreadCommand::Start),
    ) {
        log::trace!("Sent start command to patching thread");
    }
}

/// Cancels the update process
fn handle_cancel_update(webview: &mut WebView<WebViewUserData>) {
    if let Ok(_) = block_on(
        webview
            .user_data_mut()
            .patching_thread_tx
            .send(PatchingThreadCommand::Cancel),
    ) {
        log::trace!("Sent cancel command to patching thread");
    }
}

/// Resets the cache used to keep track of already applied patches
fn handle_reset_cache(_webview: &mut WebView<WebViewUserData>) {
    if let Some(patcher_name) = get_patcher_name() {
        let cache_file_path = PathBuf::from(patcher_name).with_extension("dat");
        if let Err(e) = fs::remove_file(cache_file_path) {
            log::warn!("Failed to remove the cache file: {}", e);
        }
    }
}

async fn wait_for_cancellation(
    patching_thread_rx: &mut mpsc::Receiver<PatchingThreadCommand>,
) -> InterruptibleFnError {
    if let Some(cmd) = patching_thread_rx.recv().await {
        match cmd {
            PatchingThreadCommand::Cancel => {
                return InterruptibleFnError::Interrupted(Interruption::Cancel)
            }
            PatchingThreadCommand::Exit => {
                return InterruptibleFnError::Interrupted(Interruption::Exit)
            }
            _ => return InterruptibleFnError::Err("Unexpected command received".to_string()),
        }
    }
    return InterruptibleFnError::Err("Channel was closed".to_string());
}

fn check_for_cancellation(
    patching_thread_rx: &mut mpsc::Receiver<PatchingThreadCommand>,
) -> Option<InterruptibleFnError> {
    if let Ok(cmd) = patching_thread_rx.try_recv() {
        match cmd {
            PatchingThreadCommand::Cancel => {
                return Some(InterruptibleFnError::Interrupted(Interruption::Cancel))
            }
            PatchingThreadCommand::Exit => {
                return Some(InterruptibleFnError::Interrupted(Interruption::Exit))
            }
            _ => return None,
        }
    }
    None
}

async fn patching_thread_routine(
    webview_handle: Handle<WebViewUserData>,
    mut patching_thread_rx: mpsc::Receiver<PatchingThreadCommand>,
    config: PatcherConfiguration,
) {
    log::trace!("Patching thread started.");
    let report_error = |err_msg| async {
        log::error!("{}", err_msg);
        dispatch_patching_status(&webview_handle, PatchingStatus::Error(err_msg)).await;
    };

    log::trace!("Waiting for commands");
    loop {
        match patching_thread_rx.recv().await {
            None => {
                log::error!("Channel has been closed");
                return;
            }
            Some(v) => match v {
                PatchingThreadCommand::Start => break,
                PatchingThreadCommand::Exit => {
                    log::info!("Exit command received");
                    return;
                }
                _ => {}
            },
        }
    }
    log::info!("Patching started");

    let patch_list_url = Url::parse(config.web.plist_url.as_str()).unwrap();
    let mut patch_list = match fetch_patch_list(patch_list_url).await {
        Err(e) => {
            report_error(format!("Failed to retrieve the patch list: {}.", e)).await;
            return;
        }
        Ok(v) => v,
    };
    log::info!("Successfully fetched patch list: {:?}", patch_list);

    // Try to read cache
    let cache_file_path = match get_patcher_name() {
        Some(patcher_name) => PathBuf::from(patcher_name).with_extension("dat"),
        None => {
            report_error("Failed to resolve patcher name.".to_string()).await;
            return;
        }
    };
    if let Ok(patcher_cache) = read_cache_file(&cache_file_path).await {
        // Ignore already applied patches if needed
        // First we verify that our cached index looks relevant
        let should_filter_patch_list = patch_list
            .iter()
            .any(|x| x.index == patcher_cache.last_patch_index);
        if should_filter_patch_list {
            patch_list.retain(|x| x.index > patcher_cache.last_patch_index);
        }
    };

    // Try fetching patch files
    log::info!("Downloading patches... ");
    let patch_url = Url::parse(config.web.patch_url.as_str()).unwrap();
    let pending_patch_queue = match download_patches(
        patch_url,
        patch_list,
        &webview_handle,
        &mut patching_thread_rx,
    )
    .await
    {
        Err(e) => match e {
            InterruptibleFnError::Err(msg) => {
                report_error(format!("Failed to download patches: {}.", msg)).await;
                return;
            }
            InterruptibleFnError::Interrupted(i) => match i {
                Interruption::Exit => {
                    log::info!("Exit command received");
                    return;
                }
                Interruption::Cancel => {
                    report_error("Patching was canceled".to_string()).await;
                    return;
                }
            },
        },
        Ok(v) => v,
    };
    log::info!("Done");

    // Proceed with actual patching
    log::info!("Applying patches...");
    if let Err(e) = apply_patches(
        pending_patch_queue,
        &webview_handle,
        &config,
        &cache_file_path,
        &mut patching_thread_rx,
    )
    .await
    {
        match e {
            InterruptibleFnError::Err(msg) => {
                report_error(format!("Failed to apply patches: {}.", msg)).await;
                return;
            }
            InterruptibleFnError::Interrupted(i) => match i {
                Interruption::Exit => {
                    log::info!("Exit command received");
                    return;
                }
                Interruption::Cancel => {
                    report_error("Patching was canceled".to_string()).await;
                    return;
                }
            },
        }
    }
    dispatch_patching_status(&webview_handle, PatchingStatus::Ready).await;
    log::info!("Patching finished!");
}

async fn fetch_patch_list(patch_list_url: Url) -> Result<ThorPatchList, String> {
    let resp = match reqwest::get(patch_list_url).await {
        Err(e) => {
            return Err(format!("Failed to retrieve the patch list: {}", e));
        }
        Ok(v) => v,
    };
    if !resp.status().is_success() {
        return Err("Patch list file not found on the remote server".to_string());
    }
    let patch_index_content = match resp.text().await {
        Err(_) => return Err("Invalid responde body".to_string()),
        Ok(v) => v,
    };
    log::info!("Parsing patch index...");
    Ok(thor::patch_list_from_string(patch_index_content.as_str()))
}

async fn download_patches(
    patch_url: Url,
    patch_list: ThorPatchList,
    webview_handle: &Handle<WebViewUserData>,
    patching_thread_rx: &mut mpsc::Receiver<PatchingThreadCommand>,
) -> Result<Vec<PendingPatch>, InterruptibleFnError> {
    let patch_count = patch_list.len();
    let mut pending_patch_queue = Vec::with_capacity(patch_count);
    dispatch_patching_status(
        &webview_handle,
        PatchingStatus::DownloadInProgress(0, patch_count),
    )
    .await;
    for (patch_number, patch) in patch_list.into_iter().enumerate() {
        let patch_file_url = match patch_url.join(patch.file_name.as_str()) {
            Err(_) => {
                return Err(InterruptibleFnError::Err(format!(
                    "Invalid file name '{}' given in patch list file.",
                    patch.file_name
                )));
            }
            Ok(v) => v,
        };
        let mut tmp_file = tempfile::tempfile().unwrap();
        let download_future = async {
            let resp = match reqwest::get(patch_file_url).await {
                Err(e) => {
                    return Err(format!(
                        "Failed to download file '{}': {}.",
                        patch.file_name, e
                    ))
                }
                Ok(v) => v,
            };
            if !resp.status().is_success() {
                return Err(format!(
                    "Patch file '{}' not found on the remote server.",
                    patch.file_name
                ));
            }
            let resp_body = match resp.text().await {
                Err(e) => {
                    return Err(format!(
                        "Failed to download file '{}': {}.",
                        patch.file_name, e
                    ))
                }
                Ok(v) => v,
            };
            let _bytes_copied = match io::copy(&mut resp_body.as_bytes(), &mut tmp_file) {
                Err(e) => {
                    return Err(format!(
                        "Failed to download file '{}': {}.",
                        patch.file_name, e
                    ));
                }
                Ok(v) => v,
            };
            Ok(())
        };
        let cancel_future = wait_for_cancellation(patching_thread_rx);
        // Download file in a cancelable manner
        tokio::select! {
            cancel_res = cancel_future => return Err(cancel_res),
            _ = download_future => {},
        }

        // File's been downloaded, seek to start and add it to the queue
        let _offset = tmp_file.seek(SeekFrom::Start(0));
        pending_patch_queue.push(PendingPatch {
            info: patch,
            local_file: tmp_file,
        });
        // Update status
        dispatch_patching_status(
            &webview_handle,
            PatchingStatus::DownloadInProgress(patch_number, patch_count),
        )
        .await;
    }
    Ok(pending_patch_queue)
}

async fn apply_patches<P: AsRef<Path>>(
    pending_patch_queue: Vec<PendingPatch>,
    webview_handle: &Handle<WebViewUserData>,
    config: &PatcherConfiguration,
    cache_file_path: P,
    patching_thread_rx: &mut mpsc::Receiver<PatchingThreadCommand>,
) -> Result<(), InterruptibleFnError> {
    let current_working_dir = match env::current_dir() {
        Err(e) => {
            return Err(InterruptibleFnError::Err(format!(
                "Failed to resolve current working directory: {}.",
                e
            )));
        }
        Ok(v) => v,
    };
    let patch_count = pending_patch_queue.len();
    dispatch_patching_status(
        &webview_handle,
        PatchingStatus::InstallationInProgress(0, patch_count),
    )
    .await;
    for (patch_number, pending_patch) in pending_patch_queue.into_iter().enumerate() {
        // Cancel the patching process if we've been asked to
        if let Some(i) = check_for_cancellation(patching_thread_rx) {
            return Err(i);
        }
        log::info!("Processing {}", pending_patch.info.file_name);
        let mut thor_archive = match ThorArchive::new(pending_patch.local_file) {
            Err(e) => {
                return Err(InterruptibleFnError::Err(format!(
                    "Cannot read '{}': {}.",
                    pending_patch.info.file_name, e
                )));
            }
            Ok(v) => v,
        };

        if thor_archive.use_grf_merging() {
            // Patch GRF file
            let patch_target_grf_name = {
                if thor_archive.target_grf_name().is_empty() {
                    config.client.default_grf_name.clone()
                } else {
                    thor_archive.target_grf_name()
                }
            };
            log::trace!("Target GRF: {:?}", patch_target_grf_name);
            let grf_patching_method = match config.patching.in_place {
                true => GrfPatchingMethod::InPlace,
                false => GrfPatchingMethod::OutOfPlace,
            };
            if let Err(e) = apply_patch_to_grf(
                grf_patching_method,
                current_working_dir.join(&patch_target_grf_name),
                &mut thor_archive,
            ) {
                return Err(InterruptibleFnError::Err(format!(
                    "Failed to patch '{}': {}.",
                    patch_target_grf_name, e
                )));
            }
        } else {
            // Patch root directory
            if let Err(e) = apply_patch_to_disk(&current_working_dir, &mut thor_archive) {
                return Err(InterruptibleFnError::Err(format!(
                    "Failed to apply patch '{}': {}.",
                    pending_patch.info.file_name, e
                )));
            }
        }
        // Update the cache file with the last successful patch's index
        if let Err(e) = write_cache_file(
            &cache_file_path,
            PatcherCache {
                last_patch_index: pending_patch.info.index,
            },
        )
        .await
        {
            log::warn!("Failed to write cache file: {}.", e);
        }
        // Update status
        dispatch_patching_status(
            &webview_handle,
            PatchingStatus::InstallationInProgress(patch_number, patch_count),
        )
        .await;
    }
    Ok(())
}

async fn dispatch_patching_status(
    webview_handle: &Handle<WebViewUserData>,
    status: PatchingStatus,
) {
    if let Err(e) = webview_handle.dispatch(move |webview| {
        let result = match status {
            PatchingStatus::Ready => webview.eval("patchingStatusReady()"),
            PatchingStatus::Error(msg) => {
                webview.eval(&format!("patchingStatusError(\"{}\")", msg))
            }
            PatchingStatus::DownloadInProgress(nb_downloaded, nb_total) => webview.eval(&format!(
                "patchingStatusDownloading({}, {})",
                nb_downloaded, nb_total
            )),
            PatchingStatus::InstallationInProgress(nb_installed, nb_total) => webview.eval(
                &format!("patchingStatusInstalling({}, {})", nb_installed, nb_total),
            ),
        };
        if let Err(e) = result {
            log::warn!("Failed to dispatch patching status: {}.", e);
        }
        Ok(())
    }) {
        log::warn!("Failed to dispatch patching status: {}.", e);
    }
}

async fn read_cache_file<P: AsRef<Path>>(cache_file_path: P) -> io::Result<PatcherCache> {
    let file = File::open(cache_file_path)?;
    match bincode::deserialize_from(file) {
        Ok(v) => Ok(v),
        Err(e) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to deserialize patcher cache: {}", e),
        )),
    }
}

async fn write_cache_file<P: AsRef<Path>>(
    cache_file_path: P,
    new_cache: PatcherCache,
) -> io::Result<()> {
    let file = File::create(cache_file_path)?;
    match bincode::serialize_into(file, &new_cache) {
        Ok(_) => Ok(()),
        Err(e) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to serialize patcher cache: {}", e),
        )),
    }
}

// Taken from the rustup project
#[cfg(windows)]
mod windows {
    use std::ffi::OsStr;
    use std::io;
    use std::os::windows::ffi::OsStrExt;

    fn to_u16s<S: AsRef<OsStr>>(s: S) -> io::Result<Vec<u16>> {
        fn inner(s: &OsStr) -> io::Result<Vec<u16>> {
            let mut maybe_result: Vec<u16> = s.encode_wide().collect();
            if maybe_result.iter().any(|&u| u == 0) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "strings passed to WinAPI cannot contain NULs",
                ));
            }
            maybe_result.push(0);
            Ok(maybe_result)
        }
        inner(s.as_ref())
    }

    // This function is required to start processes that require elevation from
    // a non-elevated process
    pub fn spawn_elevated_win32_process<S: AsRef<OsStr>>(
        path: S,
        parameter: S,
    ) -> io::Result<bool> {
        use std::ptr;
        use winapi::ctypes::c_int;
        use winapi::shared::minwindef::HINSTANCE;
        use winapi::shared::ntdef::LPCWSTR;
        use winapi::shared::windef::HWND;
        extern "system" {
            pub fn ShellExecuteW(
                hwnd: HWND,
                lpOperation: LPCWSTR,
                lpFile: LPCWSTR,
                lpParameters: LPCWSTR,
                lpDirectory: LPCWSTR,
                nShowCmd: c_int,
            ) -> HINSTANCE;
        }
        const SW_SHOW: c_int = 5;

        let path = to_u16s(path)?;
        let parameter = to_u16s(parameter)?;
        let operation = to_u16s("runas")?;
        let result = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                operation.as_ptr(),
                path.as_ptr(),
                parameter.as_ptr(),
                ptr::null(),
                SW_SHOW,
            )
        };
        Ok(result as usize > 32)
    }
}

use std::fs;
use std::include_str;
use std::path::PathBuf;
use std::process::Command;

use crate::patcher::{get_patcher_name, PatcherCommand, PatcherConfiguration};
use futures::executor::block_on;
use tokio::sync::mpsc;
use web_view::{Content, Handle, WebView};

/// 'Opaque" struct that can be used to update the UI.
pub struct UIController {
    web_view_handle: Handle<WebViewUserData>,
}
impl UIController {
    pub fn new<'a>(web_view: &WebView<'a, WebViewUserData>) -> UIController {
        UIController {
            web_view_handle: web_view.handle(),
        }
    }

    /// Allows another thread to indicate the current status of the patching process.
    ///
    /// This updates the UI with useful information.
    pub async fn dispatch_patching_status(&self, status: PatchingStatus) {
        if let Err(e) = self.web_view_handle.dispatch(move |webview| {
            let result = match status {
                PatchingStatus::Ready => webview.eval("patchingStatusReady()"),
                PatchingStatus::Error(msg) => {
                    webview.eval(&format!("patchingStatusError(\"{}\")", msg))
                }
                PatchingStatus::DownloadInProgress(nb_downloaded, nb_total, bytes_per_sec) => {
                    webview.eval(&format!(
                        "patchingStatusDownloading({}, {}, {})",
                        nb_downloaded, nb_total, bytes_per_sec
                    ))
                }
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
}

/// Used to indicate the current status of the patching process.
pub enum PatchingStatus {
    Ready,
    Error(String),                         // Error message
    DownloadInProgress(usize, usize, u64), // Downloaded files, Total number, Bytes per second
    InstallationInProgress(usize, usize),  // Installed patches, Total number
}

pub struct WebViewUserData {
    patcher_config: PatcherConfiguration,
    patching_thread_tx: mpsc::Sender<PatcherCommand>,
}
impl WebViewUserData {
    pub fn new(
        patcher_config: PatcherConfiguration,
        patching_thread_tx: mpsc::Sender<PatcherCommand>,
    ) -> WebViewUserData {
        WebViewUserData {
            patcher_config,
            patching_thread_tx,
        }
    }
}
impl Drop for WebViewUserData {
    fn drop(&mut self) {
        // Ask the patching thread to stop whenever WebViewUserData is dropped
        let _res = self.patching_thread_tx.try_send(PatcherCommand::Cancel);
    }
}

/// Creates a message box with the given title and message.
///
/// Panics in case of error.
pub fn msg_box<'a>(title: &'a str, message: &'a str) {
    let html_template = include_str!("../resources/msg_box.html");
    let content = html_template.replace("MSG_BOX_MESSAGE", message);
    let webview = web_view::builder()
        .title(title)
        .content(Content::Html(content))
        .user_data(0)
        .size(250, 80)
        .resizable(false)
        .invoke_handler(|_, _| Ok(()))
        .build()
        .unwrap();
    webview.run().unwrap();
}

/// Creates a `WebView` object with the appropriate settings for our needs.
pub fn build_webview<'a>(
    window_title: &'a str,
    user_data: WebViewUserData,
) -> web_view::WVResult<WebView<'a, WebViewUserData>> {
    web_view::builder()
        .title(window_title)
        .content(Content::Url(user_data.patcher_config.web.index_url.clone()))
        .size(
            user_data.patcher_config.window.width,
            user_data.patcher_config.window.height,
        )
        .resizable(user_data.patcher_config.window.resizable)
        .user_data(user_data)
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

/// Opens the configured game client with the configured arguments.
///
/// This function can create elevated processes on Windows with UAC activated.
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

/// Opens the configured 'Setup' software with the configured arguments.
///
/// This function can create elevated processes on Windows with UAC activated.
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

/// Exits the patcher cleanly.
fn handle_exit(webview: &mut WebView<WebViewUserData>) {
    webview.exit();
}

/// Starts the patching task/thread.
fn handle_start_update(webview: &mut WebView<WebViewUserData>) {
    if block_on(
        webview
            .user_data_mut()
            .patching_thread_tx
            .send(PatcherCommand::Start),
    )
    .is_ok()
    {
        log::trace!("Sent start command to patching thread");
    }
}

/// Cancels the patching task/thread.
fn handle_cancel_update(webview: &mut WebView<WebViewUserData>) {
    if block_on(
        webview
            .user_data_mut()
            .patching_thread_tx
            .send(PatcherCommand::Cancel),
    )
    .is_ok()
    {
        log::trace!("Sent cancel command to patching thread");
    }
}

/// Resets the patcher cache (which is used to keep track of already applied
/// patches).
fn handle_reset_cache(_webview: &mut WebView<WebViewUserData>) {
    if let Some(patcher_name) = get_patcher_name() {
        let cache_file_path = PathBuf::from(patcher_name).with_extension("dat");
        if let Err(e) = fs::remove_file(cache_file_path) {
            log::warn!("Failed to remove the cache file: {}", e);
        }
    }
}

// Note: Taken from the rustup project
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

    /// This function is required to start processes that require elevation, from
    /// a non-elevated process.
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

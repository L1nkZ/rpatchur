extern crate log;
extern crate tempfile;
extern crate url;
extern crate web_view;

mod config;
mod grf;
mod thor;

use std::fs::File;
use std::io::prelude::*;
use std::io::SeekFrom;
use std::process::Command;
use std::thread;

use config::*;
use grf::*;
use log::{info, trace, warn};
use thor::*;
use url::Url;
use web_view::*;

const PATCH_LIST_FILE_NAME: &str = "plist.txt";

#[derive(Debug)]
struct PendingPatch {
    info: thor::ThorPatchInfo,
    local_file: File,
}

fn main() {
    // Read the JSON contents of the file as an instance of `PatcherConfiguration`.
    let config = match parse_configuration("./rpatchur.json") {
        Some(v) => v,
        None => return,
    };
    let patching_thread = spawn_patching_thread(config.clone());
    web_view::builder()
        .title("RPatchur")
        .content(Content::Url(config.web.index_url.clone()))
        .size(config.window.width, config.window.height)
        .resizable(config.window.resizable)
        .user_data(config)
        .invoke_handler(|webview, arg| {
            match arg {
                "play" => handle_play(webview),
                "setup" => handle_setup(webview),
                "exit" => handle_exit(webview),
                "cancel_update" => handle_cancel_update(webview),
                "reset_cache" => handle_reset_cache(webview),
                _ => (),
            }
            Ok(())
        })
        .run()
        .unwrap();
    patching_thread.join().unwrap();
}

/// Opens the configured client with arguments, if needed
fn handle_play(webview: &mut WebView<PatcherConfiguration>) {
    let client_path: &String = &webview.user_data().play.path;
    let client_argument: &String = &webview.user_data().play.argument;
    match Command::new(client_path).arg(client_argument).spawn() {
        Ok(child) => trace!("Client started: pid={}", child.id()),
        Err(e) => {
            warn!("Failed to start client: {}", e);
        }
    }
}

/// Opens the configured 'Setup' software
fn handle_setup(webview: &mut WebView<PatcherConfiguration>) {
    let setup_exe: &String = &webview.user_data().setup.path;
    match Command::new(setup_exe).spawn() {
        Ok(child) => trace!("Setup software started: pid={}", child.id()),
        Err(e) => {
            warn!("Failed to start setup software: {}", e);
        }
    }
}

/// Exits the patcher cleanly
fn handle_exit(webview: &mut WebView<PatcherConfiguration>) {
    webview.exit();
}

/// Cancels the update process
fn handle_cancel_update(_webview: &mut WebView<PatcherConfiguration>) {
    warn!("FIXME: cancel_update");
}

/// Resets the cache used to keep track of already applied patches
fn handle_reset_cache(_webview: &mut WebView<PatcherConfiguration>) {
    warn!("FIXME: reset_cache");
}

fn spawn_patching_thread(config: PatcherConfiguration) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        trace!("Patching thread started.");
        let patch_url = Url::parse(config.web.patch_url.as_str()).unwrap();
        let patch_list_url = patch_url.join(PATCH_LIST_FILE_NAME).unwrap();
        let resp = reqwest::blocking::get(patch_list_url).unwrap();
        if !resp.status().is_success() {
            warn!(
                "Patch list '{}' not found on the remote server, aborting.",
                PATCH_LIST_FILE_NAME
            );
            return;
        }
        let patch_index_content = match resp.text() {
            Ok(v) => v,
            Err(_) => return,
        };
        info!("Parsing patch index...");
        let patch_list = thor::patch_list_from_string(patch_index_content.as_str());
        info!("Successfully fetched patch list: {:?}", patch_list);
        // Try fetching patch files
        info!("Downloading patches... ");
        let mut pending_patch_queue: Vec<PendingPatch> = vec![];
        for patch in patch_list {
            let patch_file_url = match patch_url.join(patch.file_name.as_str()) {
                Ok(v) => v,
                Err(_) => {
                    warn!(
                        "Invalid file name given in '{}': '{}', aborting.",
                        PATCH_LIST_FILE_NAME, patch.file_name
                    );
                    return;
                }
            };
            let mut tmp_file = tempfile::tempfile().unwrap();
            let mut resp = reqwest::blocking::get(patch_file_url).unwrap();
            if !resp.status().is_success() {
                warn!(
                    "Patch file '{}' not found on the remote server, aborting.",
                    patch.file_name
                );
                return;
            }
            let _bytes_copied = match resp.copy_to(&mut tmp_file) {
                Ok(v) => v,
                Err(_) => {
                    warn!("Failed to download file '{}', aborting.", patch.file_name);
                    return;
                }
            };
            // File's been downloaded, seek to start and add it to the queue
            let _offset = tmp_file.seek(SeekFrom::Start(0));
            pending_patch_queue.push(PendingPatch {
                info: patch,
                local_file: tmp_file,
            });
        }
        info!("Done");
        // Proceed with actual patching
        info!("Applying patches...");
        for pending_patch in pending_patch_queue {
            info!("Processing {}", pending_patch.info.file_name);
            let archive = match ThorArchive::new(pending_patch.local_file) {
                Ok(v) => v,
                Err(_) => {
                    warn!("Cannot read '{}', aborting.", pending_patch.info.file_name);
                    break;
                }
            };
            let patch_target_grf_name = archive.target_grf_name();
            if patch_target_grf_name.len() == 0 {
                trace!("Target GRF: {:?}", config.client.default_grf_name);
            } else {
                trace!("Target GRF: {:?}", patch_target_grf_name);
            }
            trace!("Entries:");
            for entry in archive.get_entries() {
                trace!("{:?}", entry);
            }
        }
        info!("Patching finished!");
    })
}

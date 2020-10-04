use std::env;
use std::fs::File;
use std::io::Write;
use std::io::{prelude::Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::cache::{read_cache_file, write_cache_file, PatcherCache};
use super::cancellation::{check_for_cancellation, wait_for_cancellation, InterruptibleFnError};
use super::patching::{apply_patch_to_disk, apply_patch_to_grf, GrfPatchingMethod};
use super::{get_patcher_name, PatcherCommand, PatcherConfiguration};
use crate::thor::{self, ThorArchive, ThorPatchList};
use crate::ui::{PatchingStatus, UIController};
use tokio::sync::mpsc;
use url::Url;

#[derive(Debug)]
struct PendingPatch {
    info: thor::ThorPatchInfo,
    local_file: File,
}

pub async fn patcher_thread_routine(
    ui_controller: UIController,
    config: PatcherConfiguration,
    mut patcher_thread_rx: mpsc::Receiver<PatcherCommand>,
) {
    log::trace!("Patching thread started.");
    log::trace!("Waiting for start command");
    if let Err(e) = wait_for_start_command(&mut patcher_thread_rx).await {
        log::error!("Failed to wait for start command: {}", e);
        return;
    }

    interruptible_patcher_routine(ui_controller, config, patcher_thread_rx).await
}

/// Returns when a start command is received, ignoring all other commands that might be received.
/// Returns an error if the other end of the channel happens to be closed while waiting.
async fn wait_for_start_command(rx: &mut mpsc::Receiver<PatcherCommand>) -> Result<(), String> {
    loop {
        match rx.recv().await {
            None => return Err("Channel has been closed".to_string()),
            Some(v) => {
                if let PatcherCommand::Start = v {
                    break;
                }
            }
        }
    }
    Ok(())
}

async fn interruptible_patcher_routine(
    ui_controller: UIController,
    config: PatcherConfiguration,
    mut patcher_thread_rx: mpsc::Receiver<PatcherCommand>,
) {
    log::info!("Patching started");
    // Declare a utility function for dispatching errors to the UI as well as to the logger
    let report_error = |err_msg| async {
        log::error!("{}", err_msg);
        ui_controller
            .dispatch_patching_status(PatchingStatus::Error(err_msg))
            .await;
    };

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
        &ui_controller,
        &mut patcher_thread_rx,
    )
    .await
    {
        Err(e) => match e {
            InterruptibleFnError::Err(msg) => {
                report_error(format!("Failed to download patches: {}.", msg)).await;
                return;
            }
            InterruptibleFnError::Interrupted => {
                report_error("Patching was canceled".to_string()).await;
                return;
            }
        },
        Ok(v) => v,
    };
    log::info!("Done");

    // Proceed with actual patching
    log::info!("Applying patches...");
    if let Err(e) = apply_patches(
        pending_patch_queue,
        &ui_controller,
        &config,
        &cache_file_path,
        &mut patcher_thread_rx,
    )
    .await
    {
        match e {
            InterruptibleFnError::Err(msg) => {
                report_error(format!("Failed to apply patches: {}.", msg)).await;
                return;
            }
            InterruptibleFnError::Interrupted => {
                report_error("Patching was canceled".to_string()).await;
                return;
            }
        }
    }
    ui_controller
        .dispatch_patching_status(PatchingStatus::Ready)
        .await;
    log::info!("Patching finished!");
}

/// Downloads and parses the given 'plist.txt' file from its URL
async fn fetch_patch_list(patch_list_url: Url) -> Result<ThorPatchList, String> {
    let resp = reqwest::get(patch_list_url)
        .await
        .map_err(|e| format!("Failed to retrieve the patch list: {}", e))?;
    if !resp.status().is_success() {
        return Err("Patch list file not found on the remote server".to_string());
    }
    let patch_index_content = resp
        .text()
        .await
        .map_err(|_| "Invalid responde body".to_string())?;
    log::info!("Parsing patch index...");
    Ok(thor::patch_list_from_string(patch_index_content.as_str()))
}

async fn download_patches(
    patch_url: Url,
    patch_list: ThorPatchList,
    ui_controller: &UIController,
    patching_thread_rx: &mut mpsc::Receiver<PatcherCommand>,
) -> Result<Vec<PendingPatch>, InterruptibleFnError> {
    let patch_count = patch_list.len();
    let mut pending_patch_queue = Vec::with_capacity(patch_count);
    ui_controller
        .dispatch_patching_status(PatchingStatus::DownloadInProgress(0, patch_count))
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
        let mut tmp_file = match tempfile::tempfile() {
            Err(e) => {
                return Err(InterruptibleFnError::Err(format!(
                    "Failed to create temporary file: {}.",
                    e
                )))
            }
            Ok(v) => v,
        };
        let download_future = async {
            let mut resp = match reqwest::get(patch_file_url).await {
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

            while let Some(chunk) = resp.chunk().await.ok().unwrap_or(None) {
                let _ = tmp_file
                    .write(&chunk[..])
                    .map_err(|e| format!("Failed to download file '{}': {}.", patch.file_name, e));
            }
            Ok(())
        };
        let cancel_future = wait_for_cancellation(patching_thread_rx);
        // Download file in a cancelable manner
        tokio::select! {
            cancel_res = cancel_future => return Err(cancel_res),
            _ = download_future => {},
        }

        // File's been downloaded, seek to start and add it to the queue
        let _ = tmp_file.seek(SeekFrom::Start(0));
        pending_patch_queue.push(PendingPatch {
            info: patch,
            local_file: tmp_file,
        });
        // Update status
        ui_controller
            .dispatch_patching_status(PatchingStatus::DownloadInProgress(
                patch_number,
                patch_count,
            ))
            .await;
    }
    Ok(pending_patch_queue)
}

async fn apply_patches<P: AsRef<Path>>(
    pending_patch_queue: Vec<PendingPatch>,
    ui_controller: &UIController,
    config: &PatcherConfiguration,
    cache_file_path: P,
    patching_thread_rx: &mut mpsc::Receiver<PatcherCommand>,
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
    ui_controller
        .dispatch_patching_status(PatchingStatus::InstallationInProgress(0, patch_count))
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
        ui_controller
            .dispatch_patching_status(PatchingStatus::InstallationInProgress(
                patch_number,
                patch_count,
            ))
            .await;
    }
    Ok(())
}

use std::env;
use std::fs::File;
use std::io::{self, prelude::Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::cache::{read_cache_file, write_cache_file, PatcherCache};
use super::cancellation::{check_for_cancellation, wait_for_cancellation, InterruptibleFnError};
use super::patching::{apply_patch_to_disk, apply_patch_to_grf, GrfPatchingMethod};
use super::{get_patcher_name, PatcherCommand, PatcherConfiguration};
use crate::thor::{self, ThorArchive, ThorPatchInfo, ThorPatchList};
use crate::ui::{PatchingStatus, UIController};
use tokio::sync::mpsc;
use url::Url;

/// Representation of a pending patch (a patch that's been downloaded but has
/// not been applied yet).
#[derive(Debug)]
struct PendingPatch {
    info: thor::ThorPatchInfo,
    local_file: File,
}

/// Entry point of the patching task.
///
/// This waits for a `PatcherCommand::Start` command before starting an
/// interruptible patching task.
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

    if let Err(err_msg) =
        interruptible_patcher_routine(&ui_controller, config, patcher_thread_rx).await
    {
        log::error!("{}", err_msg);
        ui_controller
            .dispatch_patching_status(PatchingStatus::Error(err_msg))
            .await;
    }
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

/// Main routine of the patching task.
///
/// This routine is written in a way that makes it interuptible (or cancellable)
/// with a relatively low latency.
async fn interruptible_patcher_routine(
    ui_controller: &UIController,
    config: PatcherConfiguration,
    mut patcher_thread_rx: mpsc::Receiver<PatcherCommand>,
) -> Result<(), String> {
    log::info!("Patching started");
    let patch_list_url = Url::parse(config.web.plist_url.as_str()).unwrap();
    let mut patch_list = fetch_patch_list(patch_list_url)
        .await
        .map_err(|e| format!("Failed to retrieve the patch list: {}.", e))?;
    log::info!("Successfully fetched patch list: {:?}", patch_list);

    // Try to read cache
    let cache_file_path =
        get_cache_file_path().ok_or_else(|| "Failed to resolve patcher name.".to_string())?;
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
    let pending_patch_queue = download_patches(
        patch_url,
        patch_list,
        config.patching.check_integrity,
        &ui_controller,
        &mut patcher_thread_rx,
    )
    .await
    .map_err(|e| match e {
        InterruptibleFnError::Err(msg) => format!("Failed to download patches: {}.", msg),
        InterruptibleFnError::Interrupted => "Patching was canceled".to_string(),
    })?;
    log::info!("Done");

    // Proceed with actual patching
    log::info!("Applying patches...");
    apply_patches(
        pending_patch_queue,
        &config,
        &cache_file_path,
        &ui_controller,
        &mut patcher_thread_rx,
    )
    .await
    .map_err(|e| match e {
        InterruptibleFnError::Err(msg) => format!("Failed to apply patches: {}.", msg),
        InterruptibleFnError::Interrupted => "Patching was canceled".to_string(),
    })?;
    log::info!("Done");
    ui_controller
        .dispatch_patching_status(PatchingStatus::Ready)
        .await;
    log::info!("Patching finished!");
    Ok(())
}

/// Downloads and parses a 'plist.txt' file located as the URL contained in the
/// `patch_list_url` argument.
///
/// Returns a vector of `ThorPatchInfo` in case of success.
async fn fetch_patch_list(patch_list_url: Url) -> Result<ThorPatchList, String> {
    let resp = reqwest::get(patch_list_url)
        .await
        .map_err(|e| format!("Failed to GET URL: {}", e))?;
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

/// Returns the patcher cache file's name as a `PathBuf` on success.
fn get_cache_file_path() -> Option<PathBuf> {
    if let Some(patcher_name) = get_patcher_name() {
        Some(PathBuf::from(patcher_name).with_extension("dat"))
    } else {
        None
    }
}

/// Downloads a list of patches (described with a `ThorPatchList`).
///
/// Files are downloaded from the remote directory located at the URL
/// contained in the 'patch_url' argument.
///
/// This function is interruptible.
async fn download_patches(
    patch_url: Url,
    patch_list: ThorPatchList,
    ensure_integrity: bool,
    ui_controller: &UIController,
    patching_thread_rx: &mut mpsc::Receiver<PatcherCommand>,
) -> Result<Vec<PendingPatch>, InterruptibleFnError> {
    let patch_count = patch_list.len();
    let mut pending_patch_queue = Vec::with_capacity(patch_count);
    ui_controller
        .dispatch_patching_status(PatchingStatus::DownloadInProgress(0, patch_count))
        .await;
    for (patch_number, patch) in patch_list.into_iter().enumerate() {
        let mut tmp_file = tempfile::tempfile().map_err(|e| {
            InterruptibleFnError::Err(format!("Failed to create temporary file: {}.", e))
        })?;
        // Download file in a cancelable manner
        tokio::select! {
            cancel_res = wait_for_cancellation(patching_thread_rx) => return Err(cancel_res),
            download_res = download_patch_to_file(&patch_url, &patch, &mut tmp_file) => {
                if let Err(msg) = download_res {
                    return Err(InterruptibleFnError::Err(msg));
                }
            },
        }

        // Check the archive's integrity if required
        if ensure_integrity {
            let _ = tmp_file.seek(SeekFrom::Start(0));
            let mut archive = ThorArchive::new(&tmp_file).map_err(|e| {
                InterruptibleFnError::Err(format!("Failed to check archive's integrity: {}", e))
            })?;
            match archive.is_valid() {
                Err(e) => {
                    if let io::ErrorKind::NotFound = e.kind() {
                    } else {
                        // Only consider this an error if the integrity file was found
                        return Err(InterruptibleFnError::Err(format!(
                            "'{}' archive's integrity file is invalid: {}",
                            patch.file_name,
                            e.to_string(),
                        )));
                    }
                }
                Ok(false) => {
                    // Integrity check failed
                    return Err(InterruptibleFnError::Err(format!(
                        "Archive '{}' is corrupt",
                        patch.file_name
                    )));
                }
                Ok(true) => {
                    // Archive's fine
                }
            }
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
                1 + patch_number,
                patch_count,
            ))
            .await;
    }
    Ok(pending_patch_queue)
}

/// Downloads a single patch described with a `ThorPatchInfo`.
async fn download_patch_to_file(
    patch_url: &Url,
    patch: &ThorPatchInfo,
    tmp_file: &mut File,
) -> Result<(), String> {
    let patch_file_url = patch_url.join(patch.file_name.as_str()).map_err(|_| {
        format!(
            "Invalid file name '{}' given in patch list file.",
            patch.file_name
        )
    })?;
    let mut resp = reqwest::get(patch_file_url)
        .await
        .map_err(|e| format!("Failed to download file '{}': {}.", patch.file_name, e))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Patch file '{}' not found on the remote server.",
            patch.file_name
        ));
    }
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("Failed to download file '{}': {}.", patch.file_name, e))?
    {
        let _ = tmp_file
            .write_all(&chunk[..])
            .map_err(|e| format!("Failed to download file '{}': {}.", patch.file_name, e))?;
    }
    tmp_file.sync_all().map_err(|e| {
        format!(
            "Failed to sync downloaded file '{}': {}.",
            patch.file_name, e
        )
    })?;
    Ok(())
}

/// Parses and applies a list of patches to GRFs and/or to the game client's
/// files.
///
/// This function is interruptible.
async fn apply_patches<P: AsRef<Path>>(
    pending_patch_queue: Vec<PendingPatch>,
    config: &PatcherConfiguration,
    cache_file_path: P,
    ui_controller: &UIController,
    patching_thread_rx: &mut mpsc::Receiver<PatcherCommand>,
) -> Result<(), InterruptibleFnError> {
    let current_working_dir = env::current_dir().map_err(|e| {
        InterruptibleFnError::Err(format!(
            "Failed to resolve current working directory: {}.",
            e
        ))
    })?;
    let patch_count = pending_patch_queue.len();
    ui_controller
        .dispatch_patching_status(PatchingStatus::InstallationInProgress(0, patch_count))
        .await;
    for (patch_number, pending_patch) in pending_patch_queue.into_iter().enumerate() {
        // Cancel the patching process if we've been asked to
        if let Some(e) = check_for_cancellation(patching_thread_rx) {
            return Err(e);
        }
        let patch_name = pending_patch.info.file_name;
        log::info!("Processing {}", patch_name);
        let mut thor_archive = ThorArchive::new(pending_patch.local_file).map_err(|e| {
            InterruptibleFnError::Err(format!("Cannot read '{}': {}.", patch_name, e))
        })?;

        if thor_archive.use_grf_merging() {
            // Patch GRF file
            let target_grf_name = {
                if thor_archive.target_grf_name().is_empty() {
                    config.client.default_grf_name.clone()
                } else {
                    thor_archive.target_grf_name()
                }
            };
            log::trace!("Target GRF: {:?}", target_grf_name);
            let grf_patching_method = match config.patching.in_place {
                true => GrfPatchingMethod::InPlace,
                false => GrfPatchingMethod::OutOfPlace,
            };
            let target_grf_path = current_working_dir.join(&target_grf_name);
            apply_patch_to_grf(
                grf_patching_method,
                config.patching.create_grf,
                target_grf_path,
                &mut thor_archive,
            )
            .map_err(|e| {
                InterruptibleFnError::Err(format!("Failed to patch '{}': {}.", target_grf_name, e))
            })?;
        } else {
            // Patch root directory
            apply_patch_to_disk(&current_working_dir, &mut thor_archive).map_err(|e| {
                InterruptibleFnError::Err(format!("Failed to apply patch '{}': {}.", patch_name, e))
            })?;
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
                1 + patch_number,
                patch_count,
            ))
            .await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use httptest::{matchers::*, responders::*, Expectation, Server};
    use std::io::Read;

    #[tokio::test]
    async fn test_download_path_to_file() {
        // Generate 200MiB of data
        let data_size: usize = 200 * 1024 * 1024;
        let body_content: Vec<u8> = (0..data_size).map(|x| x as u8).collect();

        let patch_name = "patch_archive";
        let patch_path = format!("/{}", patch_name);
        // Setup a local web server
        let server = Server::run();
        // Configure the server to expect a single GET request and respond
        // with a 200 status code.
        server.expect(
            Expectation::matching(request::method_path("GET", patch_path.clone()))
                .respond_with(status_code(200).body(body_content.clone())),
        );

        // "Download" the file
        let from_url = Url::parse(server.url("/").to_string().as_str()).unwrap();
        let patch_info = ThorPatchInfo {
            index: 0,
            file_name: patch_name.to_string(),
        };
        let mut tmp_file = tempfile::tempfile().unwrap();
        download_patch_to_file(&from_url, &patch_info, &mut tmp_file)
            .await
            .unwrap();

        tmp_file.seek(SeekFrom::Start(0)).unwrap();
        let mut file_content = Vec::with_capacity(data_size);
        tmp_file.read_to_end(&mut file_content).unwrap();
        // Size check
        assert_eq!(data_size as u64, tmp_file.metadata().unwrap().len());
        assert_eq!(data_size, file_content.len());
        // Content check
        assert_eq!(body_content, file_content);
    }
}

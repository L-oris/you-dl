use async_compat::CompatExt;
use dialoguer::Select;
use futures_util::StreamExt;
use indicatif::ProgressBar;
use qstring::QString;
use reqwest;
use smol::{fs, io};
use std::convert::TryFrom;
use std::convert::TryInto;
use std::path::Path;

// make macros available for the entire crate.
use colored;
#[macro_use]
mod log;

mod models;
mod utils;
pub mod wrapper;
pub use models::PlayerResponse;
pub use models::YouDlError;

// TODO LORIS: add adaptive formats, audios only

// TODO LORIS: remove workspaces, use log macros everywhere

// TODO LORIS: check this one: https://tyrrrz.me/blog/reverse-engineering-youtube -> add to README.md

// TODO LORIS: publish to homebrew

pub async fn process_request(
    url: &str,
    output_dir: &str,
    progress_bar: ProgressBar,
) -> Result<(), YouDlError> {
    let video_id = utils::extract_video_id(url)?;
    let player_response = get_player_response(video_id).await?;
    let download_options = models::DownloadOptions::try_from(player_response)?;
    let chosen_option = ask_preferred_file_format(download_options);
    download(chosen_option, output_dir, progress_bar).await?;
    Ok(())
}

async fn get_player_response(video_id: &str) -> Result<PlayerResponse, YouDlError> {
    let get_video_info_url = format!(
        "https://www.youtube.com/get_video_info?video_id={}",
        video_id
    );

    let response_body = reqwest::get(&get_video_info_url)
        .compat()
        .await
        .map_err(|e| YouDlError::InvalidResponse(e.to_string()))?
        .text()
        .await
        .map_err(|e| YouDlError::InvalidResponse(e.to_string()))?;

    let player_response = QString::from(response_body.as_str())
        .get("player_response")
        .map(|s| s.to_owned())
        .ok_or(YouDlError::InvalidResponse(
            "missing value for player_response".to_owned(),
        ))?;

    serde_json::from_str::<PlayerResponse>(&player_response)
        .map_err(|e| YouDlError::InvalidResponse(e.to_string()))
}

fn ask_preferred_file_format(
    mut download_options: models::DownloadOptions,
) -> models::DownloadOption {
    select!(
        "choose the file format for `{}`:",
        download_options.get_title()
    );
    let chosen_index = Select::new()
        .items(&download_options.0)
        .default(0)
        .interact()
        .unwrap();

    let chosen = download_options.0.remove(chosen_index);
    info!("chosen itag {} for `{}`", chosen.itag, chosen.title);
    chosen
}

async fn download(
    download_option: models::DownloadOption,
    output_dir: &str,
    progress_bar: ProgressBar,
) -> Result<(), YouDlError> {
    let response = reqwest::get(&download_option.url)
        .compat()
        .await
        .map_err(|e| YouDlError::InvalidResponse(e.to_string()))?;
    progress_bar.set_length(response.content_length().unwrap_or(u64::MAX));
    progress_bar.set_prefix("Status:"); // Setting the prefix in main will show the bars before the prompt

    let mut output_file = fs::File::create(Path::new(output_dir).join(&download_option.file_name))
        .await
        .map_err(|e| YouDlError::Application(e.to_string()))?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| YouDlError::InvalidResponse(e.to_string()))?;
        progress_bar.inc(chunk.len().try_into().expect("valid conversion"));
        io::copy(&mut &*chunk, &mut output_file)
            .await
            .map_err(|e| YouDlError::Application(e.to_string()))?;
    }

    progress_bar.finish_with_message(&format!(
        "Successfully downloaded: {}",
        download_option.title
    ));
    Ok(())
}
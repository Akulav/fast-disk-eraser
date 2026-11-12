//! Minimal blocking HTTP used for update checks and downloads (always on a worker thread).

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};

pub const USER_AGENT: &str = concat!("FastDiskEraser/", env!("CARGO_PKG_VERSION"));

pub fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(None)
        .build()?)
}

pub fn get_text(client: &reqwest::blocking::Client, url: &str) -> Result<String> {
    let resp = client.get(url).timeout(Duration::from_secs(30)).send().with_context(|| format!("GET {url}"))?;
    let resp = resp.error_for_status().with_context(|| format!("GET {url}"))?;
    Ok(resp.text()?.trim_start_matches('\u{feff}').to_string())
}

pub fn get_json<T: serde::de::DeserializeOwned>(client: &reqwest::blocking::Client, url: &str) -> Result<T> {
    let text = get_text(client, url)?;
    serde_json::from_str(&text).with_context(|| format!("invalid JSON from {url}"))
}

/// Download `url` to `dest`, reporting `(done, total)`; `cancelled()` aborts.
pub fn download(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    on_bytes: &dyn Fn(u64, u64),
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    let mut resp = client.get(url).send().with_context(|| format!("GET {url}"))?.error_for_status()?;
    let total = resp.content_length().unwrap_or(0);
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut f = std::fs::File::create(dest).with_context(|| format!("could not write {}", dest.display()))?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    loop {
        if cancelled() {
            drop(f);
            let _ = std::fs::remove_file(dest);
            bail!("cancelled");
        }
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n])?;
        done += n as u64;
        on_bytes(done, total.max(done));
    }
    f.sync_all()?;
    Ok(())
}

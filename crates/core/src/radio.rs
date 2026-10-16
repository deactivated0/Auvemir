//! Portable custom-station playlists. Audio resources are never opened during import.
use crate::model::{Station, stable_id};
use anyhow::{Result, ensure};

pub fn parse_m3u(text: &str) -> Result<Vec<Station>> {
    ensure!(
        text.len() <= 8 * 1024 * 1024,
        "Radio playlist exceeds 8 MiB"
    );
    let mut stations = Vec::new();
    let mut name = None;
    for line in text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            name = info.split_once(',').map(|(_, name)| name.trim().to_owned());
        } else if !line.starts_with('#') {
            ensure!(line.len() <= 8192, "Station URL is too long");
            let url = crate::network::validate_url(line, true)?;
            let name = name
                .take()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| url.host_str().unwrap_or("Radio").to_owned());
            ensure!(name.len() <= 8192, "Station name is too long");
            ensure!(
                stations.len() < 10_000,
                "Radio playlist exceeds 10,000 stations"
            );
            stations.push(Station {
                id: format!("custom:{}", stable_id("import-radio", line)),
                name,
                url: line.to_owned(),
                country: String::new(),
                language: String::new(),
                tags: String::new(),
                logo: None,
                custom: true,
                favorite: false,
            });
        }
    }
    Ok(stations)
}
pub fn write_m3u(stations: &[Station]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for station in stations {
        out.push_str("#EXTINF:-1,");
        out.push_str(&station.name.replace(['\r', '\n'], " "));
        out.push('\n');
        out.push_str(&station.url.replace(['\r', '\n'], ""));
        out.push('\n');
    }
    out
}

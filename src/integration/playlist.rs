use crate::config::{Station, StationsCatalog};
use crate::error::{ResoError, Result};
use std::path::Path;

pub fn import_playlist(path: &Path, catalog: &mut StationsCatalog) -> Result<usize> {
    let content = std::fs::read_to_string(path)?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let entries = match extension.to_ascii_lowercase().as_str() {
        "m3u" | "m3u8" => parse_m3u(&content),
        "pls" => parse_pls(&content),
        _ => {
            return Err(ResoError::Config(
                "Playlist extension must be .m3u, .m3u8, or .pls".to_string(),
            ))
        }
    };
    let mut imported = 0;
    for (name, url) in entries {
        let parsed = reqwest::Url::parse(&url)
            .map_err(|_| ResoError::Config(format!("Invalid stream URL in playlist: {url}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(ResoError::Config(format!(
                "Unsupported stream URL scheme: {}",
                parsed.scheme()
            )));
        }
        let normalized_url = crate::util::normalize_stream_url(&url);
        if catalog
            .stations
            .iter()
            .any(|station| crate::util::normalize_stream_url(&station.url) == normalized_url)
        {
            continue;
        }
        let id_base = slug(&name);
        let mut id = id_base.clone();
        let mut suffix = 2;
        while catalog.stations.iter().any(|station| station.id == id) {
            id = format!("{id_base}-{suffix}");
            suffix += 1;
        }
        catalog.add_station(Station {
            id,
            name,
            url,
            codec: "UNKNOWN".to_string(),
            sample_rate: 0,
            bit_depth: 0,
            homepage: None,
            tags: Vec::new(),
            favorite: false,
            rp_channel: None,
        });
        imported += 1;
    }
    Ok(imported)
}

pub fn export_playlist(path: &Path, catalog: &StationsCatalog) -> Result<()> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let content = match extension.to_ascii_lowercase().as_str() {
        "m3u" | "m3u8" => {
            let mut out = String::from("#EXTM3U\n");
            for station in &catalog.stations {
                out.push_str(&format!(
                    "#EXTINF:-1,{}\n{}\n",
                    single_line(&station.name),
                    station.url
                ));
            }
            out
        }
        "pls" => {
            let mut out = String::from("[playlist]\n");
            for (index, station) in catalog.stations.iter().enumerate() {
                let number = index + 1;
                out.push_str(&format!(
                    "File{number}={}\nTitle{number}={}\n",
                    station.url,
                    single_line(&station.name)
                ));
            }
            out.push_str(&format!(
                "NumberOfEntries={}\nVersion=2\n",
                catalog.stations.len()
            ));
            out
        }
        _ => {
            return Err(ResoError::Config(
                "Playlist extension must be .m3u, .m3u8, or .pls".to_string(),
            ))
        }
    };
    std::fs::write(path, content)?;
    Ok(())
}

fn parse_m3u(content: &str) -> Vec<(String, String)> {
    let mut name = None;
    let mut entries = Vec::new();
    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            name = info
                .split_once(',')
                .map(|(_, title)| title.trim().to_string());
        } else if !line.starts_with('#') {
            let url = line.to_string();
            let station_name = name
                .take()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| url.clone());
            entries.push((station_name, url));
        }
    }
    entries
}

fn parse_pls(content: &str) -> Vec<(String, String)> {
    let mut files = std::collections::BTreeMap::new();
    let mut titles = std::collections::BTreeMap::new();
    for line in content.lines().map(str::trim) {
        if let Some((key, value)) = line.split_once('=') {
            let (prefix, number) = key.split_at(key.find(char::is_numeric).unwrap_or(key.len()));
            if let Ok(index) = number.parse::<usize>() {
                match prefix.to_ascii_lowercase().as_str() {
                    "file" => {
                        files.insert(index, value.trim().to_string());
                    }
                    "title" => {
                        titles.insert(index, value.trim().to_string());
                    }
                    _ => {}
                }
            }
        }
    }
    files
        .into_iter()
        .map(|(index, url)| {
            let title = titles
                .remove(&index)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| url.clone());
            (title, url)
        })
        .collect()
}

fn slug(name: &str) -> String {
    let slug: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    let normalized = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if normalized.is_empty() {
        "station".to_string()
    } else {
        normalized
    }
}

fn single_line(value: &str) -> String {
    value
        .chars()
        .filter(|character| *character != '\n' && *character != '\r')
        .collect()
}

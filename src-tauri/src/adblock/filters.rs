use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

pub const FILTER_FILES: [&str; 2] = ["easylist.txt", "easyprivacy.txt"];

const FILTER_LISTS: [(&str, &str); 2] = [
    ("easylist.txt", "https://easylist.to/easylist/easylist.txt"),
    ("easyprivacy.txt", "https://easylist.to/easylist/easyprivacy.txt"),
];

/// Downloads supported filter lists and atomically replaces their cache files.
/// Returns true if at least one list was successfully refreshed.
pub fn refresh_filter_cache(cache_dir: &Path) -> bool {
    if let Err(error) = fs::create_dir_all(cache_dir) {
        eprintln!("[Pake Adblock] Cannot create filter cache: {error}");
        return false;
    }

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(25))
        .user_agent("Pake-Adblock/1.0")
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            eprintln!("[Pake Adblock] Cannot create HTTP client: {error}");
            return false;
        }
    };

    let mut refreshed = false;
    for (filename, url) in FILTER_LISTS {
        match client.get(url).send().and_then(|response| response.error_for_status()) {
            Ok(response) => match response.text() {
                Ok(contents) if contents.contains("!") || contents.contains("##") => {
                    if write_cache_atomically(cache_dir, filename, contents.as_bytes()).is_ok() {
                        refreshed = true;
                    }
                }
                Ok(_) => eprintln!("[Pake Adblock] Ignored unexpectedly empty list: {filename}"),
                Err(error) => eprintln!("[Pake Adblock] Failed reading {filename}: {error}"),
            },
            Err(error) => eprintln!("[Pake Adblock] Failed downloading {filename}: {error}"),
        }
    }

    refreshed
}

fn write_cache_atomically(cache_dir: &Path, filename: &str, bytes: &[u8]) -> std::io::Result<()> {
    let target = cache_dir.join(filename);
    let temporary: PathBuf = cache_dir.join(format!("{filename}.download"));
    {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    // Windows does not replace an existing destination with std::fs::rename.
    // Remove the old file only after the new download has been fully written.
    if target.exists() {
        fs::remove_file(&target)?;
    }
    fs::rename(temporary, target)
}

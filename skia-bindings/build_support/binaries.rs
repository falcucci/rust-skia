//! Support for exporting and building prebuilt binaries.

use crate::build_support::{azure, binaries, cargo, git, skia};
use flate2::read::GzDecoder;
use std::fs;
use std::io;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tar::Archive;

/// Export binaries if we are inside a git repository _and_
/// the artifact staging directory is set.
/// The git repository test is important to support package verifications.
pub fn should_export() -> Option<PathBuf> {
    if git::half_hash().is_none() {
        return None;
    }

    azure::artifact_staging_directory()
}

/// Export the binaries to a target directory.
pub fn export(config: &skia::BinariesConfiguration, target_dir: &Path) -> io::Result<()> {
    let half_hash = git::half_hash().expect("failed to retrieve the git hash");
    let key = binaries::key(&half_hash, &config.feature_ids);

    let export_dir = prepare_export_directory(&key, target_dir)?;

    fs::copy(crate::SRC_BINDINGS_RS, export_dir.join("bindings.rs"))?;

    let output_directory = &config.output_directory;

    let target = cargo::target();

    for lib in &config.built_libraries {
        let filename = &target.library_to_filename(lib);
        fs::copy(output_directory.join(filename), export_dir.join(filename))?;
    }

    for file in &config.additional_files {
        fs::copy(output_directory.join(file), export_dir.join(file))?;
    }

    Ok(())
}

/// Prepares the binaries directory and sets the tag.txt and key.txt
/// file.
fn prepare_export_directory(key: &str, artifacts: &Path) -> io::Result<PathBuf> {
    let binaries = artifacts.join("skia-binaries");
    fs::create_dir_all(&binaries)?;

    // this is primarily for azure to know the tag and the key of the binaries,
    // but they can stay inside the archive.

    {
        let mut tag_file = fs::File::create(binaries.join("tag.txt")).unwrap();
        tag_file.write_all(cargo::package_version().as_bytes())?;
    }
    {
        let mut key_file = fs::File::create(binaries.join("key.txt")).unwrap();
        key_file.write_all(key.as_bytes())?;
    }

    Ok(binaries)
}

/// The name of the tar archive without any keys or file extensions. This is also the name
/// of the subdirectory that is created when the archive is unpacked.
pub const ARCHIVE_NAME: &str = "skia-binaries";

/// Key generation function.
/// The resulting string will uniquely identify the generated binaries.
/// Every part of the key is separated by '-' and no grouping / enclosing characters are used
/// because GitHub strips them from the filenames (tested "<>[]{}()",
/// and also Unicode characters seem to be stripped).
pub fn key(repository_short_hash: &str, features: &[impl AsRef<str>]) -> String {
    let mut components = Vec::new();

    fn group(str: impl AsRef<str>) -> String {
        // no grouping syntax ATM
        format!("{}", str.as_ref())
    }

    // SHA hash of the rust-skia repository.
    components.push(repository_short_hash.to_owned());

    // The target architecture, vendor, system, and abi if specified.
    components.push(group(cargo::target().to_string()));

    // features, sorted and duplicates removed.
    if !features.is_empty() {
        let features: String = {
            let mut features: Vec<String> =
                features.iter().map(|f| f.as_ref().to_string()).collect();
            features.sort();
            features.dedup();
            features.join("-")
        };

        components.push(group(features));
    };

    components.join("-")
}

/// Create the download URL for the prebuilt binaries archive.
pub fn download_url(tag: impl AsRef<str>, key: impl AsRef<str>) -> String {
    format!(
        "https://github.com/rust-skia/skia-binaries/releases/download/{}/{}-{}.tar.gz",
        tag.as_ref(),
        ARCHIVE_NAME,
        key.as_ref()
    )
}

pub fn unpack(archive: impl Read, output_directory: &Path) -> io::Result<()> {
    let tar = GzDecoder::new(archive);
    // note: this creates the skia-bindings/ directory.
    Archive::new(tar).unpack(output_directory)?;
    let binaries_dir = output_directory.join(ARCHIVE_NAME);
    let paths: Vec<PathBuf> = fs::read_dir(binaries_dir)?
        .map(|e| e.unwrap().path())
        .collect();

    // pull out all nested files.
    for path in paths {
        let name = path.file_name().unwrap();
        let target_path = output_directory.join(name);
        fs::rename(path, target_path)?
    }
    Ok(())
}

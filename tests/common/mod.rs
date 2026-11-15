#![allow(dead_code)] // Integration-test binaries intentionally use different helper subsets.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use jabfa::Section;
use tempfile::TempDir;

pub fn basic_vault(temp: &TempDir) -> PathBuf {
    let vault = temp.path().join("vault");
    copy_dir(&fixture_path("basic"), &vault);

    fs::create_dir_all(vault.join(".git")).expect("create ignored .git directory");
    fs::write(
        vault.join(".git/ignored.md"),
        "# Git metadata\n\nThis must not be indexed.\n",
    )
    .expect("write ignored .git fixture");

    fs::create_dir_all(vault.join(".jabfa")).expect("create ignored index directory");
    fs::write(
        vault.join(".jabfa/ignored.md"),
        "# Derived data\n\nThis must not be indexed.\n",
    )
    .expect("write ignored .jabfa fixture");

    vault
}

pub fn malformed_vault(temp: &TempDir) -> PathBuf {
    let vault = temp.path().join("vault");
    copy_dir(&fixture_path("malformed"), &vault);
    vault
}

pub fn retrieval_vault(temp: &TempDir) -> PathBuf {
    let vault = temp.path().join("vault");
    copy_dir(&fixture_path("retrieval"), &vault);
    vault
}

pub fn obsidian_vault(temp: &TempDir) -> PathBuf {
    let vault = temp.path().join("vault");
    copy_dir(&fixture_path("obsidian"), &vault);
    vault
}

pub fn vault_manifest(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut manifest = BTreeMap::new();
    collect_manifest(root, root, &mut manifest);
    manifest
}

#[allow(clippy::too_many_arguments)]
pub fn assert_section(
    section: &Section,
    heading: &str,
    level: u8,
    heading_path: &[&str],
    start_line: usize,
    end_line: usize,
    text: &str,
) {
    assert_eq!(section.heading.as_deref(), Some(heading), "heading");
    assert_eq!(section.level, level, "heading level for {heading}");
    let actual_path: Vec<&str> = section.heading_path.iter().map(String::as_str).collect();
    assert_eq!(actual_path, heading_path, "heading path for {heading}");
    assert_eq!(section.start_line, start_line, "start line for {heading}");
    assert_eq!(section.end_line, end_line, "end line for {heading}");
    assert_eq!(section.text, text, "body text for {heading}");
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn copy_dir(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create fixture destination");
    let mut entries: Vec<_> = fs::read_dir(source)
        .expect("read fixture directory")
        .map(|entry| entry.expect("read fixture entry").path())
        .collect();
    entries.sort();

    for path in entries {
        let target = destination.join(path.file_name().expect("fixture file name"));
        if path.is_dir() {
            copy_dir(&path, &target);
        } else {
            fs::copy(&path, &target).expect("copy fixture file");
        }
    }
}

fn collect_manifest(root: &Path, directory: &Path, manifest: &mut BTreeMap<String, Vec<u8>>) {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .expect("read vault directory")
        .map(|entry| entry.expect("read vault entry").path())
        .collect();
    entries.sort();

    for path in entries {
        if path.is_dir() {
            collect_manifest(root, &path, manifest);
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("vault-relative path")
                .to_string_lossy()
                .replace('\\', "/");
            manifest.insert(relative, fs::read(&path).expect("read vault file"));
        }
    }
}

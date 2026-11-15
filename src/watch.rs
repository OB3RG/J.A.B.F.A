use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::{JabfaError, Result};
use crate::index::{IndexConfig, IndexReport, index_vault_incremental};

const SHUTDOWN_POLL: Duration = Duration::from_millis(50);

/// Watch a vault and commit one incremental generation after each debounced batch.
///
/// The callback receives the initial and subsequent committed reports. Setting
/// `shutdown` requests a clean return. Filesystem notifications and indexing stay
/// synchronous; the atomic flag is the only shared watcher state.
pub fn watch_vault<F>(
    config: &IndexConfig,
    debounce: Duration,
    shutdown: Arc<AtomicBool>,
    mut on_report: F,
) -> Result<()>
where
    F: FnMut(IndexReport) -> Result<()>,
{
    let (sender, receiver) = channel();
    let mut watcher = RecommendedWatcher::new(
        move |event| {
            let _ = sender.send(event);
        },
        notify::Config::default(),
    )
    .map_err(|error| JabfaError::Watch(error.to_string()))?;
    watcher
        .watch(&config.vault_root, RecursiveMode::Recursive)
        .map_err(|error| JabfaError::Watch(error.to_string()))?;

    on_report(index_vault_incremental(config)?)?;
    while !shutdown.load(Ordering::Relaxed) {
        match receiver.recv_timeout(SHUTDOWN_POLL) {
            Ok(Ok(event)) if is_relevant(&event) => {
                debounce_batch(&receiver, debounce, &shutdown)?;
                if !shutdown.load(Ordering::Relaxed) {
                    on_report(index_vault_incremental(config)?)?;
                }
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => return Err(JabfaError::Watch(error.to_string())),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(JabfaError::Watch(
                    "filesystem notification channel disconnected".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn debounce_batch(
    receiver: &Receiver<notify::Result<Event>>,
    debounce: Duration,
    shutdown: &AtomicBool,
) -> Result<()> {
    let mut deadline = Instant::now()
        .checked_add(debounce)
        .unwrap_or_else(Instant::now);
    loop {
        if shutdown.load(Ordering::Relaxed) {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        match receiver.recv_timeout(remaining.min(SHUTDOWN_POLL)) {
            Ok(Ok(event)) if is_relevant(&event) => {
                deadline = Instant::now()
                    .checked_add(debounce)
                    .unwrap_or_else(Instant::now);
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => return Err(JabfaError::Watch(error.to_string())),
            Err(RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    return Ok(());
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(JabfaError::Watch(
                    "filesystem notification channel disconnected".to_owned(),
                ));
            }
        }
    }
}

fn is_relevant(event: &Event) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.iter().any(|path| is_markdown_source(path))
}

fn is_markdown_source(path: &Path) -> bool {
    let is_markdown = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
    is_markdown
        && !path.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|name| matches!(name, ".git" | ".obsidian" | ".jabfa"))
        })
}

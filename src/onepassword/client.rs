use anyhow::{Context, Result, anyhow};
use std::io::Write;
use std::process::{Command, Stdio};

pub fn copy_secret_to_clipboard(reference: &str) -> Result<()> {
    let output = Command::new("op")
        .arg("read")
        .arg("--no-newline")
        .arg(reference)
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .context("Failed to execute 1Password CLI")?;

    if !output.status.success() {
        return Err(anyhow!(
            "1Password could not retrieve the requested credential."
        ));
    }

    if output.stdout.is_empty() {
        return Err(anyhow!("The requested credential is empty."));
    }

    let mut clipboard = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .context("Failed to launch macOS clipboard utility")?;

    clipboard
        .stdin
        .take()
        .context("Could not access clipboard input")?
        .write_all(&output.stdout)
        .context("Failed to copy credential to clipboard")?;

    let status = clipboard
        .wait()
        .context("Failed to complete clipboard operation")?;

    if !status.success() {
        return Err(anyhow!("Failed to copy credential to clipboard."));
    }

    Ok(())
}

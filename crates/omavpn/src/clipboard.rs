use std::process::Command;

use anyhow::{bail, Result};

/// Read clipboard text on Wayland (`wl-paste`) with X11 fallback (`xclip`/`xsel`).
pub fn read_clipboard() -> Result<String> {
    let candidates: &[(&str, &[&str])] = &[
        ("wl-paste", &["--no-newline"]),
        ("xclip", &["-o", "-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--output"]),
    ];

    for (bin, args) in candidates {
        if let Ok(output) = Command::new(bin).args(*args).output() {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !text.is_empty() {
                    return Ok(text);
                }
            }
        }
    }

    bail!("clipboard is empty or no clipboard tool found (install wl-clipboard)")
}

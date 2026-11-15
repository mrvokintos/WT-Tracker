// "Create shortcut": a .desktop entry for the app launcher on Linux, a Start menu shortcut on Windows.

use std::path::PathBuf;

/// The file to launch: the AppImage itself when running from one (its mount point changes every run).
fn target() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file()) {
        return Ok(p);
    }
    std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
pub fn create() -> Result<PathBuf, String> {
    let exe = target()?;
    let apps = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or("HOME is not set")?
        .join("applications");
    std::fs::create_dir_all(&apps).map_err(|e| format!("{}: {e}", apps.display()))?;
    let path = apps.join("wt-tracker.desktop");
    let dir = exe.parent().map(|d| d.display().to_string()).unwrap_or_default();
    let text = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=WT Tracker\n\
         Comment=War Thunder enemy tracker overlay\n\
         Exec={}\n\
         Path={dir}\n\
         Icon=applications-games\n\
         Terminal=false\n\
         Categories=Game;\n\
         StartupWMClass=wt-tracker\n\
         X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n",
        desktop_quote(&exe.display().to_string())
    );
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    // most launchers watch the folder; this just refreshes the cache where there is one
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    Ok(path)
}

/// Exec= quoting from the Desktop Entry spec.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn desktop_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(windows)]
pub fn create() -> Result<PathBuf, String> {
    use std::os::windows::process::CommandExt;
    let exe = target()?;
    // canonicalize gives \\?\C:\..., which shortcuts don't like
    let exe = PathBuf::from(exe.display().to_string().trim_start_matches(r"\\?\"));
    let programs = PathBuf::from(std::env::var_os("APPDATA").ok_or("APPDATA is not set")?)
        .join(r"Microsoft\Windows\Start Menu\Programs");
    std::fs::create_dir_all(&programs).map_err(|e| e.to_string())?;
    let lnk = programs.join("WT Tracker.lnk");
    let q = |p: &std::path::Path| p.display().to_string().replace('\'', "''");
    let dir = exe.parent().map(q).unwrap_or_default();
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.WorkingDirectory='{dir}';$s.Description='War Thunder enemy tracker overlay';$s.Save()",
        q(&lnk),
        q(&exe)
    );
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("powershell: {e}"))?;
    if !out.status.success() || !lnk.is_file() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(lnk)
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn create() -> Result<PathBuf, String> {
    let _ = target;
    Err("not supported on this OS".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_exec_is_quoted() {
        assert_eq!(super::desktop_quote("/opt/WT Tracker/wt-tracker"), "\"/opt/WT Tracker/wt-tracker\"");
        assert_eq!(super::desktop_quote(r#"/a"b$c\d%e"#), r#""/a\"b\$c\\d%%e""#);
    }
}

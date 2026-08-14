//! Auto-split from services.rs
use super::*;

    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    const CRON_LINE: &str = "@reboot $(which ruflo) daemon start --background\n";
    const SYSTEMD_UNIT_NAME: &str = "ruflo-daemon.service";
    const LAUNCHD_LABEL: &str = "io.ruflo.daemon";
    const LAUNCHD_PLIST_NAME: &str = "io.ruflo.daemon.plist";

    /// Resolve the absolute path to the `ruflo` binary via `which ruflo`.
    /// Falls back to the bare word `ruflo` if the lookup fails.
    fn ruflo_path() -> String {
        Command::new("which")
            .arg("ruflo")
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                } else {
                    None
                }
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "ruflo".to_string())
    }

    fn build_systemd_unit(ruflo: &str) -> String {
        format!(
            "[Unit]\n\
             Description=Ruflo Daemon\n\
             After=network.target\n\n\
             [Service]\n\
             ExecStart={ruflo} daemon start --foreground\n\
             Restart=always\n\n\
             [Install]\n\
             WantedBy=default.target\n"
        )
    }

    fn build_launchd_plist(ruflo: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <plist version=\"1.0\"><dict>\n\
             \x20 <key>Label</key><string>{label}</string>\n\
             \x20 <key>ProgramArguments</key><array>\n\
             \x20   <string>{ruflo}</string><string>daemon</string>\n\
             \x20   <string>start</string><string>--foreground</string>\n\
             \x20 </array>\n\
             \x20 <key>RunAtLoad</key><true/>\n\
             </dict></plist>\n",
            label = LAUNCHD_LABEL,
            ruflo = ruflo
        )
    }

    /// Install via crontab: pipe `{existing}\n{cron_line}` to `crontab -`.
    /// Idempotent — skips if the line is already present.
    pub fn install_cron() -> Result<String, String> {
        // Read existing crontab (may be empty or unset).
        let existing = match Command::new("crontab").arg("-l").output() {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
            _ => String::new(),
        };
        let merged = if existing.contains(CRON_LINE.trim()) {
            existing
        } else {
            format!("{existing}{CRON_LINE}")
        };
        let mut child = Command::new("crontab")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("crontab spawn failed: {e}"))?;
        {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| "crontab stdin unavailable".to_string())?;
            stdin
                .write_all(merged.as_bytes())
                .map_err(|e| format!("crontab write failed: {e}"))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|e| format!("crontab wait failed: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "crontab install failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        write_state(
            "autostart",
            &json!({
                "method": "cron",
                "config": CRON_LINE,
                "installedAt": now_ms()
            }),
        );
        Ok(CRON_LINE.into())
    }

    /// Install via systemd user unit: write
    /// `~/.config/systemd/user/ruflo-daemon.service`, then
    /// `systemctl --user daemon-reload && systemctl --user enable ruflo-daemon`.
    pub fn install_systemd() -> Result<String, String> {
        let ruflo = ruflo_path();
        let unit = build_systemd_unit(&ruflo);
        let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
        let dir = PathBuf::from(&home).join(".config/systemd/user");
        fs::create_dir_all(&dir).map_err(|e| format!("mkdir systemd user dir: {e}"))?;
        let unit_path = dir.join(SYSTEMD_UNIT_NAME);
        fs::write(&unit_path, &unit).map_err(|e| format!("write unit file: {e}"))?;
        let reload = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()
            .map_err(|e| format!("systemctl --user daemon-reload spawn: {e}"))?;
        if !reload.success() {
            return Err("systemctl --user daemon-reload failed".to_string());
        }
        let enable = Command::new("systemctl")
            .args(["--user", "enable", "ruflo-daemon"])
            .status()
            .map_err(|e| format!("systemctl --user enable spawn: {e}"))?;
        if !enable.success() {
            return Err("systemctl --user enable ruflo-daemon failed".to_string());
        }
        write_state(
            "autostart",
            &json!({
                "method": "systemd",
                "config": unit,
                "path": unit_path.display().to_string(),
                "unitName": SYSTEMD_UNIT_NAME,
                "installedAt": now_ms()
            }),
        );
        Ok(unit)
    }

    /// Install via launchd (macOS only): write
    /// `~/Library/LaunchAgents/io.ruflo.daemon.plist`, then
    /// `launchctl load`. On non-macOS targets, returns Err.
    #[cfg(target_os = "macos")]
    pub fn install_launchd() -> Result<String, String> {
        let ruflo = ruflo_path();
        let plist = build_launchd_plist(&ruflo);
        let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
        let dir = PathBuf::from(&home).join("Library/LaunchAgents");
        fs::create_dir_all(&dir).map_err(|e| format!("mkdir LaunchAgents dir: {e}"))?;
        let plist_path = dir.join(LAUNCHD_PLIST_NAME);
        fs::write(&plist_path, &plist).map_err(|e| format!("write plist: {e}"))?;
        let load = Command::new("launchctl")
            .arg("load")
            .arg(&plist_path)
            .status()
            .map_err(|e| format!("launchctl load spawn: {e}"))?;
        if !load.success() {
            return Err("launchctl load failed".to_string());
        }
        write_state(
            "autostart",
            &json!({
                "method": "launchd",
                "config": plist,
                "path": plist_path.display().to_string(),
                "label": LAUNCHD_LABEL,
                "installedAt": now_ms()
            }),
        );
        Ok(plist)
    }

    /// launchd install is macOS-only. On other platforms the build is still
    /// valid; we surface an explicit runtime error here.
    #[cfg(not(target_os = "macos"))]
    pub fn install_launchd() -> Result<String, String> {
        Err("launchd install is only supported on macOS".to_string())
    }

    /// Reverse the appropriate install based on the stored `method`. Clears
    /// the autostart state file on success.
    pub fn uninstall() -> Result<(), String> {
        let state = read_state("autostart");
        let method = state["method"].as_str().unwrap_or("");
        match method {
            "cron" => {
                // Remove the ruflo line from existing crontab, write back.
                let existing = match Command::new("crontab").arg("-l").output() {
                    Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
                    _ => String::new(),
                };
                let kept: String = existing
                    .lines()
                    .filter(|l| !l.contains("ruflo daemon start --background"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut child = Command::new("crontab")
                    .arg("-")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| format!("crontab spawn failed: {e}"))?;
                {
                    let stdin = child
                        .stdin
                        .as_mut()
                        .ok_or_else(|| "crontab stdin unavailable".to_string())?;
                    let mut to_write = kept;
                    if !to_write.is_empty() {
                        to_write.push('\n');
                    }
                    stdin
                        .write_all(to_write.as_bytes())
                        .map_err(|e| format!("crontab write failed: {e}"))?;
                }
                let _ = child.wait();
            }
            "systemd" => {
                let unit_name = state["unitName"]
                    .as_str()
                    .unwrap_or(SYSTEMD_UNIT_NAME);
                let _ = Command::new("systemctl")
                    .args(["--user", "disable", unit_name])
                    .status();
                let _ = Command::new("systemctl")
                    .args(["--user", "stop", unit_name])
                    .status();
                if let Some(path_str) = state["path"].as_str() {
                    let _ = fs::remove_file(path_str);
                }
                let _ = Command::new("systemctl")
                    .args(["--user", "daemon-reload"])
                    .status();
            }
            "launchd" => {
                uninstall_launchd(&state)?;
            }
            _ => {}
        }
        let path = state_path("autostart");
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn uninstall_launchd(state: &Value) -> Result<(), String> {
        let path_str = state["path"].as_str().ok_or("missing plist path in state")?;
        let _ = Command::new("launchctl").arg("unload").arg(path_str).status();
        let _ = fs::remove_file(path_str);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn uninstall_launchd(_state: &Value) -> Result<(), String> {
        Err("launchd uninstall is only supported on macOS".to_string())
    }

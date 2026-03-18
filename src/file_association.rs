#[cfg(target_os = "windows")]
pub fn ensure_rc_file_association() {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            log::warn!("Cannot resolve current executable path: {}", err);
            return;
        }
    };
    let exe_str = exe.to_string_lossy().replace('/', "\\");

    let program_id = "RustyCraft.WorldFile";
    let command_value = format!("\"{}\" \"%1\"", exe_str);
    let icon_value = format!("\"{}\",0", exe_str);

    if let Err(err) = reg_add_default_value("HKCU\\Software\\Classes\\.rc", program_id) {
        log::warn!("Failed to register .rc extension: {}", err);
        return;
    }
    if let Err(err) = reg_add_default_value(
        &format!("HKCU\\Software\\Classes\\{}", program_id),
        "RustyCraft World Save",
    ) {
        log::warn!("Failed to register .rc file type: {}", err);
        return;
    }
    if let Err(err) = reg_add_default_value(
        &format!("HKCU\\Software\\Classes\\{}\\DefaultIcon", program_id),
        &icon_value,
    ) {
        log::warn!("Failed to register .rc icon: {}", err);
        return;
    }
    if let Err(err) = reg_add_default_value(
        &format!("HKCU\\Software\\Classes\\{}\\shell\\open\\command", program_id),
        &command_value,
    ) {
        log::warn!("Failed to register .rc open command: {}", err);
        return;
    }

    log::info!("Registered .rc file association for {:?}", exe);
}

#[cfg(target_os = "windows")]
fn reg_add_default_value(key: &str, value: &str) -> Result<(), String> {
    use std::process::Command;

    let output = Command::new("reg")
        .args(["add", key, "/ve", "/t", "REG_SZ", "/d", value, "/f"])
        .output()
        .map_err(|err| format!("reg add failed for '{}': {}", key, err))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("{} ({})", key, stderr.trim()))
}

#[cfg(not(target_os = "windows"))]
pub fn ensure_rc_file_association() {}

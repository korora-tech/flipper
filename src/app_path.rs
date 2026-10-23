//! Resolve the executable path the user invoked, preserving wrapper scripts.

use std::path::{Path, PathBuf};

pub fn invoked() -> Result<PathBuf, String> {
    let argv0 = std::env::args_os()
        .next()
        .ok_or_else(|| "cannot determine the invoked Flipper executable".to_owned())?;
    let path = PathBuf::from(&argv0);
    if path.is_absolute() {
        return Ok(path);
    }
    if path
        .file_name()
        .is_some_and(|name| name != path.as_os_str())
    {
        return std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| format!("cannot resolve Flipper executable: {error}"));
    }

    let name = Path::new(&argv0);
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return if candidate.is_absolute() {
                Ok(candidate)
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(candidate))
                    .map_err(|error| format!("cannot resolve Flipper executable: {error}"))
            };
        }
    }
    Err("cannot find the invoked Flipper executable in PATH".into())
}

pub fn desktop_exec_arg(path: &Path) -> Result<String, String> {
    let path = path
        .to_str()
        .ok_or_else(|| "Flipper executable path is not valid UTF-8".to_owned())?;
    let mut quoted = String::from("\"");
    for ch in path.chars() {
        match ch {
            '%' => quoted.push_str("%%"),
            '"' | '\\' | '$' | '`' => {
                quoted.push('\\');
                quoted.push(ch);
            }
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

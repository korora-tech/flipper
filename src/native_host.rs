//! Chromium/Firefox native messaging bridge for the optional context-menu add-on.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Deserialize;

const MAX_MESSAGE: usize = 1024 * 1024;
const HOST_NAME: &str = "com.korora.flipper";

#[derive(Deserialize)]
struct Request {
    url: String,
}

pub fn run() -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    loop {
        let mut header = [0; 4];
        match input.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(format!("native messaging input: {error}")),
        }
        let length = u32::from_le_bytes(header) as usize;
        if length == 0 || length > MAX_MESSAGE {
            write_response(&mut output, &Response::error("invalid message size"))?;
            return Err(format!("native messaging message size {length} is invalid"));
        }
        let mut bytes = vec![0; length];
        input
            .read_exact(&mut bytes)
            .map_err(|error| format!("native messaging payload: {error}"))?;
        let response = match serde_json::from_slice::<Request>(&bytes) {
            Ok(request) => dispatch(&request.url),
            Err(error) => Response::error(&format!("invalid request: {error}")),
        };
        write_response(&mut output, &response)?;
    }
}

pub fn install(browser: &str, extension_id: &str) -> Result<(), String> {
    if !matches!(browser, "chrome" | "firefox") {
        return Err("browser must be 'chrome' or 'firefox'".into());
    }
    if extension_id.is_empty()
        || extension_id.len() > 128
        || !extension_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '-'))
    {
        return Err("extension ID contains unsupported characters".into());
    }
    if browser == "chrome"
        && (extension_id.len() != 32 || !extension_id.chars().all(|c| ('a'..='p').contains(&c)))
    {
        return Err("Chrome extension IDs must be 32 characters from a through p".into());
    }

    let exe = crate::app_path::invoked()
        .map_err(|error| format!("cannot resolve Flipper executable: {error}"))?;
    let launcher = install_launcher(&exe)?;
    let root = match browser {
        "chrome" => crate::browsers::chromium_user_data_root("google-chrome")
            .ok_or_else(|| "neither Chrome's data root nor HOME is available".to_owned())?
            .join("NativeMessagingHosts"),
        "firefox" => home_root()?.join(".mozilla/native-messaging-hosts"),
        _ => unreachable!(),
    };
    std::fs::create_dir_all(&root).map_err(|error| format!("{}: {error}", root.display()))?;

    let mut manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "Flipper browser picker",
        "path": launcher,
        "type": "stdio",
    });
    if browser == "chrome" {
        manifest["allowed_origins"] =
            serde_json::json!([format!("chrome-extension://{extension_id}/")]);
    } else {
        manifest["allowed_extensions"] = serde_json::json!([extension_id]);
    }
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    let path = root.join(format!("{HOST_NAME}.json"));
    let temp = root.join(format!(".{HOST_NAME}.{}.tmp", std::process::id()));
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|error| format!("{}: {error}", temp.display()))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("{}: {error}", temp.display()))?;
    drop(file);
    std::fs::rename(&temp, &path).map_err(|error| format!("{}: {error}", path.display()))?;
    println!(
        "Installed native messaging host manifest at {}",
        path.display()
    );
    Ok(())
}

fn data_root() -> Result<PathBuf, String> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or_else(|| "neither XDG_DATA_HOME nor HOME is set".to_owned())
}

fn home_root() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_owned())
}

fn install_launcher(exe: &std::path::Path) -> Result<PathBuf, String> {
    let executable = exe
        .to_str()
        .ok_or_else(|| "Flipper executable path is not valid UTF-8".to_owned())?;
    let root = data_root()?.join("flipper");
    std::fs::create_dir_all(&root).map_err(|error| format!("{}: {error}", root.display()))?;
    let path = root.join("flipper-native-host");
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "{} is a symlink; remove it before reinstalling the native host",
                path.display()
            ));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(format!(
                "{} is not a regular file; refusing to replace it",
                path.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {error}", path.display())),
    }
    let temp = root.join(format!(".flipper-native-host.{}.tmp", std::process::id()));
    let launcher = format!(
        "#!/bin/sh\nexec {} --native-host\n",
        shell_quote(executable)
    );
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&temp)
        .map_err(|error| format!("{}: {error}", temp.display()))?;
    if let Err(error) = file
        .write_all(launcher.as_bytes())
        .and_then(|()| file.sync_all())
    {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("{}: {error}", temp.display()));
    }
    drop(file);
    if let Err(error) = std::fs::rename(&temp, &path) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("{}: {error}", path.display()));
    }
    Ok(path)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(serde::Serialize)]
struct Response {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Response {
    fn error(error: &str) -> Self {
        Self {
            ok: false,
            error: Some(error.to_owned()),
        }
    }
}

fn dispatch(url: &str) -> Response {
    if url.len() > 16 * 1024 {
        return Response::error("URL is too long");
    }
    match url::Url::parse(url) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https" | "file") => {}
        Ok(_) => return Response::error("unsupported URL scheme"),
        Err(_) => return Response::error("invalid URL"),
    }
    let exe = match crate::app_path::invoked() {
        Ok(path) => path,
        Err(_) => return Response::error("cannot locate the Flipper executable"),
    };
    match Command::new(exe)
        .arg("--")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => Response {
            ok: true,
            error: None,
        },
        Err(_) => Response::error("could not start Flipper"),
    }
}

fn write_response(output: &mut impl Write, response: &Response) -> Result<(), String> {
    let bytes = serde_json::to_vec(response).map_err(|error| error.to_string())?;
    let length = u32::try_from(bytes.len()).map_err(|_| "response is too large".to_owned())?;
    output
        .write_all(&length.to_le_bytes())
        .and_then(|()| output.write_all(&bytes))
        .and_then(|()| output.flush())
        .map_err(|error| format!("native messaging output: {error}"))
}

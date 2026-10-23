//! Browser process launching. Commands remain argument vectors end to end.

use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use crate::browsers::BrowserProfile;

pub fn launch(profile: &BrowserProfile, url: &str) -> std::io::Result<()> {
    let mut argv = profile.launch.clone();
    let mut replaced = false;
    for argument in &mut argv {
        if argument == "{url}" {
            *argument = url.to_owned();
            replaced = true;
        }
    }
    if !replaced {
        argv.push(url.to_owned());
    }
    launch_argv(&argv)
}

pub fn launch_argv(argv: &[String]) -> std::io::Result<()> {
    let Some((program, args)) = argv.split_first() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "browser command is empty",
        ));
    };

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // SAFETY: setsid is async-signal-safe and touches no Rust state.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

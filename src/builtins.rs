use std::path::{Path, PathBuf};

/// The list of all builtin command names.
const BUILTINS: &[&str] = &["cd", "pwd", "exit", "echo", "export", "unset", "type"];

/// Returns true if the command name is a shell builtin.
pub fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

/// Execute a builtin command. Returns the exit code.
pub fn execute(program: &str, args: &[String]) -> i32 {
    match program {
        "cd" => builtin_cd(args),
        "pwd" => builtin_pwd(),
        "exit" => builtin_exit(args),
        "echo" => builtin_echo(args),
        "export" => builtin_export(args),
        "unset" => builtin_unset(args),
        "type" => builtin_type(args),
        _ => {
            eprintln!("jsh: unknown builtin: {program}");
            1
        }
    }
}

fn builtin_cd(args: &[String]) -> i32 {
    let target = match args.first() {
        Some(dir) if dir == "-" => {
            // cd - : go to previous directory
            match std::env::var("OLDPWD") {
                Ok(prev) => prev,
                Err(_) => {
                    eprintln!("cd: OLDPWD not set");
                    return 1;
                }
            }
        }
        Some(dir) => dir.clone(),
        None => {
            // cd with no args → go home
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string())
        }
    };

    // Save current directory as OLDPWD before changing.
    // SAFETY: We only mutate env vars on the main thread. The ctrlc handler
    // thread does not read or write environment variables.
    if let Ok(cwd) = std::env::current_dir() {
        unsafe { std::env::set_var("OLDPWD", cwd) };
    }

    if let Err(e) = std::env::set_current_dir(&target) {
        eprintln!("cd: {target}: {e}");
        return 1;
    }

    0
}

fn builtin_pwd() -> i32 {
    match std::env::current_dir() {
        Ok(path) => {
            println!("{}", path.display());
            0
        }
        Err(e) => {
            eprintln!("pwd: {e}");
            1
        }
    }
}

fn builtin_exit(args: &[String]) -> i32 {
    match args.first() {
        None => std::process::exit(0),
        Some(s) => match s.parse::<i32>() {
            Ok(code) => std::process::exit(code),
            Err(_) => {
                eprintln!("exit: {s}: numeric argument required");
                std::process::exit(2);
            }
        },
    }
}

fn builtin_echo(args: &[String]) -> i32 {
    println!("{}", args.join(" "));
    0
}

fn builtin_export(args: &[String]) -> i32 {
    for arg in args {
        if let Some((key, value)) = arg.split_once('=') {
            // SAFETY: Env var mutation only happens on the main thread.
            unsafe { std::env::set_var(key, value) };
        } else {
            // export VAR with no value — just mark for export (no-op for now)
            eprintln!("export: usage: export VAR=value");
        }
    }
    0
}

fn builtin_unset(args: &[String]) -> i32 {
    for arg in args {
        // SAFETY: Env var mutation only happens on the main thread.
        unsafe { std::env::remove_var(arg) };
    }
    0
}

fn builtin_type(args: &[String]) -> i32 {
    let mut exit_code = 0;
    for arg in args {
        if is_builtin(arg) {
            println!("{arg} is a shell builtin");
        } else {
            match find_in_path(arg) {
                Some(path) => println!("{arg} is {}", path.display()),
                None => {
                    eprintln!("{arg}: not found");
                    exit_code = 1;
                }
            }
        }
    }
    exit_code
}

/// Check if a path points to an executable file.
fn is_executable(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }

    // On Unix, check the executable permission bits
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return meta.permissions().mode() & 0o111 != 0;
    }

    // On Windows, being a file with the right extension is sufficient
    #[cfg(not(unix))]
    {
        let extension = match path.extension().and_then(|ext| ext.to_str()) {
            Some(ext) => ext.to_ascii_lowercase(),
            None => return false,
        };

        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| {
            ".COM;.EXE;.BAT;.CMD".to_string()
        });
        pathext
            .split(';')
            .any(|ext| extension == ext.trim_start_matches('.').to_ascii_lowercase())
    }
}

/// Search PATH for an executable with the given name.
fn find_in_path(cmd: &str) -> Option<PathBuf> {
    let path_var = std::env::var("PATH").ok()?;
    let separator = if cfg!(windows) { ';' } else { ':' };

    for dir in path_var.split(separator) {
        let full_path = Path::new(dir).join(cmd);
        if is_executable(&full_path) {
            return Some(full_path);
        }
        // On Windows, also try common executable extensions
        if cfg!(windows) {
            for ext in &["exe", "cmd", "bat", "com"] {
                let with_ext = full_path.with_extension(ext);
                if is_executable(&with_ext) {
                    return Some(with_ext);
                }
            }
        }
    }
    None
}

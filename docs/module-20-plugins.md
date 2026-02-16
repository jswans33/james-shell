# Module 20: Plugin System

## What are we building?

No single team can anticipate every use case. The most successful developer tools — VS Code, Vim, Emacs, even bash itself — thrive because of their ecosystems. People extend them in ways the original authors never imagined.

In this module, we build a **plugin system** for james-shell that lets anyone extend the shell with new commands, new completers, new data sources, and new functionality — all without touching the core codebase:

- **Plugins as separate executables** — any language can be a plugin: Rust, Python, Go, even another shell script. They communicate via a protocol over stdin/stdout.
- **JSON-RPC protocol** — structured communication between the shell and plugins. Plugins receive structured data, return structured data.
- **Plugin discovery** — drop a plugin into `~/.jsh/plugins/` and it's available.
- **First-class commands** — plugin commands feel identical to built-in commands. `docker ps | where status == "running"` works whether `docker` is a plugin or a builtin.
- **WASM sandboxing** — optional WebAssembly execution for plugins that need security guarantees.
- **A plugin manager** — `plugin install`, `plugin list`, `plugin remove`.

This is the module where james-shell becomes a *platform* — not just a shell, but a foundation that a community can build on.

---

## Concept 1: Plugin Architecture Overview

A james-shell plugin is a **separate executable** that speaks a defined protocol over stdin/stdout. This design has several important advantages:

```
┌─────────────────────────────────────────────────┐
│                 james-shell (host)               │
│                                                  │
│  ┌──────────┐  ┌──────────┐  ┌──────────────┐  │
│  │ Builtins │  │ External │  │ Plugin       │  │
│  │ (cd, pwd │  │ Commands │  │ Registry     │  │
│  │  etc.)   │  │ (ls, git │  │              │  │
│  │          │  │  etc.)   │  │ ┌──────────┐ │  │
│  └──────────┘  └──────────┘  │ │ Plugin A │ │  │
│                               │ └────┬─────┘ │  │
│                               │      │stdin/ │  │
│                               │      │stdout │  │
│                               │ ┌────┴─────┐ │  │
│                               │ │ Plugin B │ │  │
│                               │ └────┬─────┘ │  │
│                               │      │       │  │
│                               │ ┌────┴─────┐ │  │
│                               │ │ Plugin C │ │  │
│                               │ │ (WASM)   │ │  │
│                               │ └──────────┘ │  │
│                               └──────────────┘  │
└─────────────────────────────────────────────────┘
```

### Why separate processes?

| Approach | Pros | Cons |
|----------|------|------|
| **Shared library (.so/.dll)** | Fast, direct memory access | Crash in plugin kills shell, must be same language/ABI, no sandboxing |
| **Separate process (our choice)** | Language-agnostic, crash-isolated, easy to develop | IPC overhead, serialization cost |
| **WASM module** | Sandboxed, portable, fast startup | Limited system access, newer ecosystem |

We choose separate processes as the primary mechanism because:
1. **Any language can write a plugin.** A Python developer can write a james-shell plugin without learning Rust.
2. **Isolation.** A buggy plugin can't segfault the shell. It runs in its own process, and if it crashes, the shell recovers.
3. **No ABI coupling.** The shell and plugins can be compiled with different Rust versions, or different languages entirely.

We add WASM as an optional secondary mechanism for high-security environments (Concept 9).

---

## Concept 2: The Plugin Protocol (JSON-RPC over stdin/stdout)

Plugins communicate with the shell via [JSON-RPC 2.0](https://www.jsonrpc.org/specification) over their stdin (receiving) and stdout (sending). This is the same approach used by LSP (Language Server Protocol), which powers VS Code extensions.

### Protocol flow

```
Shell                              Plugin
  │                                  │
  │──── Handshake (initialize) ────>│
  │<─── Plugin manifest ───────────│
  │                                  │
  │  (user runs a plugin command)    │
  │                                  │
  │──── Run command request ──────>│
  │<─── Progress/output stream ────│
  │<─── Final result ──────────────│
  │                                  │
  │  (shell is shutting down)        │
  │                                  │
  │──── Shutdown request ─────────>│
  │<─── Shutdown acknowledgment ───│
  │                                  │
```

### Message types

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// A JSON-RPC request from the shell to a plugin.
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,  // always "2.0"
    pub id: u64,
    pub method: String,
    pub params: Option<JsonValue>,
}

/// A JSON-RPC response from a plugin to the shell.
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// A JSON-RPC error.
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<JsonValue>,
}

/// A notification (no response expected).
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<JsonValue>,
}
```

### Protocol methods

| Method | Direction | Purpose |
|--------|-----------|---------|
| `initialize` | Shell -> Plugin | Start the plugin, get its manifest |
| `shutdown` | Shell -> Plugin | Graceful shutdown |
| `run` | Shell -> Plugin | Execute a command |
| `complete` | Shell -> Plugin | Get tab completions |
| `output` | Plugin -> Shell | Stream output back (notification) |
| `progress` | Plugin -> Shell | Report progress (notification) |
| `get_config` | Plugin -> Shell | Read shell configuration |
| `get_env` | Plugin -> Shell | Read environment variables |

### The initialize handshake

When a plugin starts, the shell sends an `initialize` request. The plugin responds with its manifest — what commands it provides, what it can complete, what it needs.

```rust
/// The manifest a plugin returns during initialization.
#[derive(Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Human-readable name.
    pub name: String,
    /// Semantic version.
    pub version: String,
    /// Short description.
    pub description: String,
    /// Commands this plugin provides.
    pub commands: Vec<PluginCommand>,
    /// Does this plugin provide custom completions?
    pub completions: Vec<PluginCompletion>,
    /// Minimum james-shell version required.
    pub min_shell_version: Option<String>,
    /// Permissions the plugin requests.
    pub permissions: Vec<Permission>,
}

/// A command provided by a plugin.
#[derive(Debug, Serialize, Deserialize)]
pub struct PluginCommand {
    /// The command name (what the user types).
    pub name: String,
    /// Short description for help text.
    pub description: String,
    /// Usage string for help.
    pub usage: String,
    /// Does this command accept pipeline input?
    pub accepts_input: bool,
    /// Parameter definitions for validation and completion.
    pub parameters: Vec<PluginParameter>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginParameter {
    pub name: String,
    pub description: String,
    pub param_type: String,  // "string", "int", "bool", etc.
    pub required: bool,
    pub default: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginCompletion {
    /// Which command this completer handles.
    pub command: String,
    /// Which argument position(s).
    pub positions: Vec<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Permission {
    FileRead,
    FileWrite,
    Network,
    ProcessSpawn,
    EnvRead,
    EnvWrite,
}
```

---

## Concept 3: Plugin Discovery and Lifecycle

### Where plugins live

```
~/.jsh/plugins/
├── jsh-docker/
│   ├── manifest.json          # static manifest (for discovery without starting)
│   └── jsh-docker(.exe)       # the plugin executable
├── jsh-git-extras/
│   ├── manifest.json
│   └── jsh-git-extras(.exe)
└── jsh-k8s/
    ├── manifest.json
    └── jsh-k8s.wasm           # WASM plugin
```

### Plugin naming convention

Plugins follow the naming convention `jsh-<name>`. When the user types `docker ps`, the shell:

1. Checks if `docker` is a builtin -- no.
2. Checks if `docker` is registered by a plugin -- checks for `jsh-docker` plugin.
3. Checks PATH for an external `docker` command.

### The Plugin Manager

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::io::{BufRead, BufReader, Write};

/// Manages plugin discovery, lifecycle, and communication.
pub struct PluginManager {
    /// Directory where plugins are installed.
    plugin_dir: PathBuf,
    /// Loaded plugins with their manifests and running processes.
    plugins: HashMap<String, LoadedPlugin>,
    /// Map from command name to the plugin that provides it.
    command_map: HashMap<String, String>,
}

struct LoadedPlugin {
    manifest: PluginManifest,
    executable: PathBuf,
    /// The running process, if the plugin is started.
    /// Plugins are started lazily (on first use).
    process: Option<PluginProcess>,
}

struct PluginProcess {
    child: Child,
    stdin: std::io::BufWriter<std::process::ChildStdin>,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl PluginManager {
    pub fn new() -> Result<Self, ShellError> {
        let plugin_dir = dirs::home_dir()
            .ok_or_else(|| ShellError::new(
                "cannot determine home directory",
                ErrorCategory::IOError,
            ))?
            .join(".jsh")
            .join("plugins");

        // Create the plugins directory if it doesn't exist
        std::fs::create_dir_all(&plugin_dir).map_err(|e| {
            ShellError::new(
                format!("cannot create plugin directory: {}", e),
                ErrorCategory::IOError,
            )
        })?;

        let mut manager = Self {
            plugin_dir,
            plugins: HashMap::new(),
            command_map: HashMap::new(),
        };

        manager.discover_plugins()?;
        Ok(manager)
    }

    /// Scan the plugins directory for installed plugins.
    fn discover_plugins(&mut self) -> Result<(), ShellError> {
        let entries = std::fs::read_dir(&self.plugin_dir).map_err(|e| {
            ShellError::new(
                format!("cannot read plugin directory: {}", e),
                ErrorCategory::IOError,
            )
        })?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let dir_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            // Look for manifest.json
            let manifest_path = path.join("manifest.json");
            if !manifest_path.exists() {
                continue;
            }

            let manifest_content = std::fs::read_to_string(&manifest_path)
                .map_err(|e| {
                    ShellError::new(
                        format!("cannot read manifest for {}: {}", dir_name, e),
                        ErrorCategory::IOError,
                    )
                })?;

            let manifest: PluginManifest = serde_json::from_str(&manifest_content)
                .map_err(|e| {
                    ShellError::new(
                        format!("invalid manifest for {}: {}", dir_name, e),
                        ErrorCategory::SyntaxError,
                    )
                })?;

            // Find the executable
            let exe_name = if cfg!(windows) {
                format!("{}.exe", dir_name)
            } else {
                dir_name.clone()
            };
            let exe_path = path.join(&exe_name);

            // Or check for a WASM file
            let wasm_path = path.join(format!("{}.wasm", dir_name));

            let executable = if exe_path.exists() {
                exe_path
            } else if wasm_path.exists() {
                wasm_path
            } else {
                eprintln!("Warning: plugin {} has no executable", dir_name);
                continue;
            };

            // Register commands from this plugin
            for cmd in &manifest.commands {
                self.command_map.insert(
                    cmd.name.clone(),
                    manifest.name.clone(),
                );
            }

            self.plugins.insert(manifest.name.clone(), LoadedPlugin {
                manifest,
                executable,
                process: None,
            });
        }

        Ok(())
    }

    /// Check if a command is provided by a plugin.
    pub fn has_command(&self, name: &str) -> bool {
        self.command_map.contains_key(name)
    }

    /// Start a plugin's process (lazily, on first use).
    fn ensure_started(&mut self, plugin_name: &str) -> Result<(), ShellError> {
        let plugin = self.plugins.get_mut(plugin_name).ok_or_else(|| {
            ShellError::new(
                format!("plugin not found: {}", plugin_name),
                ErrorCategory::PluginError,
            )
        })?;

        if plugin.process.is_some() {
            return Ok(()); // already running
        }

        // Start the plugin process
        let mut child = Command::new(&plugin.executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // let plugin errors show in terminal
            .spawn()
            .map_err(|e| {
                ShellError::new(
                    format!("cannot start plugin {}: {}", plugin_name, e),
                    ErrorCategory::PluginError,
                )
            })?;

        let stdin = std::io::BufWriter::new(
            child.stdin.take().expect("stdin was piped"),
        );
        let stdout = BufReader::new(
            child.stdout.take().expect("stdout was piped"),
        );

        plugin.process = Some(PluginProcess {
            child,
            stdin,
            stdout,
            next_id: 1,
        });

        // Send the initialize request
        self.send_request(plugin_name, "initialize", Some(serde_json::json!({
            "shell_version": env!("CARGO_PKG_VERSION"),
            "protocol_version": "1.0",
        })))?;

        Ok(())
    }
}
```

---

## Concept 4: Running Plugin Commands

When the user runs a command that's provided by a plugin, the shell serializes the command as a JSON-RPC request, sends it to the plugin process, and reads back the structured result.

```rust
impl PluginManager {
    /// Execute a plugin command.
    pub fn run_command(
        &mut self,
        command_name: &str,
        args: &[Value],
        input: Option<Value>,  // pipeline input
    ) -> Result<Value, ShellError> {
        let plugin_name = self.command_map.get(command_name)
            .ok_or_else(|| ShellError::new(
                format!("no plugin provides command: {}", command_name),
                ErrorCategory::CommandNotFound,
            ))?
            .clone();

        self.ensure_started(&plugin_name)?;

        // Build the run request
        let params = serde_json::json!({
            "command": command_name,
            "args": args.iter().map(value_to_json).collect::<Vec<_>>(),
            "input": input.as_ref().map(value_to_json),
        });

        let response = self.send_request(&plugin_name, "run", Some(params))?;

        // Process any streaming output from the plugin
        // (the plugin may send `output` notifications before the final response)

        // Convert the JSON result back to a shell Value
        match response {
            Some(json_value) => json_to_value(&json_value),
            None => Ok(Value::Null),
        }
    }

    /// Send a JSON-RPC request and wait for the response.
    fn send_request(
        &mut self,
        plugin_name: &str,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<Option<serde_json::Value>, ShellError> {
        let plugin = self.plugins.get_mut(plugin_name)
            .ok_or_else(|| ShellError::new(
                format!("plugin not loaded: {}", plugin_name),
                ErrorCategory::PluginError,
            ))?;

        let process = plugin.process.as_mut()
            .ok_or_else(|| ShellError::new(
                format!("plugin not started: {}", plugin_name),
                ErrorCategory::PluginError,
            ))?;

        let id = process.next_id;
        process.next_id += 1;

        // Build and send the request
        let request = RpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        let request_json = serde_json::to_string(&request).map_err(|e| {
            ShellError::new(
                format!("cannot serialize request: {}", e),
                ErrorCategory::PluginError,
            )
        })?;

        writeln!(process.stdin, "{}", request_json).map_err(|e| {
            ShellError::new(
                format!("cannot send to plugin: {}", e),
                ErrorCategory::PluginError,
            )
        })?;
        process.stdin.flush().map_err(|e| {
            ShellError::new(
                format!("cannot flush plugin stdin: {}", e),
                ErrorCategory::PluginError,
            )
        })?;

        // Read the response (handle notifications in between)
        loop {
            let mut line = String::new();
            process.stdout.read_line(&mut line).map_err(|e| {
                ShellError::new(
                    format!("cannot read from plugin: {}", e),
                    ErrorCategory::PluginError,
                )
            })?;

            if line.trim().is_empty() {
                continue;
            }

            // Try to parse as a response
            if let Ok(response) = serde_json::from_str::<RpcResponse>(&line) {
                if response.id == id {
                    // This is our response
                    if let Some(error) = response.error {
                        return Err(ShellError::new(
                            error.message,
                            ErrorCategory::PluginError,
                        ).with_code(error.code as i32));
                    }
                    return Ok(response.result);
                }
            }

            // Try to parse as a notification
            if let Ok(notification) = serde_json::from_str::<RpcNotification>(&line) {
                match notification.method.as_str() {
                    "output" => {
                        // Plugin is streaming output — print it
                        if let Some(params) = notification.params {
                            if let Some(text) = params.get("text").and_then(|t| t.as_str()) {
                                print!("{}", text);
                            }
                        }
                    }
                    "progress" => {
                        // Plugin is reporting progress
                        if let Some(params) = notification.params {
                            if let Some(msg) = params.get("message").and_then(|m| m.as_str()) {
                                eprint!("\r{}", msg);
                            }
                        }
                    }
                    _ => {} // unknown notification, ignore
                }
            }
        }
    }
}

/// Convert a shell Value to JSON for the plugin protocol.
fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Int(n) => serde_json::json!(n),
        Value::Float(f) => serde_json::json!(f),
        Value::Bool(b) => serde_json::json!(b),
        Value::List(items) => {
            serde_json::Value::Array(items.iter().map(value_to_json).collect())
        }
        Value::Record(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map.iter()
                .map(|(k, v)| (k.clone(), value_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        Value::Error(e) => serde_json::json!({
            "error": e.message,
            "code": e.code,
        }),
        // Closures and ranges can't be serialized to plugins
        _ => serde_json::Value::Null,
    }
}

/// Convert JSON from a plugin back to a shell Value.
fn json_to_value(json: &serde_json::Value) -> Result<Value, ShellError> {
    match json {
        serde_json::Value::Null => Ok(Value::Null),
        serde_json::Value::Bool(b) => Ok(Value::Bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Int(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::Float(f))
            } else {
                Ok(Value::String(n.to_string()))
            }
        }
        serde_json::Value::String(s) => Ok(Value::String(s.clone())),
        serde_json::Value::Array(arr) => {
            let items: Result<Vec<Value>, _> = arr.iter().map(json_to_value).collect();
            Ok(Value::List(items?))
        }
        serde_json::Value::Object(obj) => {
            let map: Result<HashMap<String, Value>, _> = obj.iter()
                .map(|(k, v)| json_to_value(v).map(|val| (k.clone(), val)))
                .collect();
            Ok(Value::Record(map?))
        }
    }
}
```

---

## Concept 5: Plugin Manager Commands

Users interact with plugins through built-in commands:

```
# List installed plugins
jsh> plugin list
╭───┬────────────────┬─────────┬──────────────────────────────────────╮
│ # │      name      │ version │             description              │
├───┼────────────────┼─────────┼──────────────────────────────────────┤
│ 0 │ jsh-docker     │  0.3.1  │ Docker commands with structured data │
│ 1 │ jsh-git-extras │  1.0.0  │ Enhanced git commands                │
│ 2 │ jsh-k8s        │  0.1.0  │ Kubernetes management                │
╰───┴────────────────┴─────────┴──────────────────────────────────────╯

# Install a plugin from a registry or git URL
jsh> plugin install jsh-docker
Downloading jsh-docker v0.3.1...
Installing to ~/.jsh/plugins/jsh-docker/
Plugin jsh-docker installed successfully.
Commands available: docker-ps, docker-images, docker-logs

# Remove a plugin
jsh> plugin remove jsh-docker
Removed plugin jsh-docker.

# Show details about a plugin
jsh> plugin info jsh-docker
Name: jsh-docker
Version: 0.3.1
Description: Docker commands with structured data
Commands:
  docker-ps      - List containers as structured records
  docker-images  - List images as structured records
  docker-logs    - Stream container logs
Permissions:
  - ProcessSpawn (runs docker CLI)
  - Network (connects to Docker daemon)
```

### Implementation of plugin commands

```rust
fn builtin_plugin(state: &mut ShellState, args: &[String]) -> Result<Value, ShellError> {
    match args.first().map(|s| s.as_str()) {
        Some("list") => plugin_list(state),
        Some("install") => plugin_install(state, &args[1..]),
        Some("remove") => plugin_remove(state, &args[1..]),
        Some("info") => plugin_info(state, &args[1..]),
        Some("update") => plugin_update(state, &args[1..]),
        Some(sub) => Err(ShellError::new(
            format!("unknown plugin subcommand: {}", sub),
            ErrorCategory::CommandNotFound,
        )),
        None => Err(ShellError::new(
            "usage: plugin [list|install|remove|info|update]",
            ErrorCategory::TypeError,
        )),
    }
}

fn plugin_list(state: &ShellState) -> Result<Value, ShellError> {
    let plugins: Vec<Value> = state.plugin_manager.list_plugins()
        .iter()
        .map(|p| {
            let mut record = HashMap::new();
            record.insert("name".into(), Value::String(p.name.clone()));
            record.insert("version".into(), Value::String(p.version.clone()));
            record.insert("description".into(), Value::String(p.description.clone()));
            record.insert("commands".into(), Value::Int(p.commands.len() as i64));
            Value::Record(record)
        })
        .collect();

    Ok(Value::List(plugins))
}

fn plugin_install(state: &mut ShellState, args: &[String]) -> Result<Value, ShellError> {
    let name = args.first().ok_or_else(|| {
        ShellError::new("plugin install requires a name", ErrorCategory::TypeError)
    })?;

    // Check if it's a URL or a registry name
    if name.starts_with("https://") || name.starts_with("git@") {
        install_from_git(state, name)
    } else if name.ends_with(".wasm") {
        install_from_file(state, name)
    } else {
        install_from_registry(state, name)
    }
}

fn install_from_git(state: &mut ShellState, url: &str) -> Result<Value, ShellError> {
    let plugin_dir = &state.plugin_manager.plugin_dir;

    // Extract the plugin name from the URL
    let name = url
        .rsplit('/')
        .next()
        .unwrap_or("unknown")
        .trim_end_matches(".git");

    let target_dir = plugin_dir.join(name);

    // Clone the repository
    let output = std::process::Command::new("git")
        .args(["clone", "--depth", "1", url])
        .arg(&target_dir)
        .output()
        .map_err(|e| {
            ShellError::new(
                format!("git clone failed: {}", e),
                ErrorCategory::CommandFailed,
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ShellError::new(
            format!("git clone failed: {}", stderr),
            ErrorCategory::CommandFailed,
        ));
    }

    // If it's a Rust project, build it
    let cargo_toml = target_dir.join("Cargo.toml");
    if cargo_toml.exists() {
        eprintln!("Building plugin {}...", name);
        let build_output = std::process::Command::new("cargo")
            .args(["build", "--release"])
            .current_dir(&target_dir)
            .output()
            .map_err(|e| {
                ShellError::new(
                    format!("cargo build failed: {}", e),
                    ErrorCategory::CommandFailed,
                )
            })?;

        if !build_output.status.success() {
            let stderr = String::from_utf8_lossy(&build_output.stderr);
            return Err(ShellError::new(
                format!("build failed: {}", stderr),
                ErrorCategory::CommandFailed,
            ));
        }

        // Copy the built executable into the plugin directory
        let exe_name = if cfg!(windows) {
            format!("{}.exe", name)
        } else {
            name.to_string()
        };
        let built = target_dir.join("target").join("release").join(&exe_name);
        let dest = target_dir.join(&exe_name);
        std::fs::copy(&built, &dest).map_err(|e| {
            ShellError::new(
                format!("cannot copy executable: {}", e),
                ErrorCategory::IOError,
            )
        })?;
    }

    // Re-discover plugins to pick up the new one
    state.plugin_manager.discover_plugins()?;

    Ok(Value::String(format!("Plugin {} installed successfully.", name)))
}
```

---

## Concept 6: Writing a Plugin in Rust

Let's build a complete example plugin: `jsh-docker`, which wraps Docker commands to return structured data instead of plain text.

### Project structure

```
jsh-docker/
├── Cargo.toml
├── manifest.json
└── src/
    └── main.rs
```

### manifest.json

```json
{
    "name": "jsh-docker",
    "version": "0.3.1",
    "description": "Docker commands with structured data output",
    "commands": [
        {
            "name": "docker-ps",
            "description": "List Docker containers as structured records",
            "usage": "docker-ps [--all] [--filter <key=value>]",
            "accepts_input": false,
            "parameters": [
                {
                    "name": "all",
                    "description": "Show all containers (including stopped)",
                    "param_type": "bool",
                    "required": false,
                    "default": false
                },
                {
                    "name": "filter",
                    "description": "Filter containers by key=value",
                    "param_type": "string",
                    "required": false,
                    "default": null
                }
            ]
        },
        {
            "name": "docker-images",
            "description": "List Docker images as structured records",
            "usage": "docker-images [--filter <key=value>]",
            "accepts_input": false,
            "parameters": []
        },
        {
            "name": "docker-logs",
            "description": "Stream container logs",
            "usage": "docker-logs <container> [--tail <n>] [--follow]",
            "accepts_input": false,
            "parameters": [
                {
                    "name": "container",
                    "description": "Container name or ID",
                    "param_type": "string",
                    "required": true,
                    "default": null
                },
                {
                    "name": "tail",
                    "description": "Number of lines from the end",
                    "param_type": "int",
                    "required": false,
                    "default": 100
                }
            ]
        }
    ],
    "completions": [
        {
            "command": "docker-logs",
            "positions": [0]
        }
    ],
    "permissions": ["ProcessSpawn"]
}
```

### Cargo.toml

```toml
[package]
name = "jsh-docker"
version = "0.3.1"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

### src/main.rs — The complete plugin

```rust
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::io::{self, BufRead, Write};
use std::process::Command;

// --- JSON-RPC types ---

#[derive(Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    params: Option<JsonValue>,
}

#[derive(Serialize)]
struct RpcResponse {
    jsonrpc: String,
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

// --- Plugin logic ---

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: RpcRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                eprintln!("jsh-docker: invalid request: {}", e);
                continue;
            }
        };

        let response = handle_request(&request);

        let response_json = serde_json::to_string(&response).unwrap();
        writeln!(stdout, "{}", response_json).unwrap();
        stdout.flush().unwrap();
    }
}

fn handle_request(req: &RpcRequest) -> RpcResponse {
    match req.method.as_str() {
        "initialize" => RpcResponse {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: Some(json!({
                "name": "jsh-docker",
                "version": "0.3.1",
                "protocol": "1.0"
            })),
            error: None,
        },

        "run" => {
            let params = req.params.as_ref().unwrap();
            let command = params["command"].as_str().unwrap_or("");

            let result = match command {
                "docker-ps" => run_docker_ps(params),
                "docker-images" => run_docker_images(params),
                "docker-logs" => run_docker_logs(params),
                _ => Err(format!("unknown command: {}", command)),
            };

            match result {
                Ok(value) => RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: Some(value),
                    error: None,
                },
                Err(msg) => RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(RpcError {
                        code: 1,
                        message: msg,
                    }),
                },
            }
        }

        "complete" => {
            let params = req.params.as_ref().unwrap();
            let result = handle_completion(params);
            RpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(result),
                error: None,
            }
        }

        "shutdown" => RpcResponse {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: Some(json!({"status": "ok"})),
            error: None,
        },

        _ => RpcResponse {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: None,
            error: Some(RpcError {
                code: -32601,
                message: format!("method not found: {}", req.method),
            }),
        },
    }
}

/// Run `docker ps` and return structured output.
fn run_docker_ps(params: &JsonValue) -> Result<JsonValue, String> {
    let mut cmd = Command::new("docker");
    cmd.args(["ps", "--format", "{{json .}}"]);

    // Handle --all flag
    let args = params.get("args").and_then(|a| a.as_array());
    if let Some(args) = args {
        for arg in args {
            if arg.as_str() == Some("--all") || arg.as_str() == Some("-a") {
                cmd.arg("--all");
            }
        }
    }

    let output = cmd.output().map_err(|e| format!("cannot run docker: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("docker ps failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let containers: Vec<JsonValue> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let docker_json: JsonValue = serde_json::from_str(line).ok()?;

            // Transform Docker's JSON format into our structured format
            Some(json!({
                "id": docker_json["ID"],
                "name": docker_json["Names"],
                "image": docker_json["Image"],
                "status": docker_json["Status"],
                "state": docker_json["State"],
                "ports": docker_json["Ports"],
                "created": docker_json["CreatedAt"],
                "size": docker_json["Size"],
            }))
        })
        .collect();

    Ok(json!(containers))
}

/// Run `docker images` and return structured output.
fn run_docker_images(params: &JsonValue) -> Result<JsonValue, String> {
    let output = Command::new("docker")
        .args(["images", "--format", "{{json .}}"])
        .output()
        .map_err(|e| format!("cannot run docker: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("docker images failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let images: Vec<JsonValue> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let docker_json: JsonValue = serde_json::from_str(line).ok()?;
            Some(json!({
                "repository": docker_json["Repository"],
                "tag": docker_json["Tag"],
                "id": docker_json["ID"],
                "created": docker_json["CreatedAt"],
                "size": docker_json["Size"],
            }))
        })
        .collect();

    Ok(json!(images))
}

/// Run `docker logs` for a specific container.
fn run_docker_logs(params: &JsonValue) -> Result<JsonValue, String> {
    let args = params.get("args").and_then(|a| a.as_array())
        .ok_or("docker-logs requires a container name")?;

    let container = args.first()
        .and_then(|a| a.as_str())
        .ok_or("docker-logs requires a container name")?;

    let tail = args.iter()
        .position(|a| a.as_str() == Some("--tail"))
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.as_str())
        .unwrap_or("100");

    let output = Command::new("docker")
        .args(["logs", "--tail", tail, container])
        .output()
        .map_err(|e| format!("cannot run docker logs: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("docker logs failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<JsonValue> = stdout
        .lines()
        .map(|line| json!(line))
        .collect();

    Ok(json!(lines))
}

/// Handle tab completion requests.
fn handle_completion(params: &JsonValue) -> JsonValue {
    let command = params.get("command").and_then(|c| c.as_str()).unwrap_or("");
    let partial = params.get("partial").and_then(|p| p.as_str()).unwrap_or("");

    match command {
        "docker-logs" => {
            // Complete container names
            let output = Command::new("docker")
                .args(["ps", "--format", "{{.Names}}"])
                .output();

            match output {
                Ok(out) => {
                    let names: Vec<JsonValue> = String::from_utf8_lossy(&out.stdout)
                        .lines()
                        .filter(|name| name.starts_with(partial))
                        .map(|name| json!({
                            "value": name,
                            "description": "container"
                        }))
                        .collect();
                    json!(names)
                }
                Err(_) => json!([]),
            }
        }
        _ => json!([]),
    }
}
```

### Using the plugin

Once installed, the plugin's commands work seamlessly in the shell:

```
jsh> docker-ps
╭───┬──────────────┬──────────────┬──────────────┬───────────╮
│ # │     name     │    image     │    status    │   state   │
├───┼──────────────┼──────────────┼──────────────┼───────────┤
│ 0 │ web-app      │ nginx:latest │ Up 2 hours   │ running   │
│ 1 │ api-server   │ node:18      │ Up 2 hours   │ running   │
│ 2 │ postgres-db  │ postgres:15  │ Up 2 hours   │ running   │
╰───┴──────────────┴──────────────┴──────────────┴───────────╯

jsh> docker-ps | where state == "running" | each { |c| $c.name }
[web-app, api-server, postgres-db]

jsh> docker-images | sort-by { |i| $i.size } | first 3
╭───┬────────────┬────────┬─────────╮
│ # │ repository │  tag   │  size   │
├───┼────────────┼────────┼─────────┤
│ 0 │ alpine     │ latest │ 7.04MB  │
│ 1 │ nginx      │ latest │ 142MB   │
│ 2 │ node       │ 18     │ 998MB   │
╰───┴────────────┴────────┴─────────╯
```

---

## Concept 7: The Plugin Cargo Template

To make it easy for developers to create new plugins, we provide a cargo template:

```
jsh> plugin new my-plugin
Creating plugin template at ~/.jsh/plugins/jsh-my-plugin/...
  Created Cargo.toml
  Created manifest.json
  Created src/main.rs
  Created README.md

Plugin jsh-my-plugin created. To get started:
  cd ~/.jsh/plugins/jsh-my-plugin
  cargo build
  # Your plugin is now available in james-shell
```

### Template generator

```rust
fn plugin_new(name: &str) -> Result<Value, ShellError> {
    let plugin_name = if name.starts_with("jsh-") {
        name.to_string()
    } else {
        format!("jsh-{}", name)
    };

    let plugin_dir = dirs::home_dir()
        .ok_or_else(|| ShellError::new("cannot find home", ErrorCategory::IOError))?
        .join(".jsh")
        .join("plugins")
        .join(&plugin_name);

    std::fs::create_dir_all(plugin_dir.join("src")).map_err(|e| {
        ShellError::new(format!("cannot create directory: {}", e), ErrorCategory::IOError)
    })?;

    // Generate Cargo.toml
    let cargo_toml = format!(r#"[package]
name = "{plugin_name}"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#);

    std::fs::write(plugin_dir.join("Cargo.toml"), cargo_toml).map_err(|e| {
        ShellError::new(format!("cannot write Cargo.toml: {}", e), ErrorCategory::IOError)
    })?;

    // Generate manifest.json
    let short_name = plugin_name.strip_prefix("jsh-").unwrap_or(&plugin_name);
    let manifest = serde_json::json!({
        "name": plugin_name,
        "version": "0.1.0",
        "description": format!("A james-shell plugin: {}", short_name),
        "commands": [
            {
                "name": short_name,
                "description": format!("Run the {} command", short_name),
                "usage": format!("{} [args...]", short_name),
                "accepts_input": true,
                "parameters": []
            }
        ],
        "completions": [],
        "permissions": []
    });

    std::fs::write(
        plugin_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    ).map_err(|e| {
        ShellError::new(format!("cannot write manifest: {}", e), ErrorCategory::IOError)
    })?;

    // Generate src/main.rs (skeleton)
    let main_rs = format!(r#"//! {plugin_name} — a james-shell plugin
//!
//! This plugin provides the `{short_name}` command.

use serde::{{Deserialize, Serialize}};
use serde_json::{{json, Value as JsonValue}};
use std::io::{{self, BufRead, Write}};

#[derive(Deserialize)]
struct RpcRequest {{
    id: u64,
    method: String,
    params: Option<JsonValue>,
}}

#[derive(Serialize)]
struct RpcResponse {{
    jsonrpc: String,
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}}

#[derive(Serialize)]
struct RpcError {{
    code: i64,
    message: String,
}}

fn main() {{
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {{
        let line = match line {{
            Ok(l) => l,
            Err(_) => break,
        }};

        if line.trim().is_empty() {{
            continue;
        }}

        let request: RpcRequest = match serde_json::from_str(&line) {{
            Ok(req) => req,
            Err(e) => {{
                eprintln!("{plugin_name}: invalid request: {{}}", e);
                continue;
            }}
        }};

        let response = handle_request(&request);
        let response_json = serde_json::to_string(&response).unwrap();
        writeln!(stdout, "{{}}", response_json).unwrap();
        stdout.flush().unwrap();
    }}
}}

fn handle_request(req: &RpcRequest) -> RpcResponse {{
    match req.method.as_str() {{
        "initialize" => RpcResponse {{
            jsonrpc: "2.0".into(),
            id: req.id,
            result: Some(json!({{
                "name": "{plugin_name}",
                "version": "0.1.0",
                "protocol": "1.0"
            }})),
            error: None,
        }},

        "run" => {{
            // TODO: Implement your command logic here
            let params = req.params.as_ref();
            let command = params
                .and_then(|p| p["command"].as_str())
                .unwrap_or("");

            match command {{
                "{short_name}" => {{
                    // Your command implementation goes here
                    RpcResponse {{
                        jsonrpc: "2.0".into(),
                        id: req.id,
                        result: Some(json!("Hello from {plugin_name}!")),
                        error: None,
                    }}
                }}
                _ => RpcResponse {{
                    jsonrpc: "2.0".into(),
                    id: req.id,
                    result: None,
                    error: Some(RpcError {{
                        code: -32601,
                        message: format!("unknown command: {{}}", command),
                    }}),
                }},
            }}
        }}

        "shutdown" => RpcResponse {{
            jsonrpc: "2.0".into(),
            id: req.id,
            result: Some(json!({{"status": "ok"}})),
            error: None,
        }},

        _ => RpcResponse {{
            jsonrpc: "2.0".into(),
            id: req.id,
            result: None,
            error: Some(RpcError {{
                code: -32601,
                message: format!("method not found: {{}}", req.method),
            }}),
        }},
    }}
}}
"#);

    std::fs::write(plugin_dir.join("src").join("main.rs"), main_rs).map_err(|e| {
        ShellError::new(format!("cannot write main.rs: {}", e), ErrorCategory::IOError)
    })?;

    Ok(Value::String(format!("Plugin {} created at {}", plugin_name, plugin_dir.display())))
}
```

---

## Concept 8: Security Considerations

Plugins run as separate processes with full system access. This is powerful but dangerous — a malicious plugin could read your SSH keys, install malware, or exfiltrate data. We mitigate this with several layers:

### Layer 1: Permission declarations

Plugins must declare their permissions in `manifest.json`. The shell warns the user during installation:

```
jsh> plugin install jsh-sketchy-tool
Plugin jsh-sketchy-tool requests the following permissions:
  - FileRead    (read files on your system)
  - FileWrite   (write files on your system)
  - Network     (make network connections)
  - ProcessSpawn (run other programs)

Do you want to install this plugin? [y/N]
```

### Layer 2: Runtime permission enforcement (for WASM plugins)

WASM plugins run in a sandboxed environment where permissions can actually be enforced:

```rust
/// Permissions that can be granted to a WASM plugin.
#[derive(Debug, Clone)]
pub struct WasmPermissions {
    /// Directories the plugin can read from.
    pub read_paths: Vec<PathBuf>,
    /// Directories the plugin can write to.
    pub write_paths: Vec<PathBuf>,
    /// Whether the plugin can access the network.
    pub network: bool,
    /// Whether the plugin can spawn processes.
    pub spawn: bool,
    /// Environment variables the plugin can read.
    pub env_vars: Vec<String>,
}
```

### Layer 3: Plugin signatures

For plugins distributed through a registry, we can verify cryptographic signatures:

```rust
/// Verify that a plugin's executable hasn't been tampered with.
fn verify_plugin_signature(
    plugin_dir: &Path,
    expected_hash: &str,
) -> Result<bool, ShellError> {
    use sha2::{Sha256, Digest};

    let exe_name = if cfg!(windows) {
        plugin_dir.file_name().unwrap().to_str().unwrap().to_string() + ".exe"
    } else {
        plugin_dir.file_name().unwrap().to_str().unwrap().to_string()
    };

    let exe_path = plugin_dir.join(&exe_name);
    let bytes = std::fs::read(&exe_path).map_err(|e| {
        ShellError::new(
            format!("cannot read plugin executable: {}", e),
            ErrorCategory::IOError,
        )
    })?;

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let hash = format!("{:x}", hasher.finalize());

    Ok(hash == expected_hash)
}
```

### Security best practices summary

| Risk | Mitigation |
|------|------------|
| Malicious code execution | Permission declarations, user confirmation |
| Supply chain attacks | Signature verification, pinned versions |
| Data exfiltration | Network permission required, WASM sandboxing |
| File system damage | Write permission required, WASM path restrictions |
| Resource exhaustion | Process limits (memory, CPU), timeout on RPC calls |
| Protocol abuse | Input validation, message size limits |

---

## Concept 9: WASM Plugins for Sandboxed Execution

For environments that need stronger security guarantees, james-shell supports WebAssembly plugins using the `wasmtime` runtime. WASM plugins run in a sandbox where the host (james-shell) controls exactly what resources are available.

### How WASM plugins differ

| Aspect | Native plugins | WASM plugins |
|--------|---------------|--------------|
| Language | Any (compiled to native) | Any (compiled to WASM) |
| Speed | Native speed | Near-native (WASM JIT) |
| Sandbox | None (process isolation only) | Full sandbox (memory, filesystem, network) |
| File extension | `jsh-name` or `jsh-name.exe` | `jsh-name.wasm` |
| System access | Full (OS process) | Only what the host grants |
| Startup cost | Process spawn (ms) | WASM instantiation (sub-ms after cache) |

### Using wasmtime to run WASM plugins

```rust
use wasmtime::*;
use wasmtime_wasi::*;

pub struct WasmPlugin {
    engine: Engine,
    module: Module,
    permissions: WasmPermissions,
}

impl WasmPlugin {
    /// Load a WASM plugin from a .wasm file.
    pub fn load(
        wasm_path: &Path,
        permissions: WasmPermissions,
    ) -> Result<Self, ShellError> {
        let engine = Engine::default();

        let module = Module::from_file(&engine, wasm_path).map_err(|e| {
            ShellError::new(
                format!("cannot load WASM module: {}", e),
                ErrorCategory::PluginError,
            )
        })?;

        Ok(Self {
            engine,
            module,
            permissions,
        })
    }

    /// Execute a command in the WASM sandbox.
    pub fn run_command(
        &self,
        command: &str,
        args: &[Value],
        input: Option<Value>,
    ) -> Result<Value, ShellError> {
        let mut linker = Linker::new(&self.engine);

        // Configure WASI with the granted permissions
        let wasi_ctx = self.build_wasi_context()?;

        // Pipe the JSON-RPC request through WASI stdin/stdout
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "run",
            "params": {
                "command": command,
                "args": args.iter().map(value_to_json).collect::<Vec<_>>(),
                "input": input.as_ref().map(value_to_json),
            }
        });

        let request_bytes = serde_json::to_vec(&request).map_err(|e| {
            ShellError::new(
                format!("cannot serialize request: {}", e),
                ErrorCategory::PluginError,
            )
        })?;

        // Set up stdin with the request and capture stdout for the response
        let stdin = ReadPipe::from(request_bytes);
        let stdout = WritePipe::new_in_memory();

        // The WasiCtxBuilder lets us grant specific filesystem access
        let mut wasi_builder = WasiCtxBuilder::new()
            .stdin(Box::new(stdin))
            .stdout(Box::new(stdout.clone()))
            .inherit_stderr();  // let plugin errors appear in terminal

        // Grant filesystem access according to permissions
        for path in &self.permissions.read_paths {
            if let Ok(dir) = wasmtime_wasi::Dir::open_ambient_dir(
                path,
                wasmtime_wasi::ambient_authority(),
            ) {
                let guest_path = path.to_string_lossy().to_string();
                wasi_builder = wasi_builder.preopened_dir(dir, &guest_path)
                    .map_err(|e| ShellError::new(
                        format!("cannot grant path access: {}", e),
                        ErrorCategory::PluginError,
                    ))?;
            }
        }

        // Grant environment variable access
        for var_name in &self.permissions.env_vars {
            if let Ok(value) = std::env::var(var_name) {
                wasi_builder = wasi_builder.env(var_name, &value)
                    .map_err(|e| ShellError::new(
                        format!("cannot set env: {}", e),
                        ErrorCategory::PluginError,
                    ))?;
            }
        }

        let wasi = wasi_builder.build();

        wasmtime_wasi::add_to_linker(&mut linker, |s| s).map_err(|e| {
            ShellError::new(
                format!("cannot link WASI: {}", e),
                ErrorCategory::PluginError,
            )
        })?;

        let mut store = Store::new(&self.engine, wasi);

        // Set resource limits
        store.limiter(|_| Box::new(PluginLimiter {
            memory_limit: 256 * 1024 * 1024,  // 256 MB
        }));

        let instance = linker.instantiate(&mut store, &self.module).map_err(|e| {
            ShellError::new(
                format!("cannot instantiate WASM module: {}", e),
                ErrorCategory::PluginError,
            )
        })?;

        // Call the _start function (WASI entry point)
        let start = instance.get_typed_func::<(), ()>(&mut store, "_start")
            .map_err(|e| ShellError::new(
                format!("WASM module has no _start: {}", e),
                ErrorCategory::PluginError,
            ))?;

        start.call(&mut store, ()).map_err(|e| {
            ShellError::new(
                format!("WASM execution failed: {}", e),
                ErrorCategory::PluginError,
            )
        })?;

        // Read the response from stdout
        let output_bytes = stdout.try_into_inner()
            .map_err(|_| ShellError::new(
                "cannot read WASM output",
                ErrorCategory::PluginError,
            ))?
            .into_inner();

        let response_str = String::from_utf8_lossy(&output_bytes);

        // Parse the JSON-RPC response
        let response: RpcResponse = serde_json::from_str(&response_str)
            .map_err(|e| ShellError::new(
                format!("invalid response from WASM plugin: {}", e),
                ErrorCategory::PluginError,
            ))?;

        match response.result {
            Some(json_val) => json_to_value(&json_val),
            None => {
                if let Some(err) = response.error {
                    Err(ShellError::new(err.message, ErrorCategory::PluginError))
                } else {
                    Ok(Value::Null)
                }
            }
        }
    }

    fn build_wasi_context(&self) -> Result<WasiCtx, ShellError> {
        // Built inline in run_command above; this is a placeholder
        // for more complex initialization logic.
        todo!()
    }
}

/// Resource limiter for WASM plugins.
struct PluginLimiter {
    memory_limit: usize,
}

impl ResourceLimiter for PluginLimiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool> {
        Ok(desired <= self.memory_limit)
    }

    fn table_growing(
        &mut self,
        _current: u32,
        _desired: u32,
        _maximum: Option<u32>,
    ) -> Result<bool> {
        Ok(true)
    }
}
```

### Building a WASM plugin

Any language that compiles to WASM can be a plugin. For Rust:

```bash
# Install the WASM target
rustup target add wasm32-wasi

# Build the plugin as WASM
cd ~/.jsh/plugins/jsh-my-plugin
cargo build --target wasm32-wasi --release

# Copy the .wasm file to the plugin directory
cp target/wasm32-wasi/release/jsh-my-plugin.wasm .
```

---

## Concept 10: Putting It All Together — The Dispatch Pipeline

When a user types a command, the shell needs to check plugins as part of its dispatch logic. Here's the complete dispatch pipeline with plugins integrated:

```
User types "docker-ps --all"
         │
         ▼
┌──────────────────┐
│ 1. Is it a       │ ──YES──▶ Run the alias expansion, re-parse
│    shell alias?  │
└──────────────────┘
         │ NO
         ▼
┌──────────────────┐
│ 2. Is it a       │ ──YES──▶ Run the builtin function directly
│    builtin?      │
│    (cd, pwd...)  │
└──────────────────┘
         │ NO
         ▼
┌──────────────────┐
│ 3. Is it a       │ ──YES──▶ Send JSON-RPC request to plugin,
│    plugin cmd?   │          return structured data
│    (docker-ps)   │
└──────────────────┘
         │ NO
         ▼
┌──────────────────┐
│ 4. Is it in      │ ──YES──▶ Fork/exec the external program
│    $PATH?        │
│    (ls, git...)  │
└──────────────────┘
         │ NO
         ▼
┌──────────────────┐
│ 5. Command not   │ ──▶ Show error with suggestions
│    found         │     (did you mean...? / install plugin?)
└──────────────────┘
```

```rust
impl Interpreter {
    fn execute_command(&mut self, cmd: &CommandNode) -> Result<Value, ShellError> {
        let name = &cmd.program;
        let args = &cmd.args;

        // 1. Check aliases
        if let Some(expanded) = self.state.aliases.get(name) {
            let reparsed = self.parse_and_execute(expanded)?;
            return Ok(reparsed);
        }

        // 2. Check builtins
        if let Some(builtin_fn) = self.builtins.get(name.as_str()) {
            return builtin_fn(&mut self.state, args);
        }

        // 3. Check plugins
        if self.state.plugin_manager.has_command(name) {
            let shell_args: Vec<Value> = args.iter()
                .map(|a| Value::String(a.clone()))
                .collect();
            let input = self.pipeline_input.take();
            return self.state.plugin_manager.run_command(name, &shell_args, input);
        }

        // 4. Check PATH (external command)
        if self.find_in_path(name).is_some() {
            return self.run_external(cmd);
        }

        // 5. Command not found
        let mut error = command_not_found_error(name);

        // Suggest similar commands
        if let Some(suggestion) = self.find_similar_command(name) {
            error = error.with_meta("suggestion", format!("Did you mean `{}`?", suggestion));
        }

        // Suggest a plugin if one might provide this command
        if name.starts_with("docker") || name.starts_with("k8s") || name.starts_with("kubectl") {
            error = error.with_meta(
                "plugin_hint",
                format!("Try: plugin install jsh-{}", name.split('-').next().unwrap_or(name)),
            );
        }

        Err(error)
    }
}
```

---

## Key Rust Concepts Used

| Concept | Where it appears |
|---------|-----------------|
| **Serde serialization** | JSON-RPC messages to/from plugins |
| **Process management** | `std::process::Command` with piped stdin/stdout |
| **BufReader/BufWriter** | Line-delimited JSON protocol over pipes |
| **HashMap registry** | Mapping command names to plugin names |
| **Trait objects** | Plugins and builtins share a common execution interface |
| **Builder pattern** | `WasiCtxBuilder` for configuring WASM sandboxes |
| **Error handling with `map_err`** | Converting various error types to `ShellError` |
| **`cfg!(windows)`** | Cross-platform executable naming (.exe) |
| **Lazy initialization** | Plugins start on first use, not at shell startup |
| **Resource limiting** | `ResourceLimiter` trait for WASM memory caps |

---

## Milestone

After completing this module, your shell should handle the following:

```
jsh> plugin list
╭───┬────────────────┬─────────┬──────────────────────────────────────╮
│ # │      name      │ version │             description              │
├───┼────────────────┼─────────┼──────────────────────────────────────┤
│ 0 │ jsh-docker     │  0.3.1  │ Docker commands with structured data │
╰───┴────────────────┴─────────┴──────────────────────────────────────╯

jsh> docker-ps
╭───┬──────────────┬──────────────┬────────────┬─────────╮
│ # │     name     │    image     │   status   │  state  │
├───┼──────────────┼──────────────┼────────────┼─────────┤
│ 0 │ web-app      │ nginx:latest │ Up 2 hours │ running │
│ 1 │ api-server   │ node:18      │ Up 2 hours │ running │
╰───┴──────────────┴──────────────┴────────────┴─────────╯

jsh> docker-ps | where state == "running" | each { |c| $c.name }
[web-app, api-server]

jsh> plugin new weather
Plugin jsh-weather created at ~/.jsh/plugins/jsh-weather

jsh> plugin install https://github.com/example/jsh-k8s.git
Cloning jsh-k8s...
Building plugin...
Plugin jsh-k8s installed successfully.
Commands available: k8s-pods, k8s-services, k8s-deploy

jsh> plugin remove jsh-k8s
Removed plugin jsh-k8s.

jsh> nonexistent-tool
Error [CommandNotFound]: command not found: nonexistent-tool
  Suggestion: Did you mean `nslookup`?

jsh> docker-logs web-app --tail 5
2026-02-16T10:00:01Z  GET /health 200
2026-02-16T10:00:05Z  GET /api/users 200
2026-02-16T10:00:12Z  POST /api/login 401
2026-02-16T10:00:15Z  GET /api/users 200
2026-02-16T10:00:20Z  GET /health 200
```

---

## What's next?

Congratulations — you have built a shell that is genuinely better than bash. It has intelligent completions, structured error handling, a modern scripting language, and an extensible plugin system. From here, the roadmap is yours to write:

- **Module 21: Configuration & Theming** — a powerful `config.jsh` system with color themes, key bindings, and prompt customization.
- **Module 22: Network-Aware Shell** — built-in HTTP client, SSH multiplexing, and remote command execution.
- **Module 23: Shell as an IDE** — inline documentation, type hover, go-to-definition for functions, and integrated debugging.
- **Module 24: Performance & Profiling** — benchmark your shell, optimize hot paths, and add `time` and `profile` builtins.

The foundation is solid. Build something great.

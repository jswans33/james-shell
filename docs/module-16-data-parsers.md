# Module 16: Built-in Data Format Handling

## What are we building?

Every developer eventually ends up doing the same thing in bash: parsing JSON, querying TOML configs, filtering CSV data, or hitting a REST API. And every time, it requires a patchwork of external tools:

```bash
# Bash: read a JSON API response
curl -s https://api.example.com/users | jq '.[] | select(.active == true) | .name'

# Bash: read a TOML config
# ...there IS no standard tool. Maybe `yq -t`? Maybe a Python one-liner?
cat config.toml | python3 -c "import sys, toml; print(toml.load(sys.stdin)['database']['host'])"

# Bash: filter a CSV
cat data.csv | awk -F',' 'NR>1 && $3 > 100 { print $1","$2 }'

# Bash: read a YAML config
cat deploy.yaml | yq '.services[0].image'
```

Each tool has its own syntax, its own installation requirements, its own quirks. `jq` is powerful but has a non-trivial learning language. `yq` has multiple incompatible versions. `awk` is cryptic for column-based CSV work.

After this module, james-shell will handle all of these natively:

```
jsh> open config.toml | get database.host        # auto-detects TOML
localhost

jsh> open data.csv | where revenue > 100 | select name  # auto-detects CSV
 # | name
---+-----------
 0 | Acme Corp
 1 | Widgets Inc

jsh> fetch https://api.example.com/users | where active == true | get name
["alice", "bob", "charlie"]
```

No external tools. No separate query language. Just the same pipeline syntax you already know from Module 15.

---

## Concept 1: The `open` command — format auto-detection

The `open` command reads a file and automatically parses it based on the file extension:

```
jsh> open Cargo.toml          # → parsed TOML → Record
jsh> open data.csv            # → parsed CSV → Table
jsh> open package.json        # → parsed JSON → Record/Table
jsh> open config.yaml         # → parsed YAML → Record
jsh> open readme.md           # → raw text (no parser for .md)
jsh> open image.png           # → Binary
```

### The implementation

```rust
use std::path::Path;

pub struct OpenCommand;

impl InternalCommand for OpenCommand {
    fn name(&self) -> &str { "open" }
    fn description(&self) -> &str { "Open a file and parse it based on its format" }

    fn run(
        &self,
        _input: PipelineInput,
        args: &[String],
    ) -> Result<PipelineOutput, ShellError> {
        if args.is_empty() {
            return Err(ShellError::InvalidArgs {
                command: "open".into(),
                message: "Expected a file path".into(),
            });
        }

        let file_path = &args[0];
        let path = Path::new(file_path);

        // Check file exists
        if !path.exists() {
            return Err(ShellError::FileNotFound {
                path: file_path.clone(),
            });
        }

        // Determine format from extension
        let extension = path.extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_lowercase());

        match extension.as_deref() {
            Some("json") => parse_json_file(path),
            Some("toml") => parse_toml_file(path),
            Some("csv") => parse_csv_file(path),
            Some("tsv") => parse_tsv_file(path),
            Some("yaml") | Some("yml") => parse_yaml_file(path),
            Some("xml") => parse_xml_file(path),
            Some("txt") | Some("log") | Some("md") | Some("rs") | Some("py")
            | Some("js") | Some("ts") | Some("sh") | Some("toml") => {
                // Text files: read as string
                let content = std::fs::read_to_string(path).map_err(|e| {
                    ShellError::IoError {
                        context: format!("reading {}", file_path),
                        error: e.to_string(),
                    }
                })?;
                Ok(PipelineOutput::Value(Value::String(content)))
            }
            _ => {
                // Unknown extension: try reading as UTF-8 text, fall back to binary
                match std::fs::read_to_string(path) {
                    Ok(content) => Ok(PipelineOutput::Value(Value::String(content))),
                    Err(_) => {
                        let bytes = std::fs::read(path).map_err(|e| {
                            ShellError::IoError {
                                context: format!("reading {}", file_path),
                                error: e.to_string(),
                            }
                        })?;
                        Ok(PipelineOutput::Value(Value::Binary(bytes)))
                    }
                }
            }
        }
    }
}
```

---

## Concept 2: JSON parsing with `serde_json`

JSON is the most common structured data format. We use the `serde_json` crate for parsing and generation.

### Cargo.toml dependencies

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

### Converting between `serde_json::Value` and our `Value`

The `serde_json` crate has its own `Value` type. We need bidirectional conversion:

```rust
use serde_json;

/// Convert a serde_json::Value into our shell's Value
pub fn json_to_value(json: serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Nothing,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                // u64 that doesn't fit in i64 — store as float
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(arr) => {
            let items: Vec<Value> = arr.into_iter().map(json_to_value).collect();
            // Try to promote to table if it's a list of objects
            try_promote_to_table(items)
        }
        serde_json::Value::Object(map) => {
            let record: BTreeMap<String, Value> = map
                .into_iter()
                .map(|(k, v)| (k, json_to_value(v)))
                .collect();
            Value::Record(record)
        }
    }
}

/// Convert our shell's Value into serde_json::Value
pub fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Nothing => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(n) => serde_json::json!(*n),
        Value::Float(f) => serde_json::json!(*f),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::List(items) => {
            serde_json::Value::Array(
                items.iter().map(value_to_json).collect()
            )
        }
        Value::Record(map) => {
            let object: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), value_to_json(v)))
                .collect();
            serde_json::Value::Object(object)
        }
        Value::Table { columns, rows } => {
            // Convert table back to JSON array of objects
            let arr: Vec<serde_json::Value> = rows
                .iter()
                .map(|row| {
                    let object: serde_json::Map<String, serde_json::Value> = columns
                        .iter()
                        .filter_map(|col| {
                            row.get(col).map(|v| (col.clone(), value_to_json(v)))
                        })
                        .collect();
                    serde_json::Value::Object(object)
                })
                .collect();
            serde_json::Value::Array(arr)
        }
        Value::Filesize(n) => serde_json::json!(*n),
        Value::Duration(n) => serde_json::json!(*n),
        Value::Binary(bytes) => {
            // Base64 encode binary data for JSON
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            serde_json::Value::String(encoded)
        }
    }
}
```

### Parsing a JSON file

```rust
fn parse_json_file(path: &Path) -> Result<PipelineOutput, ShellError> {
    let content = std::fs::read_to_string(path).map_err(|e| ShellError::IoError {
        context: format!("reading {}", path.display()),
        error: e.to_string(),
    })?;

    let json: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        ShellError::ParseError {
            format: "JSON".into(),
            error: e.to_string(),
            file: path.display().to_string(),
        }
    })?;

    Ok(PipelineOutput::Value(json_to_value(json)))
}
```

### Example usage

Given a file `users.json`:
```json
[
    {"name": "alice", "age": 30, "active": true},
    {"name": "bob", "age": 25, "active": false},
    {"name": "charlie", "age": 35, "active": true}
]
```

```
jsh> open users.json
 # | name    | age | active
---+---------+-----+--------
 0 | alice   |  30 | true
 1 | bob     |  25 | false
 2 | charlie |  35 | true

jsh> open users.json | where active == true | select name age
 # | name    | age
---+---------+-----
 0 | alice   |  30
 1 | charlie |  35

jsh> open users.json | get 1
{active: false, age: 25, name: bob}

jsh> open users.json | get 1.name
bob
```

---

## Concept 3: TOML parsing with the `toml` crate

TOML is the configuration format of choice for Rust projects (and many others). Every Rust developer interacts with `Cargo.toml` daily.

### Cargo.toml dependency

```toml
[dependencies]
toml = "0.8"
```

### Converting TOML to Value

The `toml` crate also uses its own `toml::Value` type. Our conversion looks very similar to the JSON one:

```rust
use toml;

pub fn toml_to_value(toml_val: toml::Value) -> Value {
    match toml_val {
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Integer(n) => Value::Int(n),
        toml::Value::Float(f) => Value::Float(f),
        toml::Value::String(s) => Value::String(s),
        toml::Value::Datetime(dt) => {
            // TOML has a native datetime type — we store as string for now
            Value::String(dt.to_string())
        }
        toml::Value::Array(arr) => {
            let items: Vec<Value> = arr.into_iter().map(toml_to_value).collect();
            try_promote_to_table(items)
        }
        toml::Value::Table(table) => {
            let record: BTreeMap<String, Value> = table
                .into_iter()
                .map(|(k, v)| (k, toml_to_value(v)))
                .collect();
            Value::Record(record)
        }
    }
}

pub fn value_to_toml(value: &Value) -> Result<toml::Value, ShellError> {
    match value {
        Value::Nothing => Ok(toml::Value::String(String::new())),
        Value::Bool(b) => Ok(toml::Value::Boolean(*b)),
        Value::Int(n) => Ok(toml::Value::Integer(*n)),
        Value::Float(f) => Ok(toml::Value::Float(*f)),
        Value::String(s) => Ok(toml::Value::String(s.clone())),
        Value::List(items) => {
            let arr: Result<Vec<toml::Value>, ShellError> =
                items.iter().map(value_to_toml).collect();
            Ok(toml::Value::Array(arr?))
        }
        Value::Record(map) => {
            let table: Result<toml::map::Map<String, toml::Value>, ShellError> = map
                .iter()
                .map(|(k, v)| value_to_toml(v).map(|tv| (k.clone(), tv)))
                .collect();
            Ok(toml::Value::Table(table?))
        }
        Value::Table { columns, rows } => {
            // Tables become TOML arrays of tables
            let arr: Result<Vec<toml::Value>, ShellError> = rows
                .iter()
                .map(|row| {
                    let table: Result<toml::map::Map<String, toml::Value>, ShellError> =
                        columns
                            .iter()
                            .filter_map(|col| {
                                row.get(col).map(|v| {
                                    value_to_toml(v).map(|tv| (col.clone(), tv))
                                })
                            })
                            .collect();
                    table.map(toml::Value::Table)
                })
                .collect();
            Ok(toml::Value::Array(arr?))
        }
        _ => Err(ShellError::TypeMismatch {
            expected: "TOML-compatible value".into(),
            got: value.type_name().into(),
        }),
    }
}

fn parse_toml_file(path: &Path) -> Result<PipelineOutput, ShellError> {
    let content = std::fs::read_to_string(path).map_err(|e| ShellError::IoError {
        context: format!("reading {}", path.display()),
        error: e.to_string(),
    })?;

    let toml_val: toml::Value = content.parse().map_err(|e: toml::de::Error| {
        ShellError::ParseError {
            format: "TOML".into(),
            error: e.to_string(),
            file: path.display().to_string(),
        }
    })?;

    Ok(PipelineOutput::Value(toml_to_value(toml_val)))
}
```

### Example usage

```
jsh> open Cargo.toml
{dependencies: {ctrlc: "3.4", rustyline: "14.0", ...}, package: {edition: "2021", name: "james-shell", version: "0.1.0"}}

jsh> open Cargo.toml | get package.name
james-shell

jsh> open Cargo.toml | get package.version
0.1.0

jsh> open Cargo.toml | get dependencies
{ctrlc: "3.4", rustyline: "14.0", serde: {features: ["derive"], version: "1"}, serde_json: "1"}
```

---

## Concept 4: CSV parsing with the `csv` crate

CSV is everywhere — exports from databases, spreadsheets, APIs. The `csv` crate handles the messy details (quoted fields, different delimiters, BOM handling).

### Cargo.toml dependency

```toml
[dependencies]
csv = "1"
```

### Parsing CSV into a Table

CSV maps perfectly to our Table type — columns are the header row, rows are the data:

```rust
use csv;

fn parse_csv_file(path: &Path) -> Result<PipelineOutput, ShellError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)       // Allow rows with different column counts
        .trim(csv::Trim::All) // Trim whitespace from fields
        .from_path(path)
        .map_err(|e| ShellError::IoError {
            context: format!("opening CSV {}", path.display()),
            error: e.to_string(),
        })?;

    // Read headers
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| ShellError::ParseError {
            format: "CSV".into(),
            error: e.to_string(),
            file: path.display().to_string(),
        })?
        .iter()
        .map(|h| h.to_string())
        .collect();

    // Read all rows
    let mut rows: Vec<BTreeMap<String, Value>> = Vec::new();

    for result in reader.records() {
        let record = result.map_err(|e| ShellError::ParseError {
            format: "CSV".into(),
            error: e.to_string(),
            file: path.display().to_string(),
        })?;

        let mut row = BTreeMap::new();
        for (i, field) in record.iter().enumerate() {
            let column = headers
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("column{}", i));

            // Try to infer the type of each field
            let value = infer_csv_value(field);
            row.insert(column, value);
        }
        rows.push(row);
    }

    Ok(PipelineOutput::Value(Value::Table {
        columns: headers,
        rows,
    }))
}

/// Infer the Value type from a CSV field string.
/// CSV is all text, but we can guess: numbers, booleans, etc.
fn infer_csv_value(field: &str) -> Value {
    let trimmed = field.trim();

    // Empty field
    if trimmed.is_empty() {
        return Value::Nothing;
    }

    // Boolean
    if trimmed.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }

    // Integer
    if let Ok(n) = trimmed.parse::<i64>() {
        return Value::Int(n);
    }

    // Float (must contain a dot to avoid "42" being parsed as 42.0)
    if trimmed.contains('.') {
        if let Ok(f) = trimmed.parse::<f64>() {
            return Value::Float(f);
        }
    }

    // Fall back to string
    Value::String(trimmed.to_string())
}
```

### Writing CSV output

```rust
pub fn value_to_csv(value: &Value) -> Result<String, ShellError> {
    match value {
        Value::Table { columns, rows } => {
            let mut writer = csv::Writer::from_writer(Vec::new());

            // Write header
            writer.write_record(columns).map_err(|e| ShellError::IoError {
                context: "writing CSV header".into(),
                error: e.to_string(),
            })?;

            // Write rows
            for row in rows {
                let fields: Vec<String> = columns
                    .iter()
                    .map(|col| {
                        row.get(col)
                            .map(|v| v.to_string())
                            .unwrap_or_default()
                    })
                    .collect();

                writer.write_record(&fields).map_err(|e| ShellError::IoError {
                    context: "writing CSV row".into(),
                    error: e.to_string(),
                })?;
            }

            let bytes = writer.into_inner().map_err(|e| ShellError::IoError {
                context: "finalizing CSV".into(),
                error: e.to_string(),
            })?;

            String::from_utf8(bytes).map_err(|e| ShellError::IoError {
                context: "CSV encoding".into(),
                error: e.to_string(),
            })
        }
        Value::List(items) => {
            // A list of values becomes a single-column CSV
            let mut writer = csv::Writer::from_writer(Vec::new());
            writer.write_record(&["value"]).map_err(|e| ShellError::IoError {
                context: "writing CSV header".into(),
                error: e.to_string(),
            })?;
            for item in items {
                writer.write_record(&[item.to_string()]).map_err(|e| {
                    ShellError::IoError {
                        context: "writing CSV row".into(),
                        error: e.to_string(),
                    }
                })?;
            }
            let bytes = writer.into_inner().map_err(|e| ShellError::IoError {
                context: "finalizing CSV".into(),
                error: e.to_string(),
            })?;
            String::from_utf8(bytes).map_err(|e| ShellError::IoError {
                context: "CSV encoding".into(),
                error: e.to_string(),
            })
        }
        _ => Err(ShellError::TypeMismatch {
            expected: "table or list".into(),
            got: value.type_name().into(),
        }),
    }
}
```

### Example usage

Given a file `sales.csv`:
```csv
name,revenue,region,active
Acme Corp,150,West,true
Widgets Inc,200,East,true
Foo LLC,80,West,false
Bar Co,175,East,true
```

```
jsh> open sales.csv
 # | name        | revenue | region | active
---+-------------+---------+--------+--------
 0 | Acme Corp   |     150 | West   | true
 1 | Widgets Inc |     200 | East   | true
 2 | Foo LLC     |      80 | West   | false
 3 | Bar Co      |     175 | East   | true

jsh> open sales.csv | where revenue > 100 | select name revenue
 # | name        | revenue
---+-------------+---------
 0 | Acme Corp   |     150
 1 | Widgets Inc |     200
 2 | Bar Co      |     175

jsh> open sales.csv | where region == "East" | get revenue | math sum
375
```

---

## Concept 5: YAML parsing with `serde_yaml`

YAML is heavily used in DevOps (Kubernetes, Docker Compose, Ansible, GitHub Actions). Having built-in YAML support makes james-shell a powerful tool for infrastructure work.

### Cargo.toml dependency

```toml
[dependencies]
serde_yaml = "0.9"
```

### Converting YAML to Value

```rust
use serde_yaml;

pub fn yaml_to_value(yaml: serde_yaml::Value) -> Value {
    match yaml {
        serde_yaml::Value::Null => Value::Nothing,
        serde_yaml::Value::Bool(b) => Value::Bool(b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Float(0.0)
            }
        }
        serde_yaml::Value::String(s) => Value::String(s),
        serde_yaml::Value::Sequence(seq) => {
            let items: Vec<Value> = seq.into_iter().map(yaml_to_value).collect();
            try_promote_to_table(items)
        }
        serde_yaml::Value::Mapping(map) => {
            let record: BTreeMap<String, Value> = map
                .into_iter()
                .map(|(k, v)| {
                    let key = match k {
                        serde_yaml::Value::String(s) => s,
                        other => format!("{:?}", other), // Non-string keys
                    };
                    (key, yaml_to_value(v))
                })
                .collect();
            Value::Record(record)
        }
        serde_yaml::Value::Tagged(tagged) => {
            // YAML tags (like !!str, !!int) — just parse the inner value
            yaml_to_value(tagged.value)
        }
    }
}

fn parse_yaml_file(path: &Path) -> Result<PipelineOutput, ShellError> {
    let content = std::fs::read_to_string(path).map_err(|e| ShellError::IoError {
        context: format!("reading {}", path.display()),
        error: e.to_string(),
    })?;

    let yaml: serde_yaml::Value = serde_yaml::from_str(&content).map_err(|e| {
        ShellError::ParseError {
            format: "YAML".into(),
            error: e.to_string(),
            file: path.display().to_string(),
        }
    })?;

    Ok(PipelineOutput::Value(yaml_to_value(yaml)))
}
```

### Example usage

Given a file `docker-compose.yml`:
```yaml
version: "3.8"
services:
  web:
    image: nginx:1.25
    ports:
      - "80:80"
      - "443:443"
    environment:
      NODE_ENV: production
  db:
    image: postgres:16
    ports:
      - "5432:5432"
    environment:
      POSTGRES_PASSWORD: secret
```

```
jsh> open docker-compose.yml | get services.web.image
nginx:1.25

jsh> open docker-compose.yml | get services.web.ports
["80:80", "443:443"]

jsh> open docker-compose.yml | get services
{db: {environment: {POSTGRES_PASSWORD: secret}, image: postgres:16, ports: ["5432:5432"]}, web: {environment: {NODE_ENV: production}, image: nginx:1.25, ports: ["80:80", "443:443"]}}
```

---

## Concept 6: Explicit format conversion commands

Sometimes you have data in a pipeline (not in a file) and need to parse or generate a specific format. The `from` and `to` commands handle this:

### The `from` family

```
jsh> '{"name": "alice", "age": 30}' | from json
{age: 30, name: alice}

jsh> "name,age\nalice,30\nbob,25" | from csv
 # | name  | age
---+-------+-----
 0 | alice |  30
 1 | bob   |  25

jsh> '[package]\nname = "my-app"\nversion = "1.0"' | from toml
{package: {name: my-app, version: 1.0}}
```

### The `to` family

```
jsh> { name: "alice", age: 30 } | to json
{"age":30,"name":"alice"}

jsh> { name: "alice", age: 30 } | to json --pretty
{
  "age": 30,
  "name": "alice"
}

jsh> ls | to csv
name,type,size,modified
Cargo.toml,file,342,2025-03-12 08:00:00
Cargo.lock,file,12800,2025-03-15 10:30:00
src,dir,4096,2025-03-15 10:30:00

jsh> { host: "localhost", port: 8080 } | to toml
host = "localhost"
port = 8080
```

### Implementation

```rust
pub struct FromCommand;

impl InternalCommand for FromCommand {
    fn name(&self) -> &str { "from" }
    fn description(&self) -> &str {
        "Parse text input as a specific format (json, csv, toml, yaml)"
    }

    fn run(
        &self,
        input: PipelineInput,
        args: &[String],
    ) -> Result<PipelineOutput, ShellError> {
        let format = args.first().ok_or(ShellError::InvalidArgs {
            command: "from".into(),
            message: "Expected a format: json, csv, toml, yaml".into(),
        })?;

        let text = match input {
            PipelineInput::Value(Value::String(s)) => s,
            PipelineInput::Text(s) => s,
            PipelineInput::Value(v) => v.to_string(),
            PipelineInput::Nothing => {
                return Err(ShellError::MissingInput {
                    command: "from".into(),
                });
            }
        };

        match format.as_str() {
            "json" => {
                let json: serde_json::Value = serde_json::from_str(&text)
                    .map_err(|e| ShellError::ParseError {
                        format: "JSON".into(),
                        error: e.to_string(),
                        file: "<stdin>".into(),
                    })?;
                Ok(PipelineOutput::Value(json_to_value(json)))
            }
            "csv" => {
                parse_csv_from_string(&text)
            }
            "toml" => {
                let toml_val: toml::Value = text.parse()
                    .map_err(|e: toml::de::Error| ShellError::ParseError {
                        format: "TOML".into(),
                        error: e.to_string(),
                        file: "<stdin>".into(),
                    })?;
                Ok(PipelineOutput::Value(toml_to_value(toml_val)))
            }
            "yaml" | "yml" => {
                let yaml: serde_yaml::Value = serde_yaml::from_str(&text)
                    .map_err(|e| ShellError::ParseError {
                        format: "YAML".into(),
                        error: e.to_string(),
                        file: "<stdin>".into(),
                    })?;
                Ok(PipelineOutput::Value(yaml_to_value(yaml)))
            }
            other => Err(ShellError::InvalidArgs {
                command: "from".into(),
                message: format!(
                    "Unknown format '{}'. Supported: json, csv, toml, yaml",
                    other
                ),
            }),
        }
    }
}

pub struct ToCommand;

impl InternalCommand for ToCommand {
    fn name(&self) -> &str { "to" }
    fn description(&self) -> &str {
        "Convert structured data to a specific format (json, csv, toml, yaml)"
    }

    fn run(
        &self,
        input: PipelineInput,
        args: &[String],
    ) -> Result<PipelineOutput, ShellError> {
        let format = args.first().ok_or(ShellError::InvalidArgs {
            command: "to".into(),
            message: "Expected a format: json, csv, toml, yaml".into(),
        })?;

        let pretty = args.iter().any(|a| a == "--pretty" || a == "-p");
        let value = input_to_value(input, "to")?;

        let text = match format.as_str() {
            "json" => {
                let json = value_to_json(&value);
                if pretty {
                    serde_json::to_string_pretty(&json)
                } else {
                    serde_json::to_string(&json)
                }
                .map_err(|e| ShellError::IoError {
                    context: "serializing to JSON".into(),
                    error: e.to_string(),
                })?
            }
            "csv" => value_to_csv(&value)?,
            "toml" => {
                let toml_val = value_to_toml(&value)?;
                toml::to_string(&toml_val).map_err(|e| ShellError::IoError {
                    context: "serializing to TOML".into(),
                    error: e.to_string(),
                })?
            }
            "yaml" | "yml" => {
                let json = value_to_json(&value); // Reuse JSON conversion
                serde_yaml::to_string(&json).map_err(|e| ShellError::IoError {
                    context: "serializing to YAML".into(),
                    error: e.to_string(),
                })?
            }
            other => {
                return Err(ShellError::InvalidArgs {
                    command: "to".into(),
                    message: format!(
                        "Unknown format '{}'. Supported: json, csv, toml, yaml",
                        other
                    ),
                });
            }
        };

        Ok(PipelineOutput::Value(Value::String(text)))
    }
}

/// Parse CSV from a string (not a file)
fn parse_csv_from_string(text: &str) -> Result<PipelineOutput, ShellError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| ShellError::ParseError {
            format: "CSV".into(),
            error: e.to_string(),
            file: "<stdin>".into(),
        })?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let mut rows: Vec<BTreeMap<String, Value>> = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| ShellError::ParseError {
            format: "CSV".into(),
            error: e.to_string(),
            file: "<stdin>".into(),
        })?;

        let mut row = BTreeMap::new();
        for (i, field) in record.iter().enumerate() {
            let column = headers.get(i).cloned().unwrap_or_else(|| format!("column{}", i));
            row.insert(column, infer_csv_value(field));
        }
        rows.push(row);
    }

    Ok(PipelineOutput::Value(Value::Table {
        columns: headers,
        rows,
    }))
}
```

---

## Concept 7: The `fetch` command — HTTP requests

A modern shell should be able to talk to web APIs without shelling out to `curl`. The `fetch` command performs HTTP requests and returns structured data:

```
jsh> fetch https://api.github.com/repos/nushell/nushell
{description: "A new type of shell", full_name: "nushell/nushell", ...}

jsh> fetch https://api.github.com/repos/nushell/nushell | get stargazers_count
35000

jsh> fetch https://jsonplaceholder.typicode.com/users | select name email
 # | name               | email
---+--------------------+---------------------------
 0 | Leanne Graham      | Sincere@april.biz
 1 | Ervin Howell       | Shanna@melissa.tv
 ...
```

### Cargo.toml dependency

We use `ureq` for simplicity — it is synchronous and has no async runtime dependency. For a production shell, you might use `reqwest` with `tokio` for async support.

```toml
[dependencies]
ureq = { version = "2", features = ["json"] }
```

### Implementation

```rust
pub struct FetchCommand;

impl InternalCommand for FetchCommand {
    fn name(&self) -> &str { "fetch" }
    fn description(&self) -> &str {
        "Fetch data from a URL (HTTP GET) and return structured data"
    }

    fn run(
        &self,
        _input: PipelineInput,
        args: &[String],
    ) -> Result<PipelineOutput, ShellError> {
        if args.is_empty() {
            return Err(ShellError::InvalidArgs {
                command: "fetch".into(),
                message: "Expected a URL".into(),
            });
        }

        let url = &args[0];

        // Parse optional flags
        let headers: Vec<(&str, &str)> = parse_header_args(args);
        let timeout_secs: u64 = parse_timeout_arg(args).unwrap_or(30);

        // Build the request
        let mut request = ureq::get(url)
            .timeout(std::time::Duration::from_secs(timeout_secs));

        // Add custom headers
        for (key, value) in &headers {
            request = request.set(key, value);
        }

        // Add a user-agent so APIs don't reject us
        request = request.set("User-Agent", "james-shell/0.1");

        // Execute the request
        let response = request.call().map_err(|e| ShellError::NetworkError {
            url: url.clone(),
            error: e.to_string(),
        })?;

        // Check content type to decide how to parse
        let content_type = response
            .header("Content-Type")
            .unwrap_or("")
            .to_lowercase();

        let body = response.into_string().map_err(|e| ShellError::NetworkError {
            url: url.clone(),
            error: e.to_string(),
        })?;

        // Auto-detect format from content type
        if content_type.contains("application/json") || content_type.contains("+json") {
            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| ShellError::ParseError {
                    format: "JSON".into(),
                    error: e.to_string(),
                    file: url.clone(),
                })?;
            Ok(PipelineOutput::Value(json_to_value(json)))
        } else if content_type.contains("text/csv") {
            parse_csv_from_string(&body)
        } else if content_type.contains("application/yaml")
            || content_type.contains("application/x-yaml")
        {
            let yaml: serde_yaml::Value = serde_yaml::from_str(&body)
                .map_err(|e| ShellError::ParseError {
                    format: "YAML".into(),
                    error: e.to_string(),
                    file: url.clone(),
                })?;
            Ok(PipelineOutput::Value(yaml_to_value(yaml)))
        } else {
            // For everything else (HTML, plain text, etc.), return as string
            // The user can pipe to `from json` etc. if they know the format
            Ok(PipelineOutput::Value(Value::String(body)))
        }
    }
}

fn parse_header_args<'a>(args: &'a [String]) -> Vec<(&'a str, &'a str)> {
    let mut headers = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if (args[i] == "--header" || args[i] == "-H") && i + 1 < args.len() {
            if let Some((key, value)) = args[i + 1].split_once(':') {
                headers.push((key.trim(), value.trim()));
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    headers
}

fn parse_timeout_arg(args: &[String]) -> Option<u64> {
    for (i, arg) in args.iter().enumerate() {
        if arg == "--timeout" || arg == "-t" {
            if let Some(val) = args.get(i + 1) {
                return val.parse().ok();
            }
        }
    }
    None
}
```

### Real-world examples

```
# Fetch GitHub API data
jsh> fetch https://api.github.com/repos/rust-lang/rust/releases | first 3 | select tag_name published_at
 # | tag_name | published_at
---+----------+---------------------
 0 | 1.78.0   | 2025-05-02T00:00:00Z
 1 | 1.77.2   | 2025-04-09T00:00:00Z
 2 | 1.77.1   | 2025-03-28T00:00:00Z

# Fetch with custom headers (e.g., authentication)
jsh> fetch https://api.example.com/private -H "Authorization: Bearer $TOKEN" | get data

# Fetch and convert format
jsh> fetch https://example.com/data.csv | from csv | where status == "active"

# Chain with file operations
jsh> fetch https://jsonplaceholder.typicode.com/todos | where completed == true | to csv | save completed_todos.csv
```

---

## Concept 8: Path-based access into nested structures

One of the most powerful features is drilling into deeply nested data with dot-notation. This was introduced in Module 14's `follow_path`, but here we see it used extensively with parsed data formats:

```
jsh> open config.toml | get database.host
localhost

jsh> open package.json | get scripts.build
tsc && vite build

jsh> open docker-compose.yml | get services.web.ports.0
80:80
```

### Advanced path operations

Beyond simple dot-access, we support several navigation patterns:

```
# Get a specific field
jsh> open config.json | get database.host
localhost

# Get a nested field from each row in a table
jsh> open users.json | get name
["alice", "bob", "charlie"]

# Get a specific row, then a field
jsh> open users.json | get 0.name
alice

# Combine with where for conditional access
jsh> open users.json | where active == true | get name
["alice", "charlie"]
```

### The `get` command — deep dive

Let us look at how `get` handles the different cases when working with parsed data:

```rust
/// Resolve a dot-path on any Value, handling the table-column case specially
pub fn resolve_path(value: &Value, path: &str) -> Result<Value, ShellError> {
    let segments: Vec<&str> = path.split('.').collect();
    resolve_path_segments(value, &segments)
}

fn resolve_path_segments(value: &Value, segments: &[&str]) -> Result<Value, ShellError> {
    if segments.is_empty() {
        return Ok(value.clone());
    }

    let segment = segments[0];
    let rest = &segments[1..];

    match value {
        Value::Record(map) => {
            let inner = map.get(segment).ok_or_else(|| ShellError::ColumnNotFound {
                column: segment.to_string(),
                available: map.keys().cloned().collect(),
            })?;
            resolve_path_segments(inner, rest)
        }

        Value::Table { columns, rows } => {
            // If segment is a column name, extract that column as a list
            if columns.contains(&segment.to_string()) {
                let column_values: Vec<Value> = rows
                    .iter()
                    .map(|row| {
                        row.get(segment).cloned().unwrap_or(Value::Nothing)
                    })
                    .collect();

                if rest.is_empty() {
                    Ok(Value::List(column_values))
                } else {
                    // Apply remaining path to each value in the column
                    let drilled: Result<Vec<Value>, ShellError> = column_values
                        .iter()
                        .map(|v| resolve_path_segments(v, rest))
                        .collect();
                    Ok(Value::List(drilled?))
                }
            }
            // If segment is a number, get that row
            else if let Ok(index) = segment.parse::<usize>() {
                if index >= rows.len() {
                    return Err(ShellError::IndexOutOfBounds {
                        index,
                        length: rows.len(),
                    });
                }
                let row = Value::Record(rows[index].clone());
                resolve_path_segments(&row, rest)
            } else {
                Err(ShellError::ColumnNotFound {
                    column: segment.to_string(),
                    available: columns.clone(),
                })
            }
        }

        Value::List(items) => {
            if let Ok(index) = segment.parse::<usize>() {
                let item = items.get(index).ok_or(ShellError::IndexOutOfBounds {
                    index,
                    length: items.len(),
                })?;
                resolve_path_segments(item, rest)
            } else {
                // Try to apply the field access to each item in the list
                // (useful for lists of records)
                let results: Result<Vec<Value>, ShellError> = items
                    .iter()
                    .map(|item| resolve_path_segments(item, segments))
                    .collect();
                Ok(Value::List(results?))
            }
        }

        _ => {
            Err(ShellError::CannotAccessField {
                field: segment.to_string(),
                type_name: value.type_name().to_string(),
            })
        }
    }
}
```

---

## Concept 9: Saving output to files

The complement to `open` is `save` — writing structured data back to files:

```
jsh> { host: "localhost", port: 8080 } | to json --pretty | save config.json
jsh> open data.csv | where active == true | to csv | save filtered.csv
jsh> ls | to json | save directory-listing.json
```

```rust
pub struct SaveCommand;

impl InternalCommand for SaveCommand {
    fn name(&self) -> &str { "save" }
    fn description(&self) -> &str { "Save pipeline content to a file" }

    fn run(
        &self,
        input: PipelineInput,
        args: &[String],
    ) -> Result<PipelineOutput, ShellError> {
        if args.is_empty() {
            return Err(ShellError::InvalidArgs {
                command: "save".into(),
                message: "Expected a file path".into(),
            });
        }

        let file_path = &args[0];
        let force = args.iter().any(|a| a == "--force" || a == "-f");
        let append = args.iter().any(|a| a == "--append" || a == "-a");

        let path = Path::new(file_path);

        // Safety check: don't overwrite without --force
        if path.exists() && !force && !append {
            return Err(ShellError::FileExists {
                path: file_path.clone(),
            });
        }

        let content = match input {
            PipelineInput::Value(Value::String(s)) => s,
            PipelineInput::Value(Value::Binary(bytes)) => {
                // Write binary directly
                let write_fn = if append {
                    std::fs::OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open(path)
                } else {
                    std::fs::File::create(path).map(|f| f)
                };

                let mut file = write_fn.map_err(|e| ShellError::IoError {
                    context: format!("opening {} for writing", file_path),
                    error: e.to_string(),
                })?;

                use std::io::Write;
                file.write_all(&bytes).map_err(|e| ShellError::IoError {
                    context: format!("writing to {}", file_path),
                    error: e.to_string(),
                })?;

                return Ok(PipelineOutput::Nothing);
            }
            PipelineInput::Value(v) => v.to_string(),
            PipelineInput::Text(s) => s,
            PipelineInput::Nothing => {
                return Err(ShellError::MissingInput {
                    command: "save".into(),
                });
            }
        };

        if append {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(path)
                .map_err(|e| ShellError::IoError {
                    context: format!("opening {} for append", file_path),
                    error: e.to_string(),
                })?;
            file.write_all(content.as_bytes()).map_err(|e| ShellError::IoError {
                context: format!("writing to {}", file_path),
                error: e.to_string(),
            })?;
        } else {
            std::fs::write(path, &content).map_err(|e| ShellError::IoError {
                context: format!("writing to {}", file_path),
                error: e.to_string(),
            })?;
        }

        Ok(PipelineOutput::Nothing)
    }
}
```

---

## Concept 10: Putting it all together — real-world workflows

Here are complete workflows that demonstrate the power of built-in data format handling combined with typed pipelines:

### Workflow 1: Querying a Rust project's dependencies

```
jsh> open Cargo.toml | get dependencies
{csv: "1", serde: {features: ["derive"], version: "1"}, serde_json: "1", toml: "0.8", ureq: {features: ["json"], version: "2"}}

jsh> open Cargo.lock | get package | where name != "james-shell" | select name version | sort-by name
 # | name         | version
---+--------------+---------
 0 | csv          | 1.3.0
 1 | serde        | 1.0.200
 2 | serde_json   | 1.0.117
 3 | toml         | 0.8.14
 4 | ureq         | 2.10.0
 ...

jsh> open Cargo.lock | get package | length
47
```

### Workflow 2: Comparing two JSON files

```
jsh> let old = (open config.old.json)
jsh> let new = (open config.json)
jsh> $old.database.port
5432
jsh> $new.database.port
5433
jsh> $old.database.port == $new.database.port
false
```

### Workflow 3: Converting between formats

```
# Convert a YAML CI config to JSON (for an API that needs JSON)
jsh> open .github/workflows/ci.yml | to json --pretty | save ci.json

# Convert CSV data to a TOML config
jsh> open servers.csv | to toml | save servers.toml

# Pull API data and save as CSV for a spreadsheet
jsh> fetch https://api.example.com/sales | select name revenue date | to csv | save sales.csv
```

### Workflow 4: Infrastructure inspection

```
# Check which Kubernetes pods are not running
jsh> fetch http://localhost:8001/api/v1/pods | get items | where status.phase != "Running" | select metadata.name status.phase

# List all Docker Compose services and their images
jsh> open docker-compose.yml | get services | transpose service config | select service config.image

# Find the largest log entries
jsh> open app.log.json | sort-by timestamp --reverse | first 10 | select level message
```

### The complete format support matrix

| Format | `open` | `from` | `to` | Crate | Notes |
|--------|--------|--------|------|-------|-------|
| JSON | Auto by `.json` | `from json` | `to json` | `serde_json` | Supports `--pretty` flag |
| TOML | Auto by `.toml` | `from toml` | `to toml` | `toml` | Good for Rust configs |
| CSV | Auto by `.csv` | `from csv` | `to csv` | `csv` | Auto-infers column types |
| TSV | Auto by `.tsv` | `from tsv` | `to tsv` | `csv` (with delimiter) | Tab-separated variant |
| YAML | Auto by `.yml`/`.yaml` | `from yaml` | `to yaml` | `serde_yaml` | Good for DevOps configs |
| XML | Auto by `.xml` | `from xml` | `to xml` | `quick-xml` (optional) | Lower priority |
| Text | Fallback for unknown | N/A | N/A | std | Plain string |
| Binary | Fallback for non-UTF8 | N/A | N/A | std | Raw bytes |

---

## Key Rust concepts used

- **The `serde` ecosystem** — Rust's serialization framework is the backbone of all our format handling. `serde_json`, `toml`, `serde_yaml`, and `csv` all use `serde` for (de)serialization. Understanding `Serialize` and `Deserialize` traits is essential.
- **`From` / `Into` trait pattern** — Our `json_to_value`, `toml_to_value`, etc. functions follow the converter pattern. In a more polished implementation, these would be `From<serde_json::Value> for Value` trait implementations.
- **Error mapping with `map_err`** — Converting between crate-specific error types (`serde_json::Error`, `csv::Error`, `ureq::Error`) and our unified `ShellError`. This is a common pattern in Rust applications that use multiple libraries.
- **`BTreeMap` for deterministic output** — When we serialize a Record to JSON or TOML, using `BTreeMap` ensures the keys are always in alphabetical order, making output reproducible.
- **Builder pattern** — The `csv::ReaderBuilder` and `ureq::get().set().timeout().call()` chains demonstrate Rust's builder pattern for configuring complex objects step by step.
- **Content type negotiation** — The `fetch` command examines HTTP `Content-Type` headers to decide how to parse the response, demonstrating real-world protocol handling.
- **Feature flags in Cargo.toml** — `serde = { version = "1", features = ["derive"] }` and `ureq = { version = "2", features = ["json"] }` show how Rust crates expose optional functionality through feature flags, keeping binary size small when features are unused.

---

## Milestone

After implementing Module 16, your shell should handle these interactions:

```
jsh> '{"name": "alice", "age": 30, "active": true}' | from json
{active: true, age: 30, name: alice}

jsh> '{"name": "alice", "age": 30}' | from json | get name
alice

jsh> "name,score\nalice,95\nbob,87\ncharlie,92" | from csv
 # | name    | score
---+---------+-------
 0 | alice   |    95
 1 | bob     |    87
 2 | charlie |    92

jsh> "name,score\nalice,95\nbob,87\ncharlie,92" | from csv | where score > 90 | select name
 # | name
---+---------
 0 | alice
 1 | charlie

jsh> open Cargo.toml | get package
{edition: 2021, name: james-shell, version: 0.1.0}

jsh> open Cargo.toml | get package.name
james-shell

jsh> { host: "localhost", port: 8080 } | to json
{"host":"localhost","port":8080}

jsh> { host: "localhost", port: 8080 } | to json --pretty
{
  "host": "localhost",
  "port": 8080
}

jsh> { host: "localhost", port: 8080 } | to toml
host = "localhost"
port = 8080

jsh> ls | where type == "file" | to csv
name,type,size,modified
Cargo.toml,file,342,2025-03-12 08:00:00
Cargo.lock,file,12800,2025-03-15 10:30:00
main.rs,file,1234,2025-03-15 10:30:00

jsh> ls | to json --pretty | save directory.json
jsh> open directory.json | length
4

jsh> fetch https://jsonplaceholder.typicode.com/users | first 3 | select name email
 # | name           | email
---+----------------+----------------------
 0 | Leanne Graham  | Sincere@april.biz
 1 | Ervin Howell   | Shanna@melissa.tv
 2 | Clementine B.  | Nathan@yesenia.net

jsh> fetch https://jsonplaceholder.typicode.com/todos | where completed == true | length
90

jsh> open data.csv | to json --pretty | save data.json   # CSV → JSON conversion
jsh> open config.yaml | to toml | save config.toml       # YAML → TOML conversion
```

---

## What's next?

With structured types (Module 14), typed pipelines (Module 15), and built-in data format handling (Module 16), james-shell now has a complete "structured data" story. You can read any common format, query it with a uniform syntax, transform it through pipelines, and write it back out in any format.

The next frontier is making this fast and robust at scale: lazy streaming for large files, parallel pipeline execution, and caching for expensive operations like HTTP fetches. Beyond that lies the full programming language layer — user-defined functions that accept and return typed values, custom commands that plug into the pipeline system, and a module/package system for sharing reusable shell commands.

You have built something that genuinely surpasses bash for data-oriented tasks while remaining fully compatible with the traditional Unix tool ecosystem. That is the goal of a modern shell.

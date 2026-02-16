# Module 14: Structured Data Types

## What are we building?

This is where james-shell stops being "another bash clone" and becomes something **better**. Every shell you have used until now — bash, zsh, fish, cmd — treats everything as a string. A filename? String. A number? String. A list of files? A string with newlines between them. The exit code of a process? A string that happens to contain digits.

This works until it doesn't. And it doesn't work *constantly*:

```bash
# Bash: is "10" greater than "9"?
if [[ "10" > "9" ]]; then echo "yes"; else echo "no"; fi
# Output: no
# Because "10" < "9" in lexicographic (string) ordering! "1" < "9"

# Bash: try to add numbers
echo $((3 + 4))   # Works (special syntax)
x="3"; y="4"
echo $x + $y      # Outputs "3 + 4" — it's string concatenation!

# Bash: try to work with a list
files="foo.txt bar.txt baz.txt"
# Is that one string or three items? It depends on context! Word splitting!
```

After this module, james-shell will have **real types**: integers, floats, strings, booleans, lists, records, and tables. Commands will be able to return structured data, and the pipeline system (Module 15) will pass that data through without mangling it into text.

This is the same insight that powers [Nushell](https://www.nushell.sh/) and [PowerShell](https://docs.microsoft.com/en-us/powershell/). We are not inventing something weird — we are adopting the approach that every modern shell eventually converges on.

---

## Concept 1: Why "everything is a string" breaks down

Bash was designed in 1989. At that time, programs communicated via text streams, and the shell was glue between those programs. Treating everything as text was elegant for that world.

But consider what you actually do in a modern workflow:

| Task | What you want | What bash gives you |
|------|---------------|---------------------|
| Sort files by size | Compare integers | Parse `ls -l` output with `awk`, hope the format doesn't change |
| Read a JSON config | Access nested fields | Pipe through `jq`, learn a separate query language |
| Filter a CSV | Column-based filtering | `cut`, `awk`, or `csvtool` — each with different syntax |
| Check if a service is running | A boolean yes/no | Parse the output string of `systemctl status` with grep |
| Loop over items | Iterate a list | Word-splitting a string on IFS (invisible delimiter character!) |

Every one of these tasks requires you to **serialize** structured data into text, pass it through a pipe, and then **deserialize** it on the other side. That serialization boundary is where bugs live:

```bash
# The classic "filename with spaces" bug:
for f in $(ls); do    # Word-splitting breaks filenames with spaces
    echo "$f"
done

# A filename called "my file.txt" becomes TWO iterations: "my" and "file.txt"
```

With real types, this entire class of bugs vanishes. A list is a list. A filename is a string. A number is a number. There is no ambiguity.

---

## Concept 2: The `Value` enum — our type system's foundation

In Rust, we represent "a value that could be any type" using an `enum`. This is the heart of our type system:

```rust
use std::collections::BTreeMap;

/// A structured value in james-shell.
/// Every variable, every pipeline payload, every command return is a Value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Nothing — the result of commands that produce no output
    Nothing,

    /// A boolean: true or false
    Bool(bool),

    /// A 64-bit signed integer
    Int(i64),

    /// A 64-bit floating point number
    Float(f64),

    /// A UTF-8 string
    String(String),

    /// An ordered list of values (can be heterogeneous)
    List(Vec<Value>),

    /// A key-value map with string keys (insertion-ordered via BTreeMap)
    Record(BTreeMap<String, Value>),

    /// A list of records with consistent column names — the "spreadsheet" type
    Table {
        columns: Vec<String>,
        rows: Vec<BTreeMap<String, Value>>,
    },

    /// Raw bytes — for binary data from external commands
    Binary(Vec<u8>),

    /// A file size in bytes (displayed as "1.5 KB", "3.2 MB", etc.)
    Filesize(u64),

    /// A duration in nanoseconds
    Duration(i64),
}
```

### Why an enum and not trait objects?

You might think "use `dyn Any`" or some trait-object approach. Here is why the enum is better:

1. **Pattern matching** — Rust's `match` gives you exhaustive checking. If you add a new variant, the compiler finds every place that needs updating.
2. **No heap allocation for small types** — `Bool`, `Int`, `Float` are stored inline. Only `String`, `List`, and `Record` allocate.
3. **Clone is cheap to reason about** — you know exactly what each variant costs to clone.
4. **Serialization is trivial** — the enum maps directly to JSON, TOML, etc.

The tradeoff is that `Value` is a closed set — users cannot add new types. This is intentional. A shell's type system should be simple and predictable, not extensible like a programming language.

### Size on the stack

Let us think about how big a `Value` is in memory:

```
Value::Nothing    →  just the discriminant tag (8 bytes with alignment)
Value::Bool(b)    →  tag + 1 byte (padded to 8)
Value::Int(n)     →  tag + 8 bytes
Value::Float(f)   →  tag + 8 bytes
Value::String(s)  →  tag + 24 bytes (String = ptr + len + capacity on the stack)
Value::List(v)    →  tag + 24 bytes (Vec = ptr + len + capacity)
Value::Record(m)  →  tag + 24 bytes (BTreeMap's stack representation)
```

The total size of the enum is determined by its largest variant. With the `Table` variant containing two `Vec`s, the enum will be around 56 bytes on 64-bit systems. This is reasonable — small enough to pass by value in many cases, large enough that you might want `Box<Value>` for deeply nested structures.

---

## Concept 3: Display and formatting

Every `Value` needs to know how to display itself. We implement `std::fmt::Display`:

```rust
use std::fmt;

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nothing => write!(f, ""),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(n) => {
                if n.fract() == 0.0 {
                    // Show "3.0" not "3" so users know it's a float
                    write!(f, "{:.1}", n)
                } else {
                    write!(f, "{}", n)
                }
            }
            Value::String(s) => write!(f, "{}", s),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    // Strings in lists get quotes so you can distinguish
                    // ["hello"] from [hello]
                    match item {
                        Value::String(s) => write!(f, "\"{}\"", s)?,
                        other => write!(f, "{}", other)?,
                    }

                }
                write!(f, "]")
            }
            Value::Record(map) => {
                write!(f, "{{")?;
                for (i, (key, val)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", key, val)?;
                }
                write!(f, "}}")
            }
            Value::Table { columns, rows } => {
                // Delegate to the table formatter (see Concept 8)
                format_table(f, columns, rows)
            }
            Value::Binary(bytes) => {
                write!(f, "<{} bytes of binary data>", bytes.len())
            }
            Value::Filesize(bytes) => {
                write!(f, "{}", format_filesize(*bytes))
            }
            Value::Duration(nanos) => {
                write!(f, "{}", format_duration(*nanos))
            }
        }
    }
}

fn format_filesize(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    const TB: u64 = 1024 * GB;

    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn format_duration(nanos: i64) -> String {
    let abs_nanos = nanos.unsigned_abs();
    let sign = if nanos < 0 { "-" } else { "" };

    if abs_nanos >= 1_000_000_000 {
        format!("{}{:.2}s", sign, abs_nanos as f64 / 1_000_000_000.0)
    } else if abs_nanos >= 1_000_000 {
        format!("{}{:.2}ms", sign, abs_nanos as f64 / 1_000_000.0)
    } else if abs_nanos >= 1_000 {
        format!("{}{}us", sign, abs_nanos / 1_000)
    } else {
        format!("{}{}ns", sign, abs_nanos)
    }
}
```

---

## Concept 4: Type coercion rules

In a shell, users should not have to think about types most of the time. When you write `"5" + 3`, the shell should figure out that you mean `5 + 3 = 8`, not throw a type error. But coercion should also be **predictable** — no surprising JavaScript-style weirdness.

Here are our coercion rules:

### Arithmetic operations (`+`, `-`, `*`, `/`, `%`)

| Left | Right | Result | Rule |
|------|-------|--------|------|
| Int | Int | Int | Normal integer math |
| Int | Float | Float | Int promotes to Float |
| Float | Int | Float | Int promotes to Float |
| Float | Float | Float | Normal float math |
| String | Int | Int (or error) | Parse string as number, fail if it's not numeric |
| String | String | Error | Use `++` for string concatenation to avoid ambiguity |
| Int | String | Int (or error) | Parse string as number |

### Comparison operations (`<`, `>`, `<=`, `>=`, `==`, `!=`)

| Left | Right | Behavior |
|------|-------|----------|
| Int | Int | Numeric comparison |
| Int | Float | Promote Int to Float, then compare |
| String | String | Lexicographic comparison |
| Int | String | Try to parse string as Int; if that fails, error |
| Bool | Bool | false < true |
| Any | Any (mismatched) | Type error (we do NOT silently compare apples to oranges) |

### String concatenation (`++`)

We use `++` instead of `+` for string concatenation. This avoids the "is `+` addition or concatenation?" ambiguity that plagues JavaScript and Python:

```
jsh> "hello" ++ " " ++ "world"
hello world
jsh> "count: " ++ 42       # Int auto-converts to String for ++
count: 42
```

### The coercion implementation

```rust
impl Value {
    /// Attempt to coerce this value to an Int.
    pub fn coerce_to_int(&self) -> Result<i64, ShellError> {
        match self {
            Value::Int(n) => Ok(*n),
            Value::Float(f) => Ok(*f as i64),
            Value::Bool(b) => Ok(if *b { 1 } else { 0 }),
            Value::String(s) => s.parse::<i64>().map_err(|_| {
                ShellError::TypeMismatch {
                    expected: "int".into(),
                    got: format!("string \"{}\"", s),
                }
            }),
            other => Err(ShellError::TypeMismatch {
                expected: "int".into(),
                got: other.type_name().into(),
            }),
        }
    }

    /// Attempt to coerce this value to a Float.
    pub fn coerce_to_float(&self) -> Result<f64, ShellError> {
        match self {
            Value::Float(f) => Ok(*f),
            Value::Int(n) => Ok(*n as f64),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Value::String(s) => s.parse::<f64>().map_err(|_| {
                ShellError::TypeMismatch {
                    expected: "float".into(),
                    got: format!("string \"{}\"", s),
                }
            }),
            other => Err(ShellError::TypeMismatch {
                expected: "float".into(),
                got: other.type_name().into(),
            }),
        }
    }

    /// Coerce to a boolean. The "truthiness" rules.
    pub fn coerce_to_bool(&self) -> bool {
        match self {
            Value::Nothing => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::List(v) => !v.is_empty(),
            Value::Record(m) => !m.is_empty(),
            Value::Table { rows, .. } => !rows.is_empty(),
            Value::Binary(b) => !b.is_empty(),
            Value::Filesize(n) => *n != 0,
            Value::Duration(n) => *n != 0,
        }
    }

    /// Returns the name of this value's type, for error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nothing => "nothing",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::String(_) => "string",
            Value::List(_) => "list",
            Value::Record(_) => "record",
            Value::Table { .. } => "table",
            Value::Binary(_) => "binary",
            Value::Filesize(_) => "filesize",
            Value::Duration(_) => "duration",
        }
    }
}
```

---

## Concept 5: Variable declaration and type inference

Users declare variables with `let`. The type is inferred from the right-hand side — no type annotations needed (but they can be added for clarity or documentation):

```
jsh> let x = 42
jsh> let name = "james"
jsh> let pi = 3.14159
jsh> let active = true
jsh> let nothing = null
```

### How the parser knows which type to create

The parser examines the literal and decides:

```rust
fn parse_literal(token: &str) -> Value {
    // Check for null/nothing
    if token == "null" || token == "nothing" {
        return Value::Nothing;
    }

    // Check for booleans
    if token == "true" {
        return Value::Bool(true);
    }
    if token == "false" {
        return Value::Bool(false);
    }

    // Check for integers (including hex, octal, binary)
    if let Some(n) = try_parse_int(token) {
        return Value::Int(n);
    }

    // Check for floats
    if let Ok(f) = token.parse::<f64>() {
        // Only if it contains a dot or 'e' — otherwise "42" would match as float
        if token.contains('.') || token.contains('e') || token.contains('E') {
            return Value::Float(f);
        }
    }

    // Check for filesize literals: "10mb", "1.5gb", etc.
    if let Some(size) = try_parse_filesize(token) {
        return Value::Filesize(size);
    }

    // Check for duration literals: "5s", "100ms", "2min"
    if let Some(dur) = try_parse_duration(token) {
        return Value::Duration(dur);
    }

    // Fall through: it's a string
    Value::String(token.to_string())
}

fn try_parse_int(s: &str) -> Option<i64> {
    // Decimal
    if let Ok(n) = s.parse::<i64>() {
        return Some(n);
    }
    // Hex: 0xFF
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).ok();
    }
    // Octal: 0o77
    if let Some(oct) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        return i64::from_str_radix(oct, 8).ok();
    }
    // Binary: 0b1010
    if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        return i64::from_str_radix(bin, 2).ok();
    }
    None
}

fn try_parse_filesize(s: &str) -> Option<u64> {
    let s_lower = s.to_lowercase();
    let suffixes: &[(&str, u64)] = &[
        ("tb", 1024 * 1024 * 1024 * 1024),
        ("gb", 1024 * 1024 * 1024),
        ("mb", 1024 * 1024),
        ("kb", 1024),
        ("b", 1),
    ];

    for (suffix, multiplier) in suffixes {
        if let Some(num_str) = s_lower.strip_suffix(suffix) {
            if let Ok(n) = num_str.parse::<f64>() {
                return Some((n * *multiplier as f64) as u64);
            }
        }
    }
    None
}
```

### Filesize and duration literals

One of the nicest features we borrow from Nushell is **unit-aware literals**. Instead of writing `1048576` and hoping someone knows that is 1 MB, you write:

```
jsh> let max_upload = 10mb
jsh> let timeout = 30s
jsh> $max_upload
10.0 MB
jsh> $timeout
30.00s
```

Supported units:

| Type | Units | Examples |
|------|-------|---------|
| Filesize | `b`, `kb`, `mb`, `gb`, `tb` | `100b`, `1.5kb`, `512mb`, `2gb` |
| Duration | `ns`, `us`, `ms`, `s`, `min`, `hr`, `day`, `wk` | `100ms`, `5s`, `2min`, `1hr` |

These are **real types** — you can compare them, do arithmetic with them, and they display in human-readable form:

```
jsh> 1gb > 500mb
true
jsh> 1gb + 512mb
1.5 GB
jsh> 5min + 30s
330.00s
```

---

## Concept 6: List, Record, and Table syntax

### Lists

A list is an ordered sequence of values enclosed in square brackets:

```
jsh> let names = ["alice", "bob", "charlie"]
jsh> let mixed = [1, "hello", true, 3.14]
jsh> let nested = [[1, 2], [3, 4], [5, 6]]
```

The Rust representation is simply `Value::List(Vec<Value>)`. Lists can hold any mix of types, though in practice most lists are homogeneous.

Accessing list elements uses dot-notation with indices:

```
jsh> $names.0
alice
jsh> $names.2
charlie
jsh> $names | length
3
```

### Records

A record is a key-value map enclosed in curly braces. Think of it as a single row of data, a JSON object, or a Python dictionary:

```
jsh> let config = { host: "localhost", port: 8080, ssl: false }
jsh> $config.host
localhost
jsh> $config.port
8080
```

The syntax is deliberately close to JSON but with some shell-friendly relaxations:

- Keys don't need quotes (unless they contain spaces or special characters)
- No trailing comma required (but allowed)
- Values can be any `Value` type, including nested records and lists

```rust
/// Parse a record literal: { key1: val1, key2: val2 }
fn parse_record(tokens: &[Token]) -> Result<Value, ParseError> {
    let mut map = BTreeMap::new();

    // tokens should be the contents between { and }
    let mut i = 0;
    while i < tokens.len() {
        // Expect a key (identifier or string)
        let key = match &tokens[i] {
            Token::Ident(s) => s.clone(),
            Token::StringLit(s) => s.clone(),
            other => return Err(ParseError::ExpectedKey(format!("{:?}", other))),
        };
        i += 1;

        // Expect a colon
        match tokens.get(i) {
            Some(Token::Colon) => i += 1,
            _ => return Err(ParseError::ExpectedColon),
        }

        // Parse the value expression
        let (value, consumed) = parse_expression(&tokens[i..])?;
        i += consumed;
        map.insert(key, value);

        // Optional comma
        if matches!(tokens.get(i), Some(Token::Comma)) {
            i += 1;
        }
    }

    Ok(Value::Record(map))
}
```

### Tables

A table is the power type. It is a list of records where every record has the same columns — think of a spreadsheet, a SQL result set, or a CSV file loaded into memory:

```
jsh> let users = [
    { name: "alice", age: 30, admin: true },
    { name: "bob", age: 25, admin: false },
    { name: "charlie", age: 35, admin: true },
]
```

When the shell detects that a list contains records with consistent keys, it **automatically** promotes it to a `Table` for display:

```
jsh> $users
 # | name    | age | admin
---+---------+-----+-------
 0 | alice   |  30 | true
 1 | bob     |  25 | false
 2 | charlie |  35 | true
```

The detection logic:

```rust
/// If a list of values is actually a list of records with consistent columns,
/// promote it to a Table for better display.
fn try_promote_to_table(values: Vec<Value>) -> Value {
    if values.is_empty() {
        return Value::List(values);
    }

    // Check: are ALL values records?
    let records: Vec<&BTreeMap<String, Value>> = values
        .iter()
        .filter_map(|v| match v {
            Value::Record(m) => Some(m),
            _ => None,
        })
        .collect();

    if records.len() != values.len() {
        // Not all records — keep as a plain list
        return Value::List(values);
    }

    // Collect all unique column names (preserving order from first record)
    let columns: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        let mut cols = Vec::new();
        for record in &records {
            for key in record.keys() {
                if seen.insert(key.clone()) {
                    cols.push(key.clone());
                }
            }
        }
        cols
    };

    // Convert to table rows
    let rows: Vec<BTreeMap<String, Value>> = values
        .into_iter()
        .map(|v| match v {
            Value::Record(m) => m,
            _ => unreachable!(), // We already checked
        })
        .collect();

    Value::Table { columns, rows }
}
```

---

## Concept 7: Comparison with other shells

Let us see how our type system compares to the major players:

### Bash — everything is a string

```bash
# Bash
x=42           # x is the STRING "42"
y=$((x + 1))   # Special arithmetic context — parses "42" → 43 → "43"
files=(a b c)  # Arrays exist but are awkward
echo ${files[1]}  # "b" — zero-indexed, uses weird ${} syntax
declare -A map    # Associative arrays exist since bash 4 — rarely used
```

Bash has no booleans, no floats, no records, no tables. Arrays exist but are second-class citizens with bizarre syntax. Everything is "a string, interpreted in context."

### PowerShell — .NET objects

```powershell
# PowerShell
$x = 42                    # System.Int32
$files = Get-ChildItem     # Array of FileInfo objects
$files | Where-Object { $_.Length -gt 1MB }  # Object pipeline
$hash = @{ host = "localhost"; port = 8080 } # Hashtable
```

PowerShell has real types because it is built on .NET. Its objects carry full type information, methods, and properties. The downside: it is heavyweight, verbose, and tightly coupled to the .NET ecosystem.

### Nushell — structured values

```nu
# Nushell
let x = 42               # int
let files = (ls)          # table
$files | where size > 1mb # structured filtering
let config = { host: "localhost", port: 8080 }  # record
```

Nushell is the closest to what we are building. It pioneered the "structured shell" concept in Rust. Our type system is intentionally similar because Nushell got this design right.

### james-shell — our approach

```
jsh> let x = 42                                    # Int
jsh> let files = (ls)                               # Table
jsh> $files | where size > 1mb                      # Structured filtering
jsh> let config = { host: "localhost", port: 8080 } # Record
jsh> "hello" | length                               # 5 (works on strings too)
```

Our type system is simpler than PowerShell's (no class hierarchy, no methods on objects) but richer than Nushell's in some areas (built-in filesize/duration types with unit arithmetic). The goal is "the minimum set of types that eliminates 95% of shell pain."

### Summary table

| Feature | Bash | PowerShell | Nushell | james-shell |
|---------|------|------------|---------|-------------|
| Integers | Via `$(())` | Native | Native | Native |
| Floats | No | Native | Native | Native |
| Booleans | No (use 0/1) | Native | Native | Native |
| Lists | Awkward arrays | Native | Native | Native |
| Records/Maps | `declare -A` | Hashtable | Native | Native |
| Tables | No | Object arrays | Native | Native |
| Filesize type | No | No | Native | Native |
| Duration type | No | TimeSpan | Native | Native |
| Type coercion | Implicit string | .NET casting | Strict | Predictable |
| Null/Nothing | Empty string | `$null` | `null` | `nothing` |

---

## Concept 8: Backwards compatibility with string commands

Here is the critical design question: how does our typed system interact with the existing string-based world of external commands like `grep`, `awk`, `sort`, etc.?

The answer: **automatic conversion at boundaries**.

```
                    Internal world          Boundary          External world
                   (typed Values)             ↕               (text streams)
                                              |
    ls (internal)  → Table ──────────────────→│──→ text ──→ grep "foo" (external)
                                              │
    curl (external) → text ──────────────────→│──→ Value ──→ from json (internal)
                                              |
```

The rules:

1. **Internal command to internal command**: Values pass through the pipeline directly. No serialization. Zero cost.
2. **Internal command to external command**: The `Value` is rendered to text (using `Display`) and written to the external command's stdin.
3. **External command to internal command**: The external command's stdout is captured as a `Value::String`. The internal command can then parse it.
4. **External command to external command**: Traditional byte-stream pipe. We do not interfere.

```rust
/// Represents what a command produces
pub enum CommandOutput {
    /// A structured value from an internal command
    Value(Value),

    /// Raw text output from an external command
    Text(String),

    /// A byte stream (for piping between external commands without buffering)
    Stream(Box<dyn Read + Send>),
}

/// Convert a CommandOutput to a Value (for when an internal command receives input)
impl CommandOutput {
    pub fn into_value(self) -> Value {
        match self {
            CommandOutput::Value(v) => v,
            CommandOutput::Text(s) => Value::String(s),
            CommandOutput::Stream(mut reader) => {
                let mut buf = Vec::new();
                reader.read_to_end(&mut buf).unwrap_or_default();
                match String::from_utf8(buf) {
                    Ok(s) => Value::String(s),
                    Err(e) => Value::Binary(e.into_bytes()),
                }
            }
        }
    }

    /// Convert a Value to text (for when an external command receives input)
    pub fn into_text(self) -> String {
        match self {
            CommandOutput::Value(v) => v.to_string(),
            CommandOutput::Text(s) => s,
            CommandOutput::Stream(mut reader) => {
                let mut s = String::new();
                reader.read_to_string(&mut s).unwrap_or_default();
                s
            }
        }
    }
}
```

This means old habits keep working:

```
jsh> ls | grep ".rs"          # ls (internal, Table) → text → grep (external)
Cargo.toml                    # grep sees plain text, works fine

jsh> ls | where name =~ ".rs" # ls (internal, Table) → Table → where (internal)
 # | name     | type | size    | modified
---+----------+------+---------+---------------------
 0 | main.rs  | file | 1.2 KB  | 2025-03-15 10:30:00
 1 | lib.rs   | file | 854 B   | 2025-03-14 09:15:00
```

Both work. The first falls back to text mode. The second stays structured. Users can migrate at their own pace.

---

## Concept 9: The variable store

Variables live in a scoped store. Each scope (global, function, block) has its own map, and lookups walk up the scope chain:

```rust
use std::collections::HashMap;

pub struct VariableStore {
    /// Stack of scopes. Index 0 is global, last is current.
    scopes: Vec<HashMap<String, Value>>,
}

impl VariableStore {
    pub fn new() -> Self {
        VariableStore {
            scopes: vec![HashMap::new()], // Start with global scope
        }
    }

    /// Enter a new scope (function call, block, etc.)
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Leave the current scope
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Set a variable in the current (innermost) scope
    pub fn set(&mut self, name: &str, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), value);
        }
    }

    /// Get a variable, searching from innermost scope outward
    pub fn get(&self, name: &str) -> Option<&Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(val) = scope.get(name) {
                return Some(val);
            }
        }
        None
    }

    /// Update an existing variable in the scope where it was defined
    pub fn update(&mut self, name: &str, value: Value) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return true;
            }
        }
        false
    }
}
```

Variable access in the shell uses the `$` prefix:

```
jsh> let greeting = "hello"
jsh> echo $greeting
hello
jsh> let config = { host: "localhost", port: 8080 }
jsh> echo $config.host
localhost
jsh> let items = [10, 20, 30]
jsh> echo $items.1
20
```

### Dot-path resolution

The `$config.host` syntax needs a path resolver that can drill into nested structures:

```rust
impl Value {
    /// Access a nested value by dot-separated path.
    /// Examples: "host", "0", "database.host", "users.0.name"
    pub fn follow_path(&self, path: &str) -> Result<&Value, ShellError> {
        let mut current = self;

        for segment in path.split('.') {
            current = match current {
                Value::Record(map) => {
                    map.get(segment).ok_or_else(|| ShellError::ColumnNotFound {
                        column: segment.to_string(),
                        available: map.keys().cloned().collect(),
                    })?
                }
                Value::List(items) => {
                    let index: usize = segment.parse().map_err(|_| {
                        ShellError::InvalidIndex(segment.to_string())
                    })?;
                    items.get(index).ok_or(ShellError::IndexOutOfBounds {
                        index,
                        length: items.len(),
                    })?
                }
                Value::Table { columns, rows } => {
                    // If segment is a number, return that row as a record
                    if let Ok(index) = segment.parse::<usize>() {
                        if index < rows.len() {
                            // We need to return a reference to the row, but it's
                            // stored as BTreeMap — this works since Table rows
                            // are records
                            return Err(ShellError::NotYetImplemented(
                                "table row access by index".into()
                            ));
                        }
                    }
                    // If segment is a column name, return that column as a list
                    if columns.contains(&segment.to_string()) {
                        return Err(ShellError::NotYetImplemented(
                            "table column access by name".into()
                        ));
                    }
                    return Err(ShellError::ColumnNotFound {
                        column: segment.to_string(),
                        available: columns.clone(),
                    });
                }
                other => {
                    return Err(ShellError::CannotAccessField {
                        field: segment.to_string(),
                        type_name: other.type_name().to_string(),
                    });
                }
            };
        }

        Ok(current)
    }
}
```

---

## Concept 10: Error types for the type system

A good type system produces **helpful** error messages. Here is the error enum:

```rust
#[derive(Debug, Clone)]
pub enum ShellError {
    /// Tried to use a value as the wrong type
    TypeMismatch {
        expected: String,
        got: String,
    },

    /// Tried to access a column that doesn't exist
    ColumnNotFound {
        column: String,
        available: Vec<String>,
    },

    /// Tried to access a list index that's out of bounds
    IndexOutOfBounds {
        index: usize,
        length: usize,
    },

    /// Tried to use a non-numeric string as a list index
    InvalidIndex(String),

    /// Tried to access a field on a non-record/non-list type
    CannotAccessField {
        field: String,
        type_name: String,
    },

    /// Division by zero
    DivisionByZero,

    /// Feature not yet implemented
    NotYetImplemented(String),
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellError::TypeMismatch { expected, got } => {
                write!(f, "Type error: expected {}, got {}", expected, got)
            }
            ShellError::ColumnNotFound { column, available } => {
                write!(
                    f,
                    "Column '{}' not found. Available columns: {}",
                    column,
                    available.join(", ")
                )
            }
            ShellError::IndexOutOfBounds { index, length } => {
                write!(
                    f,
                    "Index {} out of bounds (list has {} items, valid range: 0..{})",
                    index, length, length.saturating_sub(1)
                )
            }
            ShellError::InvalidIndex(s) => {
                write!(f, "'{}' is not a valid list index (expected a number)", s)
            }
            ShellError::CannotAccessField { field, type_name } => {
                write!(
                    f,
                    "Cannot access field '{}' on a value of type '{}'",
                    field, type_name
                )
            }
            ShellError::DivisionByZero => write!(f, "Division by zero"),
            ShellError::NotYetImplemented(what) => {
                write!(f, "Not yet implemented: {}", what)
            }
        }
    }
}

impl std::error::Error for ShellError {}
```

Notice the `ColumnNotFound` variant includes the list of **available** columns. This turns a cryptic error into a helpful one:

```
jsh> let config = { host: "localhost", port: 8080 }
jsh> $config.hostname
Error: Column 'hostname' not found. Available columns: host, port
```

---

## Key Rust concepts used

- **Enums with data** — `Value` is the textbook use case for Rust enums. Each variant carries its own data, and `match` ensures you handle every case.
- **`BTreeMap` vs `HashMap`** — We use `BTreeMap` for records so keys are alphabetically ordered and output is deterministic. `HashMap` would be faster but non-deterministic iteration order.
- **`#[derive(Debug, Clone, PartialEq)]`** — Automatically implements debugging, cloning, and equality comparison for `Value`. Note: `PartialEq` on floats uses IEEE 754 semantics (NaN != NaN), which is correct for our purposes.
- **Trait implementation (`Display`)** — Custom formatting for each type variant.
- **The `?` operator** — Used throughout for error propagation in type coercion and path resolution.
- **`strip_prefix` / `strip_suffix`** — Clean string parsing without manual index arithmetic.
- **Scope-based variable resolution** — A `Vec<HashMap>` used as a stack, demonstrating that data structures do not need to be complex to model real concepts.

---

## Milestone

After implementing Module 14, your shell should handle these interactions:

```
jsh> let x = 42
jsh> $x
42
jsh> let pi = 3.14159
jsh> $pi
3.14159
jsh> let name = "james"
jsh> $name
james
jsh> let active = true
jsh> $active
true

jsh> let sizes = [1mb, 500kb, 2gb]
jsh> $sizes
[1.0 MB, 512.0 KB, 2.0 GB]
jsh> $sizes.0
1.0 MB

jsh> let config = { host: "localhost", port: 8080, debug: false }
jsh> $config
{debug: false, host: localhost, port: 8080}
jsh> $config.host
localhost
jsh> $config.port
8080

jsh> let users = [
    { name: "alice", role: "admin" },
    { name: "bob", role: "user" },
]
jsh> $users
 # | name  | role
---+-------+-------
 0 | alice | admin
 1 | bob   | user

jsh> $x + 8
50
jsh> "count: " ++ $x
count: 42
jsh> $x > 40
true
jsh> 1gb > 500mb
true

jsh> $config.hostname
Error: Column 'hostname' not found. Available columns: debug, host, port
jsh> "hello" + 5
Error: Type error: expected int, got string "hello"
```

---

## What's next?

Module 15 will make these types **flow** through pipelines. Instead of converting everything to text between commands, internal commands will pass `Value` objects directly. That is where `ls | where size > 1mb | sort-by modified` becomes real — typed filtering, sorting, and transformation without any text parsing.

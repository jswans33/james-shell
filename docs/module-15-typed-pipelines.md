# Module 15: Typed Pipelines

## What are we building?

In Module 7, we built traditional Unix pipes — byte streams flowing from one process's stdout to the next process's stdin. That works, but it means every command must serialize its output to text and every downstream command must parse that text back into something useful. This is the "universal tax" of traditional shells.

In Module 14, we gave our shell real types. Now we connect them. After this module, *internal* commands will pass `Value` objects directly through the pipeline — no serialization, no parsing, no ambiguity. External commands still use text pipes for full compatibility.

This is the moment james-shell becomes genuinely more capable than bash:

```
# Bash: sort files by size (fragile, format-dependent, breaks on spaces)
ls -la | sort -k5 -n | tail -10

# james-shell: sort files by size (typed, correct, readable)
ls | sort-by size | last 10

# Bash: find JSON config values (requires jq, separate syntax)
cat config.json | jq '.database.port'

# james-shell: first-class data access
open config.json | get database.port

# Bash: filter CSV rows (requires awk, column counting)
cat data.csv | awk -F',' '$3 > 100 { print $1 }'

# james-shell: declarative filtering
open data.csv | where revenue > 100 | select name
```

---

## Concept 1: The pipeline architecture

The core insight is that we have **two kinds** of commands, and the pipeline must handle both:

```
┌──────────────────────────────────────────────────────────────┐
│                     Pipeline Architecture                     │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  Internal commands (builtins + shell commands)                │
│  ┌──────────┐   Value   ┌──────────┐   Value   ┌──────────┐ │
│  │   ls     │──────────→│  where   │──────────→│ sort-by  │ │
│  └──────────┘           └──────────┘           └──────────┘ │
│       Structured data flows directly — no serialization      │
│                                                               │
│  External commands (grep, awk, curl, etc.)                   │
│  ┌──────────┐  bytes    ┌──────────┐  bytes    ┌──────────┐ │
│  │   cat    │──────────→│  grep    │──────────→│  wc      │ │
│  └──────────┘  (pipe)   └──────────┘  (pipe)   └──────────┘ │
│       Traditional Unix byte-stream pipe — unchanged          │
│                                                               │
│  Mixed pipeline (the interesting case)                       │
│  ┌──────────┐  Value→   ┌──────────┐   →bytes  ┌──────────┐ │
│  │   ls     │──text──→  │  grep    │──text──→   │  wc      │ │
│  └──────────┘  (auto)   └──────────┘   (pipe)  └──────────┘ │
│       Automatic conversion at the boundary                   │
│                                                               │
└──────────────────────────────────────────────────────────────┘
```

### The `PipelineElement` trait

Every command in our pipeline implements a common interface:

```rust
use crate::value::Value;

/// The input to a pipeline stage
pub enum PipelineInput {
    /// No input (first command in pipeline, or a command with no stdin)
    Nothing,

    /// Structured data from an internal command
    Value(Value),

    /// Raw text from an external command
    Text(String),
}

/// The output of a pipeline stage
pub enum PipelineOutput {
    /// Structured data (from internal commands)
    Value(Value),

    /// Raw text (from external commands)
    Text(String),

    /// Nothing (command produced no output, or was a side-effect command)
    Nothing,
}

/// Trait for internal (builtin) commands that operate on structured data
pub trait InternalCommand {
    /// The name of this command (for dispatch and help)
    fn name(&self) -> &str;

    /// Execute the command with the given input and arguments
    fn run(
        &self,
        input: PipelineInput,
        args: &[String],
    ) -> Result<PipelineOutput, ShellError>;

    /// Short description for help text
    fn description(&self) -> &str;
}
```

### The pipeline executor

The executor walks the pipeline stages, threading output to input:

```rust
pub struct Pipeline {
    stages: Vec<PipelineStage>,
}

pub enum PipelineStage {
    Internal {
        command: Box<dyn InternalCommand>,
        args: Vec<String>,
    },
    External {
        program: String,
        args: Vec<String>,
    },
}

impl Pipeline {
    pub fn execute(&self) -> Result<PipelineOutput, ShellError> {
        let mut current_output = PipelineOutput::Nothing;

        for (i, stage) in self.stages.iter().enumerate() {
            let input = if i == 0 {
                PipelineInput::Nothing
            } else {
                output_to_input(current_output)
            };

            current_output = match stage {
                PipelineStage::Internal { command, args } => {
                    command.run(input, args)?
                }
                PipelineStage::External { program, args } => {
                    run_external_in_pipeline(program, args, input)?
                }
            };
        }

        Ok(current_output)
    }
}

/// Convert the output of one stage into the input for the next
fn output_to_input(output: PipelineOutput) -> PipelineInput {
    match output {
        PipelineOutput::Value(v) => PipelineInput::Value(v),
        PipelineOutput::Text(s) => PipelineInput::Text(s),
        PipelineOutput::Nothing => PipelineInput::Nothing,
    }
}

/// Run an external command, feeding it text input and capturing text output
fn run_external_in_pipeline(
    program: &str,
    args: &[String],
    input: PipelineInput,
) -> Result<PipelineOutput, ShellError> {
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(program);
    cmd.args(args);

    // If there's input, pipe it to stdin
    let has_input = !matches!(input, PipelineInput::Nothing);
    if has_input {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit()); // Errors go to terminal

    let mut child = cmd.spawn().map_err(|e| ShellError::CommandNotFound {
        command: program.to_string(),
        error: e.to_string(),
    })?;

    // Write input to child's stdin
    if has_input {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let text = match input {
                PipelineInput::Value(v) => v.to_string(),
                PipelineInput::Text(s) => s,
                PipelineInput::Nothing => String::new(),
            };
            // Ignoring write errors — child may close stdin early (e.g., `head`)
            let _ = stdin.write_all(text.as_bytes());
            drop(stdin); // Close stdin so child knows we're done
        }
    }

    let output = child.wait_with_output().map_err(|e| ShellError::IoError {
        context: format!("waiting for {}", program),
        error: e.to_string(),
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    if stdout.is_empty() {
        Ok(PipelineOutput::Nothing)
    } else {
        Ok(PipelineOutput::Text(stdout))
    }
}
```

---

## Concept 2: The `where` filter

`where` is the most important pipeline command. It filters a table (or list) based on a condition:

```
jsh> ls | where size > 1mb
 # | name          | type | size    | modified
---+---------------+------+---------+---------------------
 0 | database.db   | file | 15.3 MB | 2025-03-10 14:20:00
 1 | archive.tar   | file | 2.1 MB  | 2025-03-08 09:15:00

jsh> [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] | where { |x| $x > 5 }
[6, 7, 8, 9, 10]

jsh> ls | where name =~ "\.rs$"
 # | name     | type | size   | modified
---+----------+------+--------+---------------------
 0 | main.rs  | file | 1.2 KB | 2025-03-15 10:30:00
 1 | lib.rs   | file | 854 B  | 2025-03-14 09:15:00
```

### The `where` implementation

`where` supports two syntaxes:

1. **Column condition**: `where <column> <operator> <value>` — shorthand for table filtering
2. **Block condition**: `where { |row| <expression> }` — general predicate

```rust
pub struct WhereCommand;

impl InternalCommand for WhereCommand {
    fn name(&self) -> &str { "where" }
    fn description(&self) -> &str { "Filter rows based on a condition" }

    fn run(
        &self,
        input: PipelineInput,
        args: &[String],
    ) -> Result<PipelineOutput, ShellError> {
        let value = match input {
            PipelineInput::Value(v) => v,
            PipelineInput::Text(s) => {
                // Convert text to a list of lines for filtering
                Value::List(
                    s.lines()
                        .map(|line| Value::String(line.to_string()))
                        .collect()
                )
            }
            PipelineInput::Nothing => {
                return Err(ShellError::MissingInput {
                    command: "where".into(),
                });
            }
        };

        match value {
            Value::Table { columns, rows } => {
                let condition = parse_where_condition(args)?;
                let filtered_rows: Vec<_> = rows
                    .into_iter()
                    .filter(|row| evaluate_condition(&condition, row))
                    .collect();

                Ok(PipelineOutput::Value(Value::Table {
                    columns,
                    rows: filtered_rows,
                }))
            }
            Value::List(items) => {
                let condition = parse_where_condition(args)?;
                let filtered: Vec<_> = items
                    .into_iter()
                    .filter(|item| evaluate_condition_on_value(&condition, item))
                    .collect();

                Ok(PipelineOutput::Value(Value::List(filtered)))
            }
            other => Err(ShellError::TypeMismatch {
                expected: "table or list".into(),
                got: other.type_name().into(),
            }),
        }
    }
}

/// A parsed filter condition
#[derive(Debug)]
pub enum WhereCondition {
    /// column op value: e.g., size > 1mb
    ColumnCompare {
        column: String,
        operator: CompareOp,
        value: Value,
    },
    /// Regex match: column =~ pattern
    RegexMatch {
        column: String,
        pattern: String,
    },
    /// Negated regex: column !~ pattern
    RegexNotMatch {
        column: String,
        pattern: String,
    },
}

#[derive(Debug)]
pub enum CompareOp {
    Equal,        // ==
    NotEqual,     // !=
    GreaterThan,  // >
    LessThan,     // <
    GreaterEq,    // >=
    LessEq,       // <=
}

fn parse_where_condition(args: &[String]) -> Result<WhereCondition, ShellError> {
    if args.len() != 3 {
        return Err(ShellError::InvalidArgs {
            command: "where".into(),
            message: "Expected: where <column> <operator> <value>".into(),
        });
    }

    let column = args[0].clone();
    let op_str = &args[1];
    let value = parse_literal(&args[2]);

    if op_str == "=~" {
        return Ok(WhereCondition::RegexMatch {
            column,
            pattern: args[2].clone(),
        });
    }
    if op_str == "!~" {
        return Ok(WhereCondition::RegexNotMatch {
            column,
            pattern: args[2].clone(),
        });
    }

    let operator = match op_str.as_str() {
        "==" | "=" => CompareOp::Equal,
        "!=" => CompareOp::NotEqual,
        ">" => CompareOp::GreaterThan,
        "<" => CompareOp::LessThan,
        ">=" => CompareOp::GreaterEq,
        "<=" => CompareOp::LessEq,
        _ => return Err(ShellError::InvalidArgs {
            command: "where".into(),
            message: format!("Unknown operator: '{}'", op_str),
        }),
    };

    Ok(WhereCondition::ColumnCompare { column, operator, value })
}
```

### Comparison logic

The comparison function handles cross-type comparisons using the coercion rules from Module 14:

```rust
fn compare_values(left: &Value, op: &CompareOp, right: &Value) -> bool {
    // Try numeric comparison first
    if let (Ok(l), Ok(r)) = (left.coerce_to_float(), right.coerce_to_float()) {
        return match op {
            CompareOp::Equal => (l - r).abs() < f64::EPSILON,
            CompareOp::NotEqual => (l - r).abs() >= f64::EPSILON,
            CompareOp::GreaterThan => l > r,
            CompareOp::LessThan => l < r,
            CompareOp::GreaterEq => l >= r,
            CompareOp::LessEq => l <= r,
        };
    }

    // Filesize comparison
    if let (Value::Filesize(l), Value::Filesize(r)) = (left, right) {
        return match op {
            CompareOp::Equal => l == r,
            CompareOp::NotEqual => l != r,
            CompareOp::GreaterThan => l > r,
            CompareOp::LessThan => l < r,
            CompareOp::GreaterEq => l >= r,
            CompareOp::LessEq => l <= r,
        };
    }

    // Fall back to string comparison
    let l = left.to_string();
    let r = right.to_string();
    match op {
        CompareOp::Equal => l == r,
        CompareOp::NotEqual => l != r,
        CompareOp::GreaterThan => l > r,
        CompareOp::LessThan => l < r,
        CompareOp::GreaterEq => l >= r,
        CompareOp::LessEq => l <= r,
    }
}
```

---

## Concept 3: The `select` command

`select` picks specific columns from a table, discarding the rest. It is the pipeline equivalent of SQL's `SELECT`:

```
jsh> ls | select name size
 # | name          | size
---+---------------+---------
 0 | main.rs       | 1.2 KB
 1 | lib.rs        | 854 B
 2 | Cargo.toml    | 342 B
 3 | Cargo.lock    | 12.5 KB

jsh> ps | select pid name cpu | where cpu > 5.0
 # | pid   | name    | cpu
---+-------+---------+------
 0 | 1234  | firefox | 12.3
 1 | 5678  | rust    | 8.7
```

```rust
pub struct SelectCommand;

impl InternalCommand for SelectCommand {
    fn name(&self) -> &str { "select" }
    fn description(&self) -> &str { "Select specific columns from a table" }

    fn run(
        &self,
        input: PipelineInput,
        args: &[String],
    ) -> Result<PipelineOutput, ShellError> {
        if args.is_empty() {
            return Err(ShellError::InvalidArgs {
                command: "select".into(),
                message: "Expected at least one column name".into(),
            });
        }

        let value = input_to_value(input, "select")?;
        let selected_columns: Vec<String> = args.to_vec();

        match value {
            Value::Table { columns, rows } => {
                // Validate that all requested columns exist
                for col in &selected_columns {
                    if !columns.contains(col) {
                        return Err(ShellError::ColumnNotFound {
                            column: col.clone(),
                            available: columns.clone(),
                        });
                    }
                }

                // Build new rows with only the selected columns
                let new_rows: Vec<BTreeMap<String, Value>> = rows
                    .into_iter()
                    .map(|mut row| {
                        let mut new_row = BTreeMap::new();
                        for col in &selected_columns {
                            if let Some(val) = row.remove(col) {
                                new_row.insert(col.clone(), val);
                            }
                        }
                        new_row
                    })
                    .collect();

                Ok(PipelineOutput::Value(Value::Table {
                    columns: selected_columns,
                    rows: new_rows,
                }))
            }
            Value::Record(map) => {
                // Select specific fields from a record
                let mut new_map = BTreeMap::new();
                for col in &selected_columns {
                    match map.get(col) {
                        Some(val) => {
                            new_map.insert(col.clone(), val.clone());
                        }
                        None => {
                            return Err(ShellError::ColumnNotFound {
                                column: col.clone(),
                                available: map.keys().cloned().collect(),
                            });
                        }
                    }
                }
                Ok(PipelineOutput::Value(Value::Record(new_map)))
            }
            other => Err(ShellError::TypeMismatch {
                expected: "table or record".into(),
                got: other.type_name().into(),
            }),
        }
    }
}
```

---

## Concept 4: The `sort-by` command

`sort-by` sorts a table by one or more columns:

```
jsh> ls | sort-by size
 # | name          | type | size    | modified
---+---------------+------+---------+---------------------
 0 | Cargo.toml    | file | 342 B   | 2025-03-12 08:00:00
 1 | lib.rs        | file | 854 B   | 2025-03-14 09:15:00
 2 | main.rs       | file | 1.2 KB  | 2025-03-15 10:30:00
 3 | Cargo.lock    | file | 12.5 KB | 2025-03-15 10:30:00

jsh> ls | sort-by size --reverse
 # | name          | type | size    | modified
---+---------------+------+---------+---------------------
 0 | Cargo.lock    | file | 12.5 KB | 2025-03-15 10:30:00
 1 | main.rs       | file | 1.2 KB  | 2025-03-15 10:30:00
 2 | lib.rs        | file | 854 B   | 2025-03-14 09:15:00
 3 | Cargo.toml    | file | 342 B   | 2025-03-12 08:00:00
```

```rust
pub struct SortByCommand;

impl InternalCommand for SortByCommand {
    fn name(&self) -> &str { "sort-by" }
    fn description(&self) -> &str { "Sort a table by one or more columns" }

    fn run(
        &self,
        input: PipelineInput,
        args: &[String],
    ) -> Result<PipelineOutput, ShellError> {
        // Parse arguments: column names and optional --reverse flag
        let reverse = args.iter().any(|a| a == "--reverse" || a == "-r");
        let sort_columns: Vec<&String> = args
            .iter()
            .filter(|a| !a.starts_with('-'))
            .collect();

        if sort_columns.is_empty() {
            return Err(ShellError::InvalidArgs {
                command: "sort-by".into(),
                message: "Expected at least one column name".into(),
            });
        }

        let value = input_to_value(input, "sort-by")?;

        match value {
            Value::Table { columns, mut rows } => {
                rows.sort_by(|a, b| {
                    for col in &sort_columns {
                        let val_a = a.get(col.as_str());
                        let val_b = b.get(col.as_str());

                        let ordering = compare_for_sort(val_a, val_b);
                        if ordering != std::cmp::Ordering::Equal {
                            return if reverse { ordering.reverse() } else { ordering };
                        }
                    }
                    std::cmp::Ordering::Equal
                });

                Ok(PipelineOutput::Value(Value::Table { columns, rows }))
            }
            Value::List(mut items) => {
                items.sort_by(|a, b| {
                    let ordering = compare_for_sort(Some(a), Some(b));
                    if reverse { ordering.reverse() } else { ordering }
                });
                Ok(PipelineOutput::Value(Value::List(items)))
            }
            other => Err(ShellError::TypeMismatch {
                expected: "table or list".into(),
                got: other.type_name().into(),
            }),
        }
    }
}

/// Compare two optional Values for sorting purposes.
/// Nothing/missing values sort to the end.
fn compare_for_sort(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,   // None sorts last
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(a), Some(b)) => compare_values_for_sort(a, b),
    }
}

fn compare_values_for_sort(a: &Value, b: &Value) -> std::cmp::Ordering {
    // Try numeric comparison
    if let (Ok(fa), Ok(fb)) = (a.coerce_to_float(), b.coerce_to_float()) {
        return fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal);
    }

    // Filesize comparison
    if let (Value::Filesize(sa), Value::Filesize(sb)) = (a, b) {
        return sa.cmp(sb);
    }

    // Duration comparison
    if let (Value::Duration(da), Value::Duration(db)) = (a, b) {
        return da.cmp(db);
    }

    // Fall back to string comparison
    a.to_string().cmp(&b.to_string())
}
```

---

## Concept 5: The `each` map command

`each` applies a transformation to every element in a list or every row in a table. It is the pipeline equivalent of `.map()` in functional programming:

```
jsh> [1, 2, 3] | each { |x| $x * 2 }
[2, 4, 6]

jsh> ["hello", "world"] | each { |s| $s | str upcase }
["HELLO", "WORLD"]

jsh> ls | each { |row| $row.name }
["main.rs", "lib.rs", "Cargo.toml", "Cargo.lock"]
```

### Closures in the shell

The `{ |x| ... }` syntax defines a **closure** — a block of shell code with named parameters. This needs its own representation:

```rust
/// A closure: a block of code with parameters that can be applied to values
#[derive(Debug, Clone)]
pub struct Closure {
    /// The parameter names (e.g., ["x"] or ["acc", "x"])
    pub params: Vec<String>,

    /// The body of the closure — a sequence of pipeline stages to execute
    /// This is stored as a parsed AST, not raw text
    pub body: Box<Expression>,
}

/// An expression in our shell's mini-language
#[derive(Debug, Clone)]
pub enum Expression {
    /// A literal value
    Literal(Value),

    /// A variable reference: $x, $row.name
    Variable {
        name: String,
        path: Vec<String>,  // dot-path segments after the variable name
    },

    /// A binary operation: $x + 1, $a * $b
    BinaryOp {
        left: Box<Expression>,
        op: Operator,
        right: Box<Expression>,
    },

    /// A pipeline within an expression
    Pipeline(Vec<PipelineStage>),

    /// A command invocation
    Command {
        name: String,
        args: Vec<Expression>,
    },
}

#[derive(Debug, Clone)]
pub enum Operator {
    Add, Sub, Mul, Div, Mod,
    Eq, Neq, Gt, Lt, Gte, Lte,
    And, Or,
    Concat,  // ++ string concatenation
}
```

### Executing `each`

```rust
pub struct EachCommand;

impl InternalCommand for EachCommand {
    fn name(&self) -> &str { "each" }
    fn description(&self) -> &str {
        "Apply a closure to each element in a list or row in a table"
    }

    fn run(
        &self,
        input: PipelineInput,
        args: &[String],
    ) -> Result<PipelineOutput, ShellError> {
        // Parse the closure from args
        let closure = parse_closure(args)?;
        let value = input_to_value(input, "each")?;

        match value {
            Value::List(items) => {
                let results: Result<Vec<Value>, ShellError> = items
                    .into_iter()
                    .map(|item| apply_closure(&closure, &[item]))
                    .collect();
                Ok(PipelineOutput::Value(Value::List(results?)))
            }
            Value::Table { columns, rows } => {
                // Apply closure to each row (as a record)
                let results: Result<Vec<Value>, ShellError> = rows
                    .into_iter()
                    .map(|row| {
                        let record = Value::Record(row);
                        apply_closure(&closure, &[record])
                    })
                    .collect();

                // If all results are records with the same keys, promote to table
                let result_list = results?;
                Ok(PipelineOutput::Value(try_promote_to_table(result_list)))
            }
            // For a single value, just apply the closure once
            single => {
                let result = apply_closure(&closure, &[single])?;
                Ok(PipelineOutput::Value(result))
            }
        }
    }
}

/// Apply a closure to a set of argument values
fn apply_closure(closure: &Closure, args: &[Value]) -> Result<Value, ShellError> {
    // Create a new scope for the closure
    let mut scope = HashMap::new();

    // Bind parameters to argument values
    for (param, value) in closure.params.iter().zip(args.iter()) {
        scope.insert(param.clone(), value.clone());
    }

    // Evaluate the closure body in the new scope
    evaluate_expression(&closure.body, &scope)
}
```

---

## Concept 6: The `reduce` fold command

`reduce` collapses a list into a single value using an accumulator. It is the pipeline equivalent of `.fold()` / `.reduce()`:

```
jsh> [1, 2, 3, 4, 5] | reduce { |acc, x| $acc + $x }
15

jsh> [1, 2, 3, 4, 5] | reduce --initial 0 { |acc, x| $acc + $x }
15

jsh> ["hello", "world", "foo"] | reduce { |acc, x| $acc ++ ", " ++ $x }
hello, world, foo

jsh> ls | get size | reduce { |acc, x| $acc + $x }
14.9 KB
```

```rust
pub struct ReduceCommand;

impl InternalCommand for ReduceCommand {
    fn name(&self) -> &str { "reduce" }
    fn description(&self) -> &str {
        "Reduce a list to a single value using an accumulator"
    }

    fn run(
        &self,
        input: PipelineInput,
        args: &[String],
    ) -> Result<PipelineOutput, ShellError> {
        // Parse optional --initial flag and the closure
        let (initial_value, closure) = parse_reduce_args(args)?;
        let value = input_to_value(input, "reduce")?;

        let items = match value {
            Value::List(items) => items,
            other => {
                return Err(ShellError::TypeMismatch {
                    expected: "list".into(),
                    got: other.type_name().into(),
                });
            }
        };

        if items.is_empty() {
            return match initial_value {
                Some(init) => Ok(PipelineOutput::Value(init)),
                None => Err(ShellError::InvalidArgs {
                    command: "reduce".into(),
                    message: "Cannot reduce an empty list without --initial".into(),
                }),
            };
        }

        let (accumulator, rest) = match initial_value {
            Some(init) => (init, items.as_slice()),
            None => (items[0].clone(), &items[1..]),
        };

        let result = rest.iter().try_fold(accumulator, |acc, item| {
            apply_closure(&closure, &[acc, item.clone()])
        })?;

        Ok(PipelineOutput::Value(result))
    }
}
```

---

## Concept 7: Additional pipeline commands

Beyond the core four (`where`, `select`, `sort-by`, `each`), a complete pipeline system needs several more commands. Here is the full roster:

### Data extraction

| Command | Description | Example |
|---------|-------------|---------|
| `get <path>` | Extract a field by dot-path | `open config.toml \| get database.host` |
| `select <cols>` | Pick columns from a table | `ls \| select name size` |
| `reject <cols>` | Remove columns from a table | `ls \| reject modified` |
| `first [n]` | Take the first n items (default 1) | `ls \| first 5` |
| `last [n]` | Take the last n items (default 1) | `ls \| last 3` |
| `skip [n]` | Skip the first n items | `ls \| skip 10` |
| `nth <n>` | Get item at index n | `ls \| nth 0` |

### Transformation

| Command | Description | Example |
|---------|-------------|---------|
| `each { }` | Map over items | `[1, 2, 3] \| each { \|x\| $x * 2 }` |
| `reduce { }` | Fold into single value | `[1, 2, 3] \| reduce { \|a, x\| $a + $x }` |
| `flatten` | Flatten nested lists | `[[1, 2], [3, 4]] \| flatten` → `[1, 2, 3, 4]` |
| `uniq` | Remove consecutive duplicates | `[1, 1, 2, 2, 3] \| uniq` → `[1, 2, 3]` |
| `compact` | Remove null/nothing values | `[1, null, 3] \| compact` → `[1, 3]` |

### Aggregation

| Command | Description | Example |
|---------|-------------|---------|
| `length` | Count items | `ls \| length` → `12` |
| `math sum` | Sum numeric values | `[1, 2, 3] \| math sum` → `6` |
| `math avg` | Average of values | `[10, 20, 30] \| math avg` → `20.0` |
| `math min` | Minimum value | `ls \| get size \| math min` |
| `math max` | Maximum value | `ls \| get size \| math max` |

### Implementing `get`

The `get` command is simple but essential — it drills into nested structures using dot-path notation:

```rust
pub struct GetCommand;

impl InternalCommand for GetCommand {
    fn name(&self) -> &str { "get" }
    fn description(&self) -> &str { "Extract a value by column name or path" }

    fn run(
        &self,
        input: PipelineInput,
        args: &[String],
    ) -> Result<PipelineOutput, ShellError> {
        if args.is_empty() {
            return Err(ShellError::InvalidArgs {
                command: "get".into(),
                message: "Expected a column name or path".into(),
            });
        }

        let path = &args[0];
        let value = input_to_value(input, "get")?;

        // For tables, "get <column>" returns a list of that column's values
        if let Value::Table { columns, rows } = &value {
            let first_segment = path.split('.').next().unwrap_or(path);
            if columns.contains(&first_segment.to_string()) {
                let column_values: Vec<Value> = rows
                    .iter()
                    .map(|row| {
                        row.get(first_segment)
                            .cloned()
                            .unwrap_or(Value::Nothing)
                    })
                    .collect();

                // If there are remaining path segments, drill into each value
                let remaining = path.strip_prefix(first_segment)
                    .and_then(|s| s.strip_prefix('.'));

                if let Some(remaining_path) = remaining {
                    let drilled: Result<Vec<Value>, ShellError> = column_values
                        .iter()
                        .map(|v| v.follow_path(remaining_path).cloned())
                        .collect();
                    return Ok(PipelineOutput::Value(Value::List(drilled?)));
                }

                return Ok(PipelineOutput::Value(Value::List(column_values)));
            }
        }

        // For records and other values, use follow_path
        let result = value.follow_path(path)?;
        Ok(PipelineOutput::Value(result.clone()))
    }
}
```

### Implementing `length`

```rust
pub struct LengthCommand;

impl InternalCommand for LengthCommand {
    fn name(&self) -> &str { "length" }
    fn description(&self) -> &str { "Count the number of items" }

    fn run(
        &self,
        input: PipelineInput,
        args: &[String],
    ) -> Result<PipelineOutput, ShellError> {
        let _ = args; // length takes no arguments
        let value = input_to_value(input, "length")?;

        let count = match &value {
            Value::List(items) => items.len(),
            Value::Table { rows, .. } => rows.len(),
            Value::String(s) => s.len(), // length of string in bytes
            Value::Record(m) => m.len(), // number of fields
            _ => {
                return Err(ShellError::TypeMismatch {
                    expected: "list, table, string, or record".into(),
                    got: value.type_name().into(),
                });
            }
        };

        Ok(PipelineOutput::Value(Value::Int(count as i64)))
    }
}
```

---

## Concept 8: Pretty-printing tables

When a pipeline ends with a `Table` value, we render it with aligned columns and borders. This is one of the most visible differences from traditional shells — instead of a wall of text, users see structured, readable output:

```
 # | name          | type | size    | modified
---+---------------+------+---------+---------------------
 0 | Cargo.toml    | file | 342 B   | 2025-03-12 08:00:00
 1 | Cargo.lock    | file | 12.5 KB | 2025-03-15 10:30:00
 2 | src           | dir  | 4.0 KB  | 2025-03-15 10:30:00
 3 | main.rs       | file | 1.2 KB  | 2025-03-15 10:30:00
```

### The table formatter

```rust
use std::fmt;
use std::collections::BTreeMap;

pub fn format_table(
    f: &mut fmt::Formatter<'_>,
    columns: &[String],
    rows: &[BTreeMap<String, Value>],
) -> fmt::Result {
    if rows.is_empty() {
        return write!(f, "(empty table)");
    }

    // Step 1: Calculate column widths
    // Each column is at least as wide as its header
    let row_num_width = rows.len().to_string().len().max(1);
    let mut col_widths: Vec<usize> = columns
        .iter()
        .map(|c| c.len())
        .collect();

    // Check each cell's display width
    for row in rows {
        for (i, col) in columns.iter().enumerate() {
            let cell_text = row
                .get(col)
                .map(|v| v.to_string())
                .unwrap_or_default();
            col_widths[i] = col_widths[i].max(cell_text.len());
        }
    }

    // Step 2: Print header row
    write!(f, " {:>width$} |", "#", width = row_num_width)?;
    for (i, col) in columns.iter().enumerate() {
        write!(f, " {:<width$} ", col, width = col_widths[i])?;
        if i < columns.len() - 1 {
            write!(f, "|")?;
        }
    }
    writeln!(f)?;

    // Step 3: Print separator
    write!(f, "-{}-+", "-".repeat(row_num_width))?;
    for (i, width) in col_widths.iter().enumerate() {
        write!(f, "-{}-", "-".repeat(*width))?;
        if i < col_widths.len() - 1 {
            write!(f, "+")?;
        }
    }
    writeln!(f)?;

    // Step 4: Print data rows
    for (row_idx, row) in rows.iter().enumerate() {
        write!(f, " {:>width$} |", row_idx, width = row_num_width)?;
        for (i, col) in columns.iter().enumerate() {
            let cell = row
                .get(col)
                .map(|v| v.to_string())
                .unwrap_or_default();

            // Right-align numeric values, left-align everything else
            let is_numeric = row.get(col).map_or(false, |v| {
                matches!(v, Value::Int(_) | Value::Float(_) | Value::Filesize(_))
            });

            if is_numeric {
                write!(f, " {:>width$} ", cell, width = col_widths[i])?;
            } else {
                write!(f, " {:<width$} ", cell, width = col_widths[i])?;
            }

            if i < columns.len() - 1 {
                write!(f, "|")?;
            }
        }
        if row_idx < rows.len() - 1 {
            writeln!(f)?;
        }
    }

    Ok(())
}
```

### Handling wide tables and terminal width

When a table is too wide for the terminal, we need to handle it gracefully:

```rust
/// Get terminal width, falling back to 80 columns
fn terminal_width() -> usize {
    // Use the `terminal_size` crate or a simpler approach
    #[cfg(unix)]
    {
        unsafe {
            let mut winsize: libc::winsize = std::mem::zeroed();
            if libc::ioctl(1, libc::TIOCGWINSZ, &mut winsize) == 0 {
                return winsize.ws_col as usize;
            }
        }
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::*;
        unsafe {
            let handle = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
            if GetConsoleScreenBufferInfo(handle, &mut info) != 0 {
                return (info.srWindow.Right - info.srWindow.Left + 1) as usize;
            }
        }
    }

    80 // Fallback
}

/// Truncate a table to fit the terminal width
fn format_table_truncated(
    columns: &[String],
    rows: &[BTreeMap<String, Value>],
    max_width: usize,
) -> String {
    // If the full table fits, render it normally
    // If not, truncate the widest columns or show "..." for overflow
    // This is a simplified version — a real implementation would be
    // more sophisticated about choosing which columns to truncate

    let mut output = String::new();
    // ... (truncation logic)
    output
}
```

---

## Concept 9: Converting between structured and text

At pipeline boundaries between internal and external commands, we need automatic conversion. Here are the rules and the code that implements them:

### Value to text (for external commands)

When an internal command's output feeds into an external command, we convert:

```
Value::Table → rendered as aligned text (what the user would see)
Value::List  → one item per line
Value::String → the string as-is
Value::Int/Float/Bool → to_string()
Value::Record → "key: value" lines
Value::Nothing → empty string
```

```rust
/// Convert a Value to text suitable for piping to an external command
pub fn value_to_pipe_text(value: &Value) -> String {
    match value {
        Value::Nothing => String::new(),
        Value::String(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),

        Value::List(items) => {
            // One item per line
            items.iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        }

        Value::Record(map) => {
            map.iter()
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect::<Vec<_>>()
                .join("\n")
        }

        Value::Table { columns, rows } => {
            // Tab-separated values (easy for external tools to parse)
            let mut lines = Vec::new();

            // Header
            lines.push(columns.join("\t"));

            // Data rows
            for row in rows {
                let cells: Vec<String> = columns.iter()
                    .map(|col| {
                        row.get(col)
                            .map(|v| v.to_string())
                            .unwrap_or_default()
                    })
                    .collect();
                lines.push(cells.join("\t"));
            }

            lines.join("\n")
        }

        Value::Binary(bytes) => {
            // For binary data going to an external command, we write raw bytes
            // This function returns a String, so we use lossy conversion
            // The actual pipe implementation should write bytes directly
            String::from_utf8_lossy(bytes).to_string()
        }

        Value::Filesize(n) => format_filesize(*n),
        Value::Duration(n) => format_duration(*n),
    }
}
```

### Text to value (from external commands)

When an external command's output feeds into an internal command, we attempt smart parsing:

```rust
/// Convert raw text from an external command into a Value.
/// The simplest approach: it stays a string. Internal commands that
/// need structure can parse it themselves (e.g., `from json`).
pub fn text_to_value(text: String) -> Value {
    // Trim trailing newline (nearly all commands add one)
    let trimmed = text.trim_end_matches('\n');

    // If it looks like it could be a table (tab-separated columns), parse it
    if looks_like_tsv(trimmed) {
        if let Some(table) = try_parse_tsv(trimmed) {
            return table;
        }
    }

    // Otherwise, keep as a plain string
    Value::String(trimmed.to_string())
}

fn looks_like_tsv(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 2 {
        return false;
    }
    // Check if all lines have the same number of tab characters
    let tab_count = lines[0].matches('\t').count();
    tab_count > 0 && lines.iter().all(|line| line.matches('\t').count() == tab_count)
}
```

---

## Concept 10: Performance — zero-copy where possible, streaming for large data

A structured pipeline should not be *slower* than text pipes. Here are the key performance strategies:

### 1. Avoid cloning Values when possible

When a pipeline stage only reads data (like `where` filtering), we can borrow instead of cloning:

```rust
// BAD: clone everything, then filter
fn where_clone(table: Value::Table, condition: &Condition) -> Value::Table {
    // This clones EVERY row, then drops the ones that don't match
}

// GOOD: move ownership, discard non-matching rows
fn where_move(table: Value::Table, condition: &Condition) -> Value::Table {
    // rows.into_iter() takes ownership — no cloning
    let filtered = rows.into_iter().filter(|row| check(row)).collect();
    // Non-matching rows are dropped without ever being cloned
}
```

We use `into_iter()` (which takes ownership and moves elements) instead of `iter()` + `clone()` throughout the pipeline. Since each pipeline stage fully consumes its input before the next stage runs (in the simple case), this is always safe.

### 2. Streaming for large data

For very large datasets, we should not hold the entire result in memory. Instead, we can use an iterator-based approach:

```rust
/// A lazy pipeline value that produces rows on demand
pub enum LazyValue {
    /// All data is already in memory
    Eager(Value),

    /// Data is produced lazily by an iterator
    Stream(Box<dyn Iterator<Item = Value> + Send>),
}

/// A streaming `where` that filters without buffering
pub fn where_streaming(
    input: Box<dyn Iterator<Item = Value> + Send>,
    condition: WhereCondition,
) -> Box<dyn Iterator<Item = Value> + Send> {
    Box::new(input.filter(move |item| {
        evaluate_condition_on_value(&condition, item)
    }))
}
```

However, for the initial implementation, eager evaluation (collecting everything into a `Vec`) is simpler and correct. Streaming can be added later as an optimization for specific use cases like reading very large files.

### 3. Internal commands skip serialization entirely

The biggest performance win is simply that internal-to-internal pipelines never serialize to text:

```
Traditional shell pipeline:
  ls → serialize to text → pipe(bytes) → parse text → grep → serialize → pipe → parse → wc

james-shell internal pipeline:
  ls → Value::Table → where (filter in memory) → Value::Table → length (count rows) → Value::Int

No serialization. No parsing. No pipes. No process spawning.
```

For a pipeline like `ls | where size > 1mb | sort-by name | first 10`, the entire operation happens in a single process, with data structures passed between functions. This is orders of magnitude faster than spawning four processes and converting data to and from text three times.

---

## Key Rust concepts used

- **Trait objects (`Box<dyn InternalCommand>`)** — Dynamic dispatch for the command registry. Each command implements the same trait but has different behavior.
- **`into_iter()` vs `iter()`** — Ownership transfer for zero-copy pipeline processing. `into_iter()` consumes the collection, giving us owned values without cloning.
- **`Iterator::filter`, `map`, `try_fold`** — Functional combinators that model our pipeline operations naturally. The Rust standard library's iterator is essentially a pipeline system.
- **Closures and `Fn` traits** — User-defined closures (`{ |x| $x * 2 }`) map to Rust's closure concepts.
- **The `?` operator** — Used throughout for error propagation in pipeline stages.
- **Enum dispatch** — `PipelineInput`, `PipelineOutput`, and `PipelineStage` use enums to handle the different cases cleanly.
- **`partial_cmp` and `Ordering`** — Sorting with custom comparators using Rust's comparison traits.
- **Cross-platform terminal handling** — `cfg(unix)` and `cfg(windows)` blocks for terminal width detection.

---

## Milestone

After implementing Module 15, your shell should handle these interactions:

```
jsh> [3, 1, 4, 1, 5, 9] | sort-by
[1, 1, 3, 4, 5, 9]

jsh> [3, 1, 4, 1, 5, 9] | where { |x| $x > 3 }
[4, 5, 9]

jsh> [1, 2, 3, 4, 5] | each { |x| $x * $x }
[1, 4, 9, 16, 25]

jsh> [1, 2, 3, 4, 5] | reduce { |acc, x| $acc + $x }
15

jsh> ls
 # | name          | type | size    | modified
---+---------------+------+---------+---------------------
 0 | Cargo.toml    | file | 342 B   | 2025-03-12 08:00:00
 1 | Cargo.lock    | file | 12.5 KB | 2025-03-15 10:30:00
 2 | src           | dir  | 4.0 KB  | 2025-03-15 10:30:00
 3 | main.rs       | file | 1.2 KB  | 2025-03-15 10:30:00

jsh> ls | where size > 1kb
 # | name       | type | size    | modified
---+------------+------+---------+---------------------
 0 | Cargo.lock | file | 12.5 KB | 2025-03-15 10:30:00
 1 | src        | dir  | 4.0 KB  | 2025-03-15 10:30:00
 2 | main.rs    | file | 1.2 KB  | 2025-03-15 10:30:00

jsh> ls | sort-by size --reverse | select name size
 # | name       | size
---+------------+---------
 0 | Cargo.lock | 12.5 KB
 1 | src        | 4.0 KB
 2 | main.rs    | 1.2 KB
 3 | Cargo.toml | 342 B

jsh> ls | where type == "file" | get name
["Cargo.toml", "Cargo.lock", "main.rs"]

jsh> ls | length
4

jsh> ls | get size | math sum
18.0 KB

jsh> ls | where name =~ "\.rs$" | select name size
 # | name    | size
---+---------+--------
 0 | main.rs | 1.2 KB

jsh> echo "hello world" | grep "world"      # external pipe still works
hello world
```

---

## What's next?

Module 16 will add built-in data format handling — `open config.json` will auto-detect JSON and parse it into a structured `Value`, `from csv` will parse CSV into a table, and `to json` will serialize our structured data back to JSON. Combined with typed pipelines, this means you can query any data format without leaving the shell or learning a separate tool.

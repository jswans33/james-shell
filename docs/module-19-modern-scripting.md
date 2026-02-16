# Module 19: Modern Scripting Language

## What are we building?

Bash scripting syntax was designed in the 1970s and it shows. Variable assignment breaks if you add a space (`x = 1` fails, `x=1` works). String comparison uses `-eq` for numbers and `=` for strings — but only inside `[ ]`, where `>` means redirection, so you need `\>` or `[[ ]]`. The `if` statement requires `then` and `fi`. Functions can't have named parameters. There are no real data types.

Nobody writes bash because they *enjoy* the syntax. They write it because the shell is already there.

In this module, we give james-shell a **modern scripting language** — one that's actually pleasant to write, with features you'd expect from Python or Ruby, but designed specifically for the command-line:

- **Named function parameters** with types and defaults
- **Type annotations** (optional, for clarity and validation)
- **String interpolation** — no more `"Hello, ${name}"` vs `'Hello, ${name}'` confusion
- **Closures and lambdas** — first-class functions for pipeline transforms
- **Pattern matching** — like Rust's `match`, but in your shell
- **Range expressions** — `1..10` just works
- **Module/import system** — organize scripts into reusable libraries
- **Functional pipelines** — `ls | where size > 1mb | sort-by modified`

This is the module where james-shell stops being "bash but in Rust" and becomes its own language.

---

## Concept 1: Named Function Parameters

In bash, function parameters are positional: `$1`, `$2`, `$3`. You have no idea what they mean without reading the function body. Worse, there's no way to specify defaults or validate types.

```bash
# Bash — what are $1, $2, $3?
deploy() {
    local env=$1
    local version=${2:-latest}
    local dry_run=${3:-false}
    # ...
}
deploy "staging" "v1.2.3" "true"
```

In james-shell, functions have **named parameters** with optional types and defaults:

```
def deploy(env: string, version: string = "latest", dry_run: bool = false) {
    if $dry_run {
        echo "DRY RUN: would deploy $version to $env"
        return
    }
    echo "Deploying $version to $env..."
}

# Call with positional args
deploy "staging" "v1.2.3"

# Or with named args (in any order)
deploy --env "staging" --dry-run true --version "v1.2.3"

# Default values work
deploy "production"  # version defaults to "latest", dry_run to false
```

### AST and type representation

```rust
/// The types that james-shell understands.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnnotation {
    String,
    Int,
    Float,
    Bool,
    List(Box<TypeAnnotation>),        // list<string>
    Record(Vec<(String, TypeAnnotation)>),  // {name: string, age: int}
    Any,                               // no type constraint
    Optional(Box<TypeAnnotation>),     // string? — can be null
}

/// A function parameter with name, optional type, and optional default.
#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub type_ann: Option<TypeAnnotation>,
    pub default: Option<Expression>,
    /// Whether this can be passed as --name from the caller.
    pub named: bool,
}

/// A function definition.
#[derive(Debug, Clone)]
pub struct FunctionDef {
    pub name: String,
    pub params: Vec<Parameter>,
    pub return_type: Option<TypeAnnotation>,
    pub body: Block,
    pub doc_comment: Option<String>,
}
```

### Binding arguments to parameters

```rust
impl Interpreter {
    fn bind_parameters(
        &mut self,
        params: &[Parameter],
        args: &[Argument],
    ) -> Result<(), ShellError> {
        // Separate positional and named arguments from the call site
        let mut positional: Vec<&Value> = Vec::new();
        let mut named: HashMap<String, &Value> = HashMap::new();

        for arg in args {
            match arg {
                Argument::Positional(val) => positional.push(val),
                Argument::Named(name, val) => {
                    named.insert(name.clone(), val);
                }
            }
        }

        let mut pos_idx = 0;

        for param in params {
            // Try named argument first, then positional, then default
            let value = if let Some(val) = named.remove(&param.name) {
                val.clone()
            } else if pos_idx < positional.len() {
                let val = positional[pos_idx].clone();
                pos_idx += 1;
                val
            } else if let Some(ref default_expr) = param.default {
                self.evaluate(default_expr)?
            } else {
                return Err(ShellError::new(
                    format!("missing required parameter: {}", param.name),
                    ErrorCategory::TypeError,
                ));
            };

            // Type check if annotation is present
            if let Some(ref expected_type) = param.type_ann {
                self.check_type(&value, expected_type, &param.name)?;
            }

            self.state.variables.insert(param.name.clone(), value);
        }

        // Check for unused named arguments
        if !named.is_empty() {
            let unknown: Vec<&String> = named.keys().collect();
            return Err(ShellError::new(
                format!("unknown parameters: {:?}", unknown),
                ErrorCategory::TypeError,
            ));
        }

        Ok(())
    }

    fn check_type(
        &self,
        value: &Value,
        expected: &TypeAnnotation,
        param_name: &str,
    ) -> Result<(), ShellError> {
        let matches = match (value, expected) {
            (_, TypeAnnotation::Any) => true,
            (Value::String(_), TypeAnnotation::String) => true,
            (Value::Int(_), TypeAnnotation::Int) => true,
            (Value::Float(_), TypeAnnotation::Float) => true,
            (Value::Bool(_), TypeAnnotation::Bool) => true,
            (Value::List(_), TypeAnnotation::List(_)) => true,
            (Value::Record(_), TypeAnnotation::Record(_)) => true,
            (Value::Null, TypeAnnotation::Optional(_)) => true,
            (val, TypeAnnotation::Optional(inner)) => {
                self.check_type(val, inner, param_name).is_ok()
            }
            _ => false,
        };

        if matches {
            Ok(())
        } else {
            Err(ShellError::new(
                format!(
                    "type mismatch for parameter `{}`: expected {:?}, got {:?}",
                    param_name, expected, value
                ),
                ErrorCategory::TypeError,
            ))
        }
    }
}
```

---

## Concept 2: Type Annotations

Type annotations in james-shell are **optional**. You never *have* to write them. But when you do, they serve as documentation and runtime validation.

### Variable declarations

```
# Without types — works fine
let name = "James"
let count = 42
let items = [1, 2, 3]

# With types — adds clarity and validation
let name: string = "James"
let count: int = 42
let items: list<int> = [1, 2, 3]
let config: record = {host: "localhost", port: 8080}

# Type errors are caught at assignment time
let x: int = "hello"
# Error [TypeError]: type mismatch: expected int, got string

# The `?` suffix means nullable
let maybe_name: string? = null
```

### The type system

james-shell's types map directly to the `Value` enum:

| Type annotation | Value variant | Examples |
|----------------|---------------|----------|
| `string` | `Value::String` | `"hello"`, `'world'` |
| `int` | `Value::Int` | `42`, `-7`, `0xff` |
| `float` | `Value::Float` | `3.14`, `-0.5` |
| `bool` | `Value::Bool` | `true`, `false` |
| `list<T>` | `Value::List` | `[1, 2, 3]`, `["a", "b"]` |
| `record` | `Value::Record` | `{name: "Jim", age: 30}` |
| `any` | Any variant | anything |
| `T?` | `Value::Null` or `T` | `null` or a value of type T |

### Automatic coercion

james-shell performs sensible automatic coercions where there's no ambiguity:

```rust
impl Value {
    /// Attempt to coerce this value to the given type.
    pub fn coerce_to(&self, target: &TypeAnnotation) -> Result<Value, ShellError> {
        match (self, target) {
            // String to int
            (Value::String(s), TypeAnnotation::Int) => {
                s.parse::<i64>()
                    .map(Value::Int)
                    .map_err(|_| ShellError::new(
                        format!("cannot convert '{}' to int", s),
                        ErrorCategory::TypeError,
                    ))
            }

            // String to float
            (Value::String(s), TypeAnnotation::Float) => {
                s.parse::<f64>()
                    .map(Value::Float)
                    .map_err(|_| ShellError::new(
                        format!("cannot convert '{}' to float", s),
                        ErrorCategory::TypeError,
                    ))
            }

            // Int to float (always safe)
            (Value::Int(n), TypeAnnotation::Float) => {
                Ok(Value::Float(*n as f64))
            }

            // Int to string
            (Value::Int(n), TypeAnnotation::String) => {
                Ok(Value::String(n.to_string()))
            }

            // Bool to string
            (Value::Bool(b), TypeAnnotation::String) => {
                Ok(Value::String(b.to_string()))
            }

            // Already the right type
            _ if self.matches_type(target) => Ok(self.clone()),

            _ => Err(ShellError::new(
                format!("cannot coerce {:?} to {:?}", self, target),
                ErrorCategory::TypeError,
            )),
        }
    }
}
```

---

## Concept 3: String Interpolation

Bash string interpolation is confusing because single quotes `'...'` and double quotes `"..."` behave completely differently, and `$var` inside double quotes expands but inside single quotes doesn't. This trips up everyone.

james-shell uses a distinct syntax for interpolated strings, so there's never ambiguity:

```
# Plain strings — no interpolation, ever (like bash single quotes)
let greeting = "Hello, world!"    # literal text
let greeting = 'Hello, world!'    # also literal text

# Interpolated strings — use $"..." with parentheses for expressions
let name = "James"
let msg = $"Hello, ($name)!"              # "Hello, James!"
let msg = $"2 + 2 = (2 + 2)"             # "2 + 2 = 4"
let msg = $"Home is ($env.HOME)"          # "Home is /home/james"
let msg = $"Files: (ls | length)"         # "Files: 42"

# Multi-line strings
let script = $"
    echo ($greeting)
    echo ($msg)
"

# Raw strings — no escapes processed at all
let regex = r"^\d{3}-\d{4}$"
let path = r"C:\Users\James\Documents"
```

### Why `$"..."` instead of `"..."`?

Because it's explicit. In bash, you can never tell at a glance whether `"$foo"` is intentional interpolation or a quoting mistake. In james-shell:

- `"$foo"` is the literal string `$foo` — no interpolation.
- `$"($foo)"` is interpolation — clearly and intentionally.

This prevents the entire class of "I forgot to quote my variable" bugs.

### Parser implementation

```rust
/// Parse an interpolated string: $"text (expr) more text (expr) ..."
fn parse_interpolated_string(&mut self) -> Result<Expression, ShellError> {
    self.expect_token(Token::DollarQuote)?; // consume $"

    let mut parts: Vec<StringPart> = Vec::new();
    let mut literal_buf = String::new();

    loop {
        match self.peek_char() {
            Some('"') => {
                // End of string
                self.advance();
                if !literal_buf.is_empty() {
                    parts.push(StringPart::Literal(literal_buf.clone()));
                }
                break;
            }

            Some('(') => {
                // Start of interpolated expression
                if !literal_buf.is_empty() {
                    parts.push(StringPart::Literal(literal_buf.clone()));
                    literal_buf.clear();
                }
                self.advance(); // consume '('
                let expr = self.parse_expression()?;
                self.expect_char(')')?;
                parts.push(StringPart::Expression(expr));
            }

            Some('\\') => {
                // Escape sequence
                self.advance();
                match self.peek_char() {
                    Some('n') => { self.advance(); literal_buf.push('\n'); }
                    Some('t') => { self.advance(); literal_buf.push('\t'); }
                    Some('\\') => { self.advance(); literal_buf.push('\\'); }
                    Some('"') => { self.advance(); literal_buf.push('"'); }
                    Some('(') => { self.advance(); literal_buf.push('('); }
                    Some(c) => {
                        self.advance();
                        literal_buf.push('\\');
                        literal_buf.push(c);
                    }
                    None => {
                        return Err(ShellError::new(
                            "unexpected end of string",
                            ErrorCategory::SyntaxError,
                        ));
                    }
                }
            }

            Some(c) => {
                self.advance();
                literal_buf.push(c);
            }

            None => {
                return Err(ShellError::new(
                    "unterminated interpolated string",
                    ErrorCategory::SyntaxError,
                ));
            }
        }
    }

    Ok(Expression::InterpolatedString(parts))
}

#[derive(Debug, Clone)]
enum StringPart {
    Literal(String),
    Expression(Expression),
}
```

### Evaluation

```rust
impl Interpreter {
    fn evaluate_interpolated_string(
        &mut self,
        parts: &[StringPart],
    ) -> Result<Value, ShellError> {
        let mut result = String::new();

        for part in parts {
            match part {
                StringPart::Literal(s) => result.push_str(s),
                StringPart::Expression(expr) => {
                    let value = self.evaluate(expr)?;
                    result.push_str(&value.to_display_string());
                }
            }
        }

        Ok(Value::String(result))
    }
}

impl Value {
    /// Convert a value to its display string for interpolation.
    fn to_display_string(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Int(n) => n.to_string(),
            Value::Float(f) => format!("{}", f),
            Value::Bool(b) => b.to_string(),
            Value::Null => "".to_string(),
            Value::List(items) => {
                let parts: Vec<String> = items.iter()
                    .map(|v| v.to_display_string())
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Record(map) => {
                let parts: Vec<String> = map.iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_display_string()))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            Value::Error(e) => format!("<error: {}>", e.message),
        }
    }
}
```

---

## Concept 4: Closures and Lambdas

Closures are anonymous functions that capture variables from their surrounding scope. They're essential for functional-style pipelines and transformations.

### Syntax

```
# Lambda with one parameter
let double = { |x| $x * 2 }
echo (double 5)        # 10

# Lambda with multiple parameters
let add = { |a, b| $a + $b }
echo (add 3 4)         # 7

# Lambda with type annotations
let parse_int = { |s: string| -> int: $s | into int }

# Closures capture surrounding variables
let multiplier = 3
let times_n = { |x| $x * $multiplier }
echo (times_n 7)       # 21

# Used directly in pipelines
[1, 2, 3, 4, 5] | each { |x| $x * $x }   # [1, 4, 9, 16, 25]

# Filter with closures
[1, 2, 3, 4, 5] | where { |x| $x > 3 }   # [4, 5]

# Sort with a custom key
let files = (ls)
$files | sort-by { |f| $f.size }
```

### Implementation

```rust
/// A closure captures its environment.
#[derive(Debug, Clone)]
pub struct Closure {
    pub params: Vec<Parameter>,
    pub body: Expression,  // or Block for multi-line closures
    pub return_type: Option<TypeAnnotation>,
    /// Captured variables from the surrounding scope at definition time.
    pub captures: HashMap<String, Value>,
}

impl Interpreter {
    /// Evaluate a closure literal — capture the current scope.
    fn evaluate_closure(
        &mut self,
        params: &[Parameter],
        body: &Expression,
    ) -> Result<Value, ShellError> {
        // Capture all variables currently in scope
        // (An optimization would be to only capture variables actually
        // referenced in the body, but this is simpler.)
        let captures = self.state.current_scope().clone();

        let closure = Closure {
            params: params.to_vec(),
            body: body.clone(),
            return_type: None,
            captures,
        };

        Ok(Value::Closure(Box::new(closure)))
    }

    /// Call a closure with arguments.
    fn call_closure(
        &mut self,
        closure: &Closure,
        args: &[Value],
    ) -> Result<Value, ShellError> {
        // Set up a scope with the captured variables
        self.push_scope();

        for (name, value) in &closure.captures {
            self.state.variables.insert(name.clone(), value.clone());
        }

        // Bind parameters
        for (param, arg) in closure.params.iter().zip(args.iter()) {
            if let Some(ref type_ann) = param.type_ann {
                self.check_type(arg, type_ann, &param.name)?;
            }
            self.state.variables.insert(param.name.clone(), arg.clone());
        }

        // Evaluate the body
        let result = self.evaluate(&closure.body);

        self.pop_scope();
        result
    }
}
```

### Pipeline integration

Closures become truly powerful when combined with pipeline operators:

```
# The `each` command applies a closure to every element
[1, 2, 3] | each { |x| $x * 2 }           # [2, 4, 6]

# `where` filters elements
1..100 | where { |x| $x mod 7 == 0 }       # [7, 14, 21, 28, ...]

# `reduce` folds a list
[1, 2, 3, 4, 5] | reduce { |acc, x| $acc + $x }  # 15

# Chaining — find the total size of Rust source files
ls **/*.rs
    | where { |f| $f.size > 0 }
    | each { |f| $f.size }
    | reduce { |acc, s| $acc + $s }
```

---

## Concept 5: Pattern Matching

Rust's `match` is one of its best features. We bring it to the shell:

### Syntax

```
# Match on a value
let status = 404
match $status {
    200 => echo "OK"
    301 | 302 => echo "Redirect"
    404 => echo "Not Found"
    500..599 => echo "Server Error"
    _ => echo "Unknown status: $status"
}

# Match with binding
let result = (curl -s https://api.example.com/user)
match $result {
    {name: $n, age: $a} if $a >= 18 => echo "$n is an adult"
    {name: $n, age: $a} => echo "$n is $a years old"
    null => echo "No user found"
    _ => echo "Unexpected response"
}

# Match returns a value
let label = match $exit_code {
    0 => "success"
    1 => "general error"
    2 => "usage error"
    126 => "not executable"
    127 => "not found"
    _ => $"error ($exit_code)"
}
```

### AST for match expressions

```rust
/// A single arm in a match expression.
#[derive(Debug, Clone)]
pub struct MatchArm {
    /// The pattern to match against.
    pub pattern: Pattern,
    /// Optional guard condition.
    pub guard: Option<Expression>,
    /// The body to execute if this arm matches.
    pub body: Expression,
}

/// Patterns that can appear in match arms.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// Match a literal value: 42, "hello", true
    Literal(Value),

    /// Match any value, bind to a variable: $x
    Binding(String),

    /// Wildcard — match anything, don't bind: _
    Wildcard,

    /// Match a range: 1..10, "a".."z"
    Range(Box<Expression>, Box<Expression>),

    /// Match multiple alternatives: 301 | 302 | 303
    Or(Vec<Pattern>),

    /// Destructure a record: {name: $n, age: $a}
    Record(Vec<(String, Pattern)>),

    /// Destructure a list: [$first, $second, ..$rest]
    List(Vec<Pattern>, Option<String>),  // rest pattern: ..$rest
}
```

### Match evaluation

```rust
impl Interpreter {
    fn evaluate_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
    ) -> Result<Value, ShellError> {
        let value = self.evaluate(subject)?;

        for arm in arms {
            // Try to match the pattern, collecting any bindings
            let mut bindings = HashMap::new();

            if self.pattern_matches(&value, &arm.pattern, &mut bindings) {
                // Check the guard condition if present
                if let Some(ref guard) = arm.guard {
                    // Temporarily bind the pattern variables
                    self.push_scope();
                    for (name, val) in &bindings {
                        self.state.variables.insert(name.clone(), val.clone());
                    }
                    let guard_result = self.evaluate(guard)?;
                    self.pop_scope();

                    if !guard_result.is_truthy() {
                        continue; // guard failed, try next arm
                    }
                }

                // Execute the arm body with bindings in scope
                self.push_scope();
                for (name, val) in bindings {
                    self.state.variables.insert(name, val);
                }
                let result = self.evaluate(&arm.body);
                self.pop_scope();
                return result;
            }
        }

        Err(ShellError::new(
            "non-exhaustive match: no arm matched the value",
            ErrorCategory::InternalError,
        ))
    }

    fn pattern_matches(
        &self,
        value: &Value,
        pattern: &Pattern,
        bindings: &mut HashMap<String, Value>,
    ) -> bool {
        match pattern {
            Pattern::Wildcard => true,

            Pattern::Literal(lit) => value == lit,

            Pattern::Binding(name) => {
                bindings.insert(name.clone(), value.clone());
                true
            }

            Pattern::Range(start_expr, end_expr) => {
                // Evaluate range bounds (cached in practice)
                if let (Ok(start), Ok(end)) = (
                    self.evaluate_const(start_expr),
                    self.evaluate_const(end_expr),
                ) {
                    value >= &start && value <= &end
                } else {
                    false
                }
            }

            Pattern::Or(alternatives) => {
                alternatives.iter().any(|alt| {
                    self.pattern_matches(value, alt, bindings)
                })
            }

            Pattern::Record(field_patterns) => {
                if let Value::Record(map) = value {
                    field_patterns.iter().all(|(key, pat)| {
                        match map.get(key) {
                            Some(field_val) => {
                                self.pattern_matches(field_val, pat, bindings)
                            }
                            None => false,
                        }
                    })
                } else {
                    false
                }
            }

            Pattern::List(elem_patterns, rest) => {
                if let Value::List(items) = value {
                    if items.len() < elem_patterns.len() {
                        return false;
                    }

                    // Match individual elements
                    for (item, pat) in items.iter().zip(elem_patterns.iter()) {
                        if !self.pattern_matches(item, pat, bindings) {
                            return false;
                        }
                    }

                    // Bind the rest if present
                    if let Some(rest_name) = rest {
                        let rest_items = items[elem_patterns.len()..].to_vec();
                        bindings.insert(
                            rest_name.clone(),
                            Value::List(rest_items),
                        );
                    } else if items.len() != elem_patterns.len() {
                        return false; // no rest pattern, lengths must match
                    }

                    true
                } else {
                    false
                }
            }
        }
    }
}
```

---

## Concept 6: Range Expressions

Ranges are first-class values in james-shell. They represent a sequence of values and can be used in loops, as function arguments, and in pattern matching.

```
# Integer ranges
1..5           # [1, 2, 3, 4] (exclusive end, like Rust)
1..=5          # [1, 2, 3, 4, 5] (inclusive end)
0..10          # [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]

# Ranges in for loops
for i in 1..=5 {
    echo $"Number: ($i)"
}

# Ranges with step
1..10 | step 2     # [1, 3, 5, 7, 9]

# Countdown
10..0              # [10, 9, 8, 7, 6, 5, 4, 3, 2, 1]

# Character ranges
'a'..'z'           # ['a', 'b', 'c', ..., 'y']
'A'..'Z'           # ['A', 'B', 'C', ..., 'Y']

# Ranges as slices
let items = [10, 20, 30, 40, 50]
$items | range 1..3     # [20, 30]
```

### Implementation

```rust
#[derive(Debug, Clone)]
pub struct Range {
    pub start: i64,
    pub end: i64,
    pub inclusive: bool,
    pub step: i64,
}

impl Range {
    pub fn new(start: i64, end: i64, inclusive: bool) -> Self {
        let step = if start <= end { 1 } else { -1 };
        Self { start, end, inclusive, step }
    }

    pub fn with_step(mut self, step: i64) -> Self {
        self.step = step;
        self
    }

    /// Iterate over the range, yielding each value.
    pub fn iter(&self) -> RangeIterator {
        RangeIterator {
            current: self.start,
            end: self.end,
            step: self.step,
            inclusive: self.inclusive,
            done: false,
        }
    }

    /// Materialize the range into a Vec.
    /// Be careful — this can be large!
    pub fn to_vec(&self) -> Vec<i64> {
        self.iter().collect()
    }
}

pub struct RangeIterator {
    current: i64,
    end: i64,
    step: i64,
    inclusive: bool,
    done: bool,
}

impl Iterator for RangeIterator {
    type Item = i64;

    fn next(&mut self) -> Option<i64> {
        if self.done {
            return None;
        }

        let in_bounds = if self.step > 0 {
            if self.inclusive {
                self.current <= self.end
            } else {
                self.current < self.end
            }
        } else {
            if self.inclusive {
                self.current >= self.end
            } else {
                self.current > self.end
            }
        };

        if in_bounds {
            let value = self.current;
            self.current += self.step;
            Some(value)
        } else {
            self.done = true;
            None
        }
    }
}
```

### Lazy evaluation

Ranges don't need to materialize the entire sequence in memory. In a pipeline like `1..1000000 | where { |x| is_prime $x } | first 10`, we never create a million-element list. The range produces values lazily, and `first 10` stops after collecting ten results.

```rust
impl Interpreter {
    fn execute_for_loop(
        &mut self,
        var_name: &str,
        iterable: &Expression,
        body: &Block,
    ) -> Result<Value, ShellError> {
        let iter_value = self.evaluate(iterable)?;

        match iter_value {
            Value::Range(range) => {
                // Iterate lazily — no materialization
                let mut last = Value::Null;
                for i in range.iter() {
                    self.state.variables.insert(
                        var_name.to_string(),
                        Value::Int(i),
                    );
                    last = self.execute_block(body)?;
                }
                Ok(last)
            }

            Value::List(items) => {
                let mut last = Value::Null;
                for item in items {
                    self.state.variables.insert(
                        var_name.to_string(),
                        item,
                    );
                    last = self.execute_block(body)?;
                }
                Ok(last)
            }

            _ => Err(ShellError::new(
                format!("cannot iterate over {:?}", iter_value),
                ErrorCategory::TypeError,
            )),
        }
    }
}
```

---

## Concept 7: Functional Pipelines

Traditional shell pipelines stream *text*. james-shell pipelines stream *structured data* (from Module 14). Combined with closures, this gives us a powerful functional programming style:

```
# List comprehension equivalent
let squares = 1..=10 | each { |x| $x * $x }
# [1, 4, 9, 16, 25, 36, 49, 64, 81, 100]

# Filter and transform
let large_rs_files = (ls **/*.rs
    | where { |f| $f.size > 1kb }
    | sort-by { |f| -$f.size }
    | each { |f| {name: $f.name, size: $f.size} })

# Group and aggregate
let by_extension = (ls
    | group-by { |f| $f.extension }
    | each { |group| {
        ext: $group.key,
        count: ($group.items | length),
        total_size: ($group.items | each { |f| $f.size } | math sum)
    }})

# String processing pipeline
let words = (open README.md
    | split "\n"
    | each { |line| $line | split " " }
    | flatten
    | where { |w| ($w | length) > 3 }
    | uniq
    | sort)
```

### Core pipeline commands

These are built-in commands that operate on structured data:

```rust
/// Register all the pipeline-aware commands.
fn register_pipeline_commands(registry: &mut CommandRegistry) {
    registry.register("each", |args, input| {
        // Apply a closure to each element of a list
        let closure = args.expect_closure(0)?;
        match input {
            Value::List(items) => {
                let results: Result<Vec<Value>, _> = items.iter()
                    .map(|item| call_closure(&closure, &[item.clone()]))
                    .collect();
                Ok(Value::List(results?))
            }
            _ => Err(type_error("each", "list", &input)),
        }
    });

    registry.register("where", |args, input| {
        // Filter elements by a predicate closure
        let predicate = args.expect_closure(0)?;
        match input {
            Value::List(items) => {
                let mut filtered = Vec::new();
                for item in items {
                    let result = call_closure(&predicate, &[item.clone()])?;
                    if result.is_truthy() {
                        filtered.push(item);
                    }
                }
                Ok(Value::List(filtered))
            }
            _ => Err(type_error("where", "list", &input)),
        }
    });

    registry.register("reduce", |args, input| {
        // Fold a list into a single value
        let reducer = args.expect_closure(0)?;
        match input {
            Value::List(items) if !items.is_empty() => {
                let mut acc = items[0].clone();
                for item in &items[1..] {
                    acc = call_closure(&reducer, &[acc, item.clone()])?;
                }
                Ok(acc)
            }
            Value::List(_) => Err(ShellError::new(
                "cannot reduce an empty list",
                ErrorCategory::InternalError,
            )),
            _ => Err(type_error("reduce", "list", &input)),
        }
    });

    registry.register("sort-by", |args, input| {
        // Sort elements by a key function
        let key_fn = args.expect_closure(0)?;
        match input {
            Value::List(mut items) => {
                // Compute keys for all items, then sort
                let mut keyed: Vec<(Value, Value)> = items.iter()
                    .map(|item| {
                        let key = call_closure(&key_fn, &[item.clone()]).unwrap_or(Value::Null);
                        (key, item.clone())
                    })
                    .collect();

                keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

                Ok(Value::List(keyed.into_iter().map(|(_, v)| v).collect()))
            }
            _ => Err(type_error("sort-by", "list", &input)),
        }
    });

    registry.register("group-by", |args, input| {
        // Group elements by a key function
        let key_fn = args.expect_closure(0)?;
        match input {
            Value::List(items) => {
                let mut groups: IndexMap<String, Vec<Value>> = IndexMap::new();
                for item in items {
                    let key = call_closure(&key_fn, &[item.clone()])?;
                    let key_str = key.to_display_string();
                    groups.entry(key_str).or_default().push(item);
                }

                let result: Vec<Value> = groups.into_iter()
                    .map(|(key, items)| {
                        let mut record = HashMap::new();
                        record.insert("key".into(), Value::String(key));
                        record.insert("items".into(), Value::List(items));
                        Value::Record(record)
                    })
                    .collect();

                Ok(Value::List(result))
            }
            _ => Err(type_error("group-by", "list", &input)),
        }
    });
}
```

---

## Concept 8: Module and Import System

As scripts grow, they need to be organized into reusable modules. james-shell supports a simple module system:

### Defining a module

```
# file: ~/.jsh/lib/utils.jsh

# Public functions (exported by default)
def greet(name: string) {
    echo $"Hello, ($name)!"
}

def sum(items: list<int>) -> int {
    $items | reduce { |acc, x| $acc + $x }
}

# Private functions (prefixed with _)
def _internal_helper() {
    # not visible to importers
}

# Module-level constants
let VERSION = "1.0.0"
```

### Importing

```
# Import everything from a module
use utils

utils greet "James"       # "Hello, James!"
echo $utils.VERSION       # "1.0.0"

# Import specific items
use utils { greet, sum }

greet "World"             # "Hello, World!"
let total = sum [1, 2, 3] # 6

# Import with an alias
use utils as u
u greet "Alias"

# Import from a path
use "./lib/deploy.jsh"
use "~/.jsh/lib/logging.jsh"
```

### Implementation

```rust
use std::collections::HashMap;
use std::path::PathBuf;

/// A loaded module with its exported symbols.
#[derive(Debug, Clone)]
pub struct Module {
    pub name: String,
    pub path: PathBuf,
    pub functions: HashMap<String, FunctionDef>,
    pub constants: HashMap<String, Value>,
}

/// The module loader — caches loaded modules to avoid reprocessing.
pub struct ModuleLoader {
    /// Search paths for modules (in order of priority).
    pub search_paths: Vec<PathBuf>,
    /// Cache of already-loaded modules.
    loaded: HashMap<String, Module>,
}

impl ModuleLoader {
    pub fn new() -> Self {
        let mut search_paths = Vec::new();

        // Current directory
        if let Ok(cwd) = std::env::current_dir() {
            search_paths.push(cwd);
        }

        // User's lib directory
        if let Some(home) = dirs::home_dir() {
            search_paths.push(home.join(".jsh").join("lib"));
        }

        // System lib directory
        if cfg!(unix) {
            search_paths.push(PathBuf::from("/usr/share/jsh/lib"));
        }

        Self {
            search_paths,
            loaded: HashMap::new(),
        }
    }

    /// Load a module by name, searching the search paths.
    pub fn load(&mut self, name: &str) -> Result<&Module, ShellError> {
        if self.loaded.contains_key(name) {
            return Ok(&self.loaded[name]);
        }

        // If name is an absolute or relative path, use it directly
        let path = if name.contains('/') || name.contains('\\') {
            let expanded = shellexpand::tilde(name);
            PathBuf::from(expanded.as_ref())
        } else {
            // Search the module paths
            self.find_module(name)?
        };

        let source = std::fs::read_to_string(&path).map_err(|e| {
            ShellError::new(
                format!("cannot load module '{}': {}", name, e),
                ErrorCategory::IOError,
            )
        })?;

        // Parse the module source
        let ast = parse(&source)?;

        // Extract exports (functions and top-level constants)
        let mut functions = HashMap::new();
        let mut constants = HashMap::new();

        for stmt in &ast.statements {
            match stmt {
                Statement::FunctionDef(func) => {
                    // Skip private functions (prefixed with _)
                    if !func.name.starts_with('_') {
                        functions.insert(func.name.clone(), func.clone());
                    }
                }
                Statement::Assignment(assign) if assign.is_const => {
                    if !assign.name.starts_with('_') {
                        constants.insert(assign.name.clone(), assign.value.clone());
                    }
                }
                _ => {}
            }
        }

        let module = Module {
            name: name.to_string(),
            path,
            functions,
            constants,
        };

        self.loaded.insert(name.to_string(), module);
        Ok(&self.loaded[name])
    }

    fn find_module(&self, name: &str) -> Result<PathBuf, ShellError> {
        let filename = format!("{}.jsh", name);

        for dir in &self.search_paths {
            let candidate = dir.join(&filename);
            if candidate.exists() {
                return Ok(candidate);
            }
        }

        Err(ShellError::new(
            format!(
                "module '{}' not found in search paths: {:?}",
                name, self.search_paths
            ),
            ErrorCategory::IOError,
        ))
    }
}
```

---

## Concept 9: Comparison with Other Languages

How does james-shell's scripting compare to bash, Python, Ruby, and Nushell?

| Feature | Bash | Python | Ruby | Nushell | james-shell |
|---------|------|--------|------|---------|-------------|
| **Named params** | No (`$1`, `$2`) | Yes | Yes | Yes | Yes |
| **Type system** | None | Dynamic | Dynamic | Rich | Optional annotations |
| **String interpolation** | `"$var"` (confusing) | `f"{var}"` | `"#{var}"` | `$"(expr)"` | `$"(expr)"` |
| **Closures** | No | `lambda` | Blocks `{ }` | Blocks `{ \|\| }` | `{ \|x\| expr }` |
| **Pattern matching** | `case` (glob-only) | `match` (3.10+) | `case` (3.0+) | Yes | `match` with destructuring |
| **Ranges** | `{1..10}` (brace expansion) | `range(1, 10)` | `1..10` | `1..10` | `1..10`, `1..=10` |
| **Modules** | `source` (dumps everything) | `import` | `require` | `use` | `use` with selective imports |
| **Pipeline data** | Text only | N/A | N/A | Structured | Structured |
| **Running commands** | Native | `subprocess` | Backticks | Native | Native |

james-shell sits in a unique position: it has the command-running ergonomics of a shell, the data types and scripting power of a programming language, and a syntax that was designed from scratch to be readable.

### The key design principle

james-shell follows this rule: **things that look similar should behave similarly, and things that behave differently should look different.**

This is why `$"..."` uses different syntax from `"..."` — they behave differently (interpolation vs literal), so they should look different. In bash, `"$foo"` and `'$foo'` look almost identical but behave completely differently, which is a constant source of bugs.

---

## Key Rust Concepts Used

| Concept | Where it appears |
|---------|-----------------|
| **Enums with data** | `Pattern`, `Expression`, `TypeAnnotation` — rich ASTs with associated data |
| **Iterator trait** | `RangeIterator` implements `Iterator` for lazy evaluation |
| **HashMap and closures** | Scope management, captured variables in closures |
| **Trait objects** | Pipeline commands as registered closures |
| **Box for heap allocation** | `Closure` contains `Box<Expression>` for recursive structures |
| **Pattern matching (in Rust)** | Our shell's `match` is implemented using Rust's own `match` |
| **Lifetime elision** | Careful management of borrows when evaluating closures with captures |
| **Generic functions** | Type coercion system using generic `Into<String>` |
| **IndexMap** | Ordered maps for `group-by` to preserve insertion order |

---

## Milestone

After completing this module, your shell should handle all of the following:

```
jsh> def greet(name: string, times: int = 1) {
       for i in 1..=$times {
           echo $"($i). Hello, ($name)!"
       }
     }

jsh> greet "World" 3
1. Hello, World!
2. Hello, World!
3. Hello, World!

jsh> greet --name "James" --times 2
1. Hello, James!
2. Hello, James!

jsh> let double = { |x| $x * 2 }
jsh> [1, 2, 3, 4, 5] | each $double
[2, 4, 6, 8, 10]

jsh> 1..=10 | where { |x| $x mod 2 == 0 }
[2, 4, 6, 8, 10]

jsh> let x: int = "hello"
Error [TypeError]: type mismatch: expected int, got string

jsh> let status = 404
jsh> match $status {
       200 => "OK"
       404 => "Not Found"
       500..599 => "Server Error"
       _ => "Unknown"
     }
Not Found

jsh> use utils { greet, sum }
jsh> sum [10, 20, 30]
60

jsh> ls | where { |f| $f.extension == "rs" } | sort-by { |f| $f.size } | first 3
╭───┬──────────────┬───────┬────────────╮
│ # │     name     │ size  │  modified  │
├───┼──────────────┼───────┼────────────┤
│ 0 │ lib.rs       │  142B │ 2 days ago │
│ 1 │ config.rs    │  891B │ yesterday  │
│ 2 │ parser.rs    │ 2.1KB │ 3 hours ago│
╰───┴──────────────┴───────┴────────────╯
```

---

## What's next?

We now have a powerful, modern scripting language built into our shell. But the best features aren't always the ones *we* build — they're the ones the community builds. In **Module 20: Plugin System**, we design an extensible plugin architecture that lets anyone add new commands, completers, and capabilities to james-shell without modifying the core codebase.

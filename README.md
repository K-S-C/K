# K

K is a small, self-contained, general-purpose programming language.
Variables, closures, classes, error handling, and matrix math are all
**built-in syntax** — not a library you import. Source compiles to
bytecode and runs on a stack-based VM: one binary, no interpreter to
install, no package manager.

```k
class Dog {
    fn init(name) { self.name = name; }
    fn speak() { return f"{self.name} says Woof!"; }
}
let d = new Dog("Rex");
print(d.speak());

let weights = [[0.8, -0.1], [0.4, 0.9]];
let inputs  = [[1.5, 0.2]];
print(relu(inputs @ weights));   // matrix multiply is a language operator, '@'
```

## Status

**v1.0.0 — initial release.** The core language, its bytecode
compiler, and its VM are real and working. Type annotations are
parsed but not yet enforced (documentation only for now), and the
standard library is intentionally small. See [Roadmap](#roadmap)
below for what's next.

## Install

Prebuilt installers for the current release are on the
[Releases page](https://github.com/K-S-C/K/releases):

| Platform | Asset |
|---|---|
| Windows | `K-Setup.exe` |
| macOS | `K-Installer.pkg` |
| Linux (Debian/Ubuntu) | `k-language_*_amd64.deb` (`sudo dpkg -i k-language_*_amd64.deb`) |

Each installer includes both the `k` CLI and the built-in GUI IDE, and
adds `k` to your `PATH`.

### Build from source

K is written in Rust. With a recent stable toolchain and Cargo installed:

```bash
git clone https://github.com/K-S-C/K.git
cd K
cargo build --release
```

The binary is at `target/release/k`.

## Usage

```
k script.k        Lex -> parse -> compile -> run the file, print output
k                  Start the interactive REPL
k repl             Same as above, explicit
k gui              Launch the built-in graphical IDE
k -h, --help       Print usage
k -v, --version    Print version
```

Every stage of the pipeline (lexer, parser, compiler) returns a
`Result` — a bad script never crashes the process. It prints a
`Lex error:`, `Parse error:`, or `Compile error:` and stops cleanly.

### REPL

One VM instance stays alive for the whole session, so anything you
define is visible on the next line. It supports arrow-key history,
`Ctrl+R` search, and keeps reading additional lines while you have an
unclosed `{`, `(`, or `[`.

```
$ k repl
K Language REPL v1.0.0 — :help for commands, Ctrl+D or :exit to quit
k> let x = 40;
k> print(x + 2);
42
k> :vars
  x = 40
k> :exit
Goodbye.
```

Special REPL commands: `:help`, `:load <file.k>`, `:vars`, `:clear`,
`:exit` / `:q` (or `Ctrl+D`).

## Language tour

- **Variables** — `let` (mutable) and `const` (intent signal; not yet
  enforced by the compiler). Optional `: type` annotations are parsed
  for readability and currently ignored.
- **Types** — `int` (`i64`), `float` (`f64`), `str`, `bool`, `null`,
  `list`, `dict`, plus `func`, `class`, and `instance`. Lists and
  dicts are reference types — assigning or passing one shares the
  underlying storage.
- **Control flow** — `if` / `elif` / `else`, `while`, `for ... in`
  (over a list, a string, or a dict's keys), `break` / `continue`.
- **Functions** — `fn name(params) { }`, default parameter values,
  full recursion, and anonymous function expressions
  (`fn(x) { return x * x; }`).
- **Closures** — real upvalue capture. Only variables actually
  captured by a nested function get boxed into a shared cell; plain
  locals stay on the stack.
- **Classes** — `class Name(Parent) { }` for single inheritance,
  implicit `self`, `init` as the constructor, dynamic method dispatch.
- **Errors** — `try { } catch e { }` / `throw expr;`. Runtime errors
  (division by zero, a bad index, a missing key) are catchable
  values, never a crash, and unwind correctly across nested calls.
- **String interpolation** — `f"Hello, {name}! {n + 1} more."`
- **Matrix math** — matrices are lists of lists of numbers; `@` is a
  real binary operator for matrix multiplication, alongside
  `relu`, `sigmoid`, `tanh`, `softmax`, and `transpose` as builtins
  that work elementwise on scalars, flat lists, or nested lists.

See the annotated scripts in [`examples/`](examples/) for full worked
programs.

## Known limitations (being upfront about these)

- `const` currently compiles identically to `let` — there is no
  compile-time enforcement yet.
- `Dict == Dict` and `Instance == Instance` always evaluate to `false`,
  even for identical data — the VM's equality check has no case for
  those types.
- `input()` is a stub; it always returns `""`.
- Calling a function with too few or too many arguments does not
  error — missing args become `null`, extras are silently dropped.
- Dict iteration order is not guaranteed (backed by a hash map).
- The standard library is intentionally minimal today: no file I/O,
  no `map`/`filter`/`reduce`, no general math beyond what's listed
  above.

## Roadmap

- Real static type checking (or removing the annotation syntax if we
  don't commit to enforcing it)
- A standard library: file I/O, JSON, general math builtins, and
  collection helpers
- Tooling: a formatter, a test runner, and editor support
- A package manager

## Contributing

Issues and pull requests are welcome — see
[Issues](https://github.com/K-S-C/K/issues). If you're planning a
larger change, consider opening an issue first to discuss direction.

## License

MIT — see [`LICENSE`](LICENSE).

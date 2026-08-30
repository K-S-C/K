# K

K is a self-contained, general-purpose programming language: variables,
closures, classes, error handling, and matrix math as **built-in syntax**,
not a library you import. It compiles to bytecode and runs on its own VM —
no interpreter to install, no package manager, no virtualenv, one binary.

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

## Why K exists
K: **zero setup cost.** Multiplying two matrices in Python means Python 
installed, then `pip` working, then a virtualenv, before line one of actual math runs. In K it's one binary and one line.  

## Architecture: bytecode VM

The previous iteration of this codebase evaluated the AST directly —
walking the parsed tree and re-matching node types on every single
execution. This version compiles the AST to flat bytecode once
(`compiler.rs`) and executes that with a real VM (`vm.rs`): a stack-based
loop over bytes, variables resolved to array slot indices at *compile
time* instead of walked through a HashMap-based scope chain at every
access.

**Measured:** `fib(27)` (832,039 function calls) — tree-walker
209–226ms, bytecode VM 187–192ms, both release builds, averaged over 3
runs each. That's a real ~15% improvement, not the 5-10x a bytecode VM can
theoretically deliver, and the gap between "real" and "theoretical" is
worth being specific about: every local variable is currently boxed as
`Rc<RefCell<Value>>` uniformly (simplest way to get closures correct),
which means every function call still does 2+ heap allocations before
running a single instruction — similar in cost to the tree-walker's
per-call scope allocation, just shaped differently. The dispatch loop
itself is genuinely faster; that win is currently being offset by
allocation overhead elsewhere

We have verified via recursion, 3-level nested-closure upvalue
chains, mutable closures (independent counters don't share state), class
inheritance with implicit `self`, try/catch across nested function calls,
break/continue including in nested loops, default parameters, matrix `@`
and the ML builtins, dict/list/string methods, and deep recursion
(500-level) without incident:
- `let`/`const`, `if`/`elif`/`else`, `while`, `for..in`, `break`/`continue`
- Functions with default parameters and real closures resolved at compile
  time to stack slots or upvalue chains (not a runtime scope search)
- Recursion, including through nested/local function declarations
- Classes with single inheritance and implicit `self`
- `try`/`catch`/`throw` as VM-level handler stack — errors unwind cleanly
  across nested calls to the nearest active handler; nothing panics
- Lists, dicts, strings with real methods (`.append`, `.sort`, `.keys`,
  `.upper`, `.split`, `.replace`, …)
- `f"...{expr}..."` string interpolation
- Matrices as nested lists with `@` (matmul), `relu`, `sigmoid`, `tanh`,
  `softmax`, `transpose`, `flatten`
- A GUI IDE (`k gui`) with a dark/light theme, basic syntax highlighting,
  Open/Save/Save As, and keyboard shortcuts (F5 run, Ctrl+S save, Ctrl+O
  open, Ctrl+N new)
- A terminal REPL (`k repl`) with arrow-key history, Ctrl+R search, and
  multi-line input (auto-continues while braces are open) via
  `rustyline`; `:help`, `:load <file>`, `:vars`, `:clear`, `:exit` — the
  REPL keeps one VM alive across lines so variables persist between them.  


## License

MIT — see `LICENSE`.

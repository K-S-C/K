//! Runtime value types for the bytecode VM. Deliberately similar to the
//! tree-walker's `Value` (same semantics, same builtins) so behavior is
//! consistent between the two engines while execution itself is now a
//! flat bytecode loop instead of AST recursion.

use crate::chunk::Chunk;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

pub type Rid<T> = Rc<RefCell<T>>;
/// A captured variable cell. Regular locals live directly on the VM stack;
/// only locals actually captured by a nested closure get boxed into one of
/// these, so the common (non-closure) case pays no extra indirection.
pub type Cell = Rc<RefCell<Value>>;

#[derive(Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(Rc<String>),
    Bool(bool),
    Null,
    List(Rid<Vec<Value>>),
    Dict(Rid<HashMap<String, Value>>),
    Closure(Rc<ClosureObj>),
    BoundMethod(Box<Value>, Rc<ClosureObj>),
    Native(&'static str),
    Class(Rc<ClassObj>),
    Instance(Rid<InstanceObj>),
}

pub struct FunctionObj {
    pub name: String,
    pub arity: usize,
    /// Total local-variable slots this function's body uses (self/receiver +
    /// parameters + every `let`/for-loop/catch binding declared anywhere in
    /// the body) — not just `arity`. Call frames must allocate this many
    /// slots, or any `let` beyond the parameter list indexes out of bounds.
    pub local_count: usize,
    pub chunk: Chunk,
    pub upvalue_count: usize,
}

pub struct ClosureObj {
    pub function: Rc<FunctionObj>,
    pub upvalues: Vec<Cell>,
}

pub struct ClassObj {
    pub name: String,
    pub parent: Option<Rc<ClassObj>>,
    pub methods: RefCell<HashMap<String, Rc<ClosureObj>>>,
}

pub struct InstanceObj {
    pub class: Rc<ClassObj>,
    pub fields: HashMap<String, Value>,
}

pub fn find_method(class: &Rc<ClassObj>, name: &str) -> Option<Rc<ClosureObj>> {
    if let Some(m) = class.methods.borrow().get(name) { return Some(m.clone()); }
    class.parent.as_ref().and_then(|p| find_method(p, name))
}

pub fn to_display(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Float(f) => if f.fract() == 0.0 && f.is_finite() { format!("{:.1}", f) } else { f.to_string() },
        Value::Str(s) => (**s).clone(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::List(items) => format!("[{}]", items.borrow().iter().map(to_repr).collect::<Vec<_>>().join(", ")),
        Value::Dict(d) => format!("{{{}}}", d.borrow().iter().map(|(k, v)| format!("\"{}\": {}", k, to_repr(v))).collect::<Vec<_>>().join(", ")),
        Value::Closure(c) => format!("<fn {}>", c.function.name),
        Value::BoundMethod(_, c) => format!("<method {}>", c.function.name),
        Value::Native(n) => format!("<builtin {}>", n),
        Value::Class(c) => format!("<class {}>", c.name),
        Value::Instance(i) => format!("<{} instance>", i.borrow().class.name),
    }
}

fn to_repr(v: &Value) -> String { match v { Value::Str(s) => format!("\"{}\"", s), other => to_display(other) } }

pub fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "int", Value::Float(_) => "float", Value::Str(_) => "str", Value::Bool(_) => "bool",
        Value::Null => "null", Value::List(_) => "list", Value::Dict(_) => "dict",
        Value::Closure(_) => "func", Value::BoundMethod(..) => "method", Value::Native(_) => "builtin",
        Value::Class(_) => "class", Value::Instance(_) => "instance",
    }
}

pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Null => false,
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !s.is_empty(),
        Value::List(l) => !l.borrow().is_empty(),
        Value::Dict(d) => !d.borrow().is_empty(),
        _ => true,
    }
}

pub fn value_eq(a: &Value, b: &Value) -> bool {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => x == y,
        (Float(x), Float(y)) => x == y,
        (Int(x), Float(y)) | (Float(y), Int(x)) => *x as f64 == *y,
        (Str(x), Str(y)) => x == y,
        (Bool(x), Bool(y)) => x == y,
        (Null, Null) => true,
        (List(x), List(y)) => {
            let (xb, yb) = (x.borrow(), y.borrow());
            xb.len() == yb.len() && xb.iter().zip(yb.iter()).all(|(p, q)| value_eq(p, q))
        }
        _ => false,
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", to_display(self)) }
}

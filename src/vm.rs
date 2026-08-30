//! The VM: a flat per-frame instruction loop (no AST recursion) plus the
//! native builtin implementations. Nested K function calls recurse through
//! `run_frame` at the Rust level (one Rust stack frame per K call), which
//! is the same approach real interpreters like CPython's `ceval` use —
//! the win here isn't eliminating native recursion, it's eliminating the
//! AST-node dispatch and HashMap-based variable lookups that dominated the
//! tree-walker's per-step cost.

use crate::chunk::OpCode;
use crate::value::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub struct VM {
    pub globals: HashMap<String, Value>,
    pub output: String,
}

const BUILTINS: &[&str] = &[
    "print", "len", "str", "int", "float", "bool", "type", "range",
    "abs", "min", "max", "sum", "sorted", "round", "input",
    "relu", "sigmoid", "tanh", "softmax", "transpose", "flatten",
];

impl VM {
    pub fn new() -> Self {
        let mut globals = HashMap::new();
        for name in BUILTINS { globals.insert(name.to_string(), Value::Native(name)); }
        VM { globals, output: String::new() }
    }

    pub fn run_program(&mut self, function: Rc<FunctionObj>) -> String {
        let local_count = function.local_count;
        let closure = Rc::new(ClosureObj { function, upvalues: Vec::new() });
        let mut locals: Vec<Cell> = Vec::with_capacity(local_count);
        for _ in 0..local_count { locals.push(Rc::new(RefCell::new(Value::Null))); }
        match self.run_frame(closure, locals) {
            Ok(_) => {}
            Err(v) => self.output.push_str(&format!("Uncaught error: {}\n", to_display(&v))),
        }
        std::mem::take(&mut self.output)
    }

    /// Executes one function's bytecode to completion, returning its result.
    /// A K-level function call (Call/Invoke/Instantiate) recurses into this
    /// via `call_value`; a thrown/propagated error unwinds naturally through
    /// the `?` operator until an enclosing try/catch's handler catches it.
    fn run_frame(&mut self, closure: Rc<ClosureObj>, mut locals: Vec<Cell>) -> Result<Value, Value> {
        let code_ptr: *const Vec<u8> = &closure.function.chunk.code;
        let const_ptr: *const Vec<Value> = &closure.function.chunk.constants;
        // Safety: `closure` (and thus its chunk) stays alive for the whole
        // function; we never mutate the chunk while executing it.
        let code: &Vec<u8> = unsafe { &*code_ptr };
        let constants: &Vec<Value> = unsafe { &*const_ptr };

        let mut stack: Vec<Value> = Vec::new();
        let mut ip: usize = 0;
        let mut handlers: Vec<(usize, usize)> = Vec::new(); // (stack_len_at_push, catch_ip)

        loop {
            let byte = code[ip];
            ip += 1;
            let op = OpCode::from_u8(byte);
            match self.exec_one(op, code, &mut ip, constants, &mut stack, &mut locals, &closure, &mut handlers) {
                Ok(None) => continue,
                Ok(Some(v)) => return Ok(v),
                Err(errval) => {
                    if let Some((slen, catch_ip)) = handlers.pop() {
                        stack.truncate(slen);
                        stack.push(errval);
                        ip = catch_ip;
                        continue;
                    }
                    return Err(errval);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn exec_one(
        &mut self,
        op: OpCode,
        code: &[u8],
        ip: &mut usize,
        constants: &[Value],
        stack: &mut Vec<Value>,
        locals: &mut Vec<Cell>,
        closure: &Rc<ClosureObj>,
        handlers: &mut Vec<(usize, usize)>,
    ) -> Result<Option<Value>, Value> {
        macro_rules! read_u16 { () => {{ let v = ((code[*ip] as u16) << 8) | (code[*ip+1] as u16); *ip += 2; v }} }
        macro_rules! read_u8 { () => {{ let v = code[*ip]; *ip += 1; v }} }
        macro_rules! push { ($v:expr) => { stack.push($v) } }
        macro_rules! pop { () => { stack.pop().expect("VM stack underflow (compiler bug)") } }

        match op {
            OpCode::Constant => { let idx = read_u16!(); push!(constants[idx as usize].clone()); }
            OpCode::Nil => push!(Value::Null),
            OpCode::True => push!(Value::Bool(true)),
            OpCode::False => push!(Value::Bool(false)),
            OpCode::Pop => { pop!(); }
            OpCode::GetLocal => { let slot = read_u8!(); push!(locals[slot as usize].borrow().clone()); }
            OpCode::SetLocal => { let slot = read_u8!(); let v = pop!(); *locals[slot as usize].borrow_mut() = v.clone(); push!(v); }
            OpCode::GetUpvalue => { let idx = read_u8!(); push!(closure.upvalues[idx as usize].borrow().clone()); }
            OpCode::SetUpvalue => { let idx = read_u8!(); let v = pop!(); *closure.upvalues[idx as usize].borrow_mut() = v.clone(); push!(v); }
            OpCode::GetGlobal => {
                let idx = read_u16!();
                let name = str_const(constants, idx);
                match self.globals.get(name) { Some(v) => push!(v.clone()), None => return Err(rt_err(format!("undefined variable '{}'", name))) }
            }
            OpCode::DefineGlobal => { let idx = read_u16!(); let name = str_const(constants, idx).to_string(); let v = pop!(); self.globals.insert(name, v); }
            OpCode::SetGlobal => {
                let idx = read_u16!();
                let name = str_const(constants, idx).to_string();
                let v = pop!();
                if !self.globals.contains_key(&name) { return Err(rt_err(format!("undefined variable '{}'", name))); }
                self.globals.insert(name, v.clone());
                push!(v);
            }
            OpCode::GetProperty => {
                let idx = read_u16!();
                let name = str_const(constants, idx).to_string();
                let target = pop!();
                push!(self.get_property(&target, &name)?);
            }
            OpCode::SetProperty => {
                let idx = read_u16!();
                let name = str_const(constants, idx).to_string();
                let value = pop!();
                let target = pop!();
                match &target {
                    Value::Instance(inst) => { inst.borrow_mut().fields.insert(name, value.clone()); push!(value); }
                    _ => return Err(rt_err(format!("cannot set field '{}' on {}", name, type_name(&target)))),
                }
            }
            OpCode::GetIndex => { let index = pop!(); let target = pop!(); push!(self.get_index(&target, &index)?); }
            OpCode::SetIndex => {
                let value = pop!(); let index = pop!(); let target = pop!();
                self.set_index(&target, &index, value.clone())?;
                push!(value);
            }
            OpCode::BuildList => {
                let count = read_u16!() as usize;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count { items.push(pop!()); }
                items.reverse();
                push!(Value::List(Rc::new(RefCell::new(items))));
            }
            OpCode::BuildDict => {
                let pairs = read_u16!() as usize;
                let mut flat = Vec::with_capacity(pairs * 2);
                for _ in 0..pairs * 2 { flat.push(pop!()); }
                flat.reverse();
                let mut map = HashMap::new();
                for chunk2 in flat.chunks(2) {
                    let key = match &chunk2[0] { Value::Str(s) => (**s).clone(), other => to_display(other) };
                    map.insert(key, chunk2[1].clone());
                }
                push!(Value::Dict(Rc::new(RefCell::new(map))));
            }
            OpCode::Equal => { let b = pop!(); let a = pop!(); push!(Value::Bool(value_eq(&a, &b))); }
            OpCode::NotEqual => { let b = pop!(); let a = pop!(); push!(Value::Bool(!value_eq(&a, &b))); }
            OpCode::Greater | OpCode::GreaterEqual | OpCode::Less | OpCode::LessEqual => {
                let b = pop!(); let a = pop!();
                let r = match (&a, &b) {
                    (Value::Str(x), Value::Str(y)) => match op { OpCode::Greater => x > y, OpCode::GreaterEqual => x >= y, OpCode::Less => x < y, _ => x <= y },
                    _ => { let (x, y) = (num(&a)?, num(&b)?); match op { OpCode::Greater => x > y, OpCode::GreaterEqual => x >= y, OpCode::Less => x < y, _ => x <= y } }
                };
                push!(Value::Bool(r));
            }
            OpCode::Add => { let b = pop!(); let a = pop!(); push!(add_values(&a, &b)?); }
            OpCode::Subtract => { let b = pop!(); let a = pop!(); push!(arith(&a, &b, i64::wrapping_sub, |x, y| x - y)?); }
            OpCode::Multiply => { let b = pop!(); let a = pop!(); push!(arith(&a, &b, i64::wrapping_mul, |x, y| x * y)?); }
            OpCode::Divide => { let b = pop!(); let a = pop!(); let d = num(&b)?; if d == 0.0 { return Err(rt_err("division by zero")); } push!(Value::Float(num(&a)? / d)); }
            OpCode::Modulo => { let b = pop!(); let a = pop!(); push!(match (&a, &b) { (Value::Int(x), Value::Int(y)) if *y != 0 => Value::Int(x % y), _ => Value::Float(num(&a)? % num(&b)?) }); }
            OpCode::Power => { let b = pop!(); let a = pop!(); push!(Value::Float(num(&a)?.powf(num(&b)?))); }
            OpCode::MatMul => { let b = pop!(); let a = pop!(); push!(matmul(&a, &b)?); }
            OpCode::Not => { let v = pop!(); push!(Value::Bool(!truthy(&v))); }
            OpCode::Negate => { let v = pop!(); push!(Value::Float(-num(&v)?)); }
            OpCode::ToStr => { let v = pop!(); push!(Value::Str(Rc::new(to_display(&v)))); }
            OpCode::Jump => { let target = read_u16!(); *ip = target as usize; }
            OpCode::JumpIfFalse => { let target = read_u16!(); if !truthy(stack.last().expect("VM stack underflow (compiler bug)")) { *ip = target as usize; } }
            OpCode::Loop => { let target = read_u16!(); *ip = target as usize; }
            OpCode::PushTry => { let catch_ip = read_u16!(); handlers.push((stack.len(), catch_ip as usize)); }
            OpCode::PopTry => { handlers.pop(); }
            OpCode::Throw => { let v = pop!(); return Err(v); }
            OpCode::Len => {
                let v = pop!();
                push!(Value::Int(match &v {
                    Value::List(l) => l.borrow().len() as i64,
                    Value::Dict(d) => d.borrow().len() as i64,
                    Value::Str(s) => s.chars().count() as i64,
                    other => return Err(rt_err(format!("len() requires a list, dict, or string, got {}", type_name(other)))),
                }));
            }
            OpCode::GetIterList => {
                let v = pop!();
                push!(match &v {
                    Value::List(_) => v,
                    Value::Str(s) => Value::List(Rc::new(RefCell::new(s.chars().map(|c| Value::Str(Rc::new(c.to_string()))).collect()))),
                    Value::Dict(d) => Value::List(Rc::new(RefCell::new(d.borrow().keys().map(|k| Value::Str(Rc::new(k.clone()))).collect()))),
                    other => return Err(rt_err(format!("value of type {} is not iterable", type_name(other)))),
                });
            }
            OpCode::Closure => {
                let fn_idx = read_u16!();
                let template_fn = match &constants[fn_idx as usize] { Value::Closure(c) => c.function.clone(), _ => unreachable!("compiler always stores a closure template here") };
                let mut upvalues = Vec::with_capacity(template_fn.upvalue_count);
                for _ in 0..template_fn.upvalue_count {
                    let is_local = read_u8!() != 0;
                    let index = read_u8!();
                    upvalues.push(if is_local { locals[index as usize].clone() } else { closure.upvalues[index as usize].clone() });
                }
                push!(Value::Closure(Rc::new(ClosureObj { function: template_fn, upvalues })));
            }
            OpCode::Class => {
                let idx = read_u16!();
                let name = str_const(constants, idx).to_string();
                let parent_val = pop!();
                let parent = match parent_val { Value::Null => None, Value::Class(c) => Some(c), _ => return Err(rt_err("base class must be a class")) };
                push!(Value::Class(Rc::new(ClassObj { name, parent, methods: RefCell::new(HashMap::new()) })));
            }
            OpCode::Method => {
                let idx = read_u16!();
                let name = str_const(constants, idx).to_string();
                let method_val = pop!();
                let closure_obj = match method_val { Value::Closure(c) => c, _ => unreachable!("compiler always emits a closure before Method") };
                if let Some(Value::Class(c)) = stack.last() { c.methods.borrow_mut().insert(name, closure_obj); }
            }
            OpCode::Inherit => {}
            OpCode::Call => {
                let argc = read_u8!() as usize;
                let mut args = Vec::with_capacity(argc);
                for _ in 0..argc { args.push(pop!()); }
                args.reverse();
                let callee = pop!();
                push!(self.call_value(callee, args)?);
            }
            OpCode::Invoke => {
                let idx = read_u16!();
                let name = str_const(constants, idx).to_string();
                let argc = read_u8!() as usize;
                let mut args = Vec::with_capacity(argc);
                for _ in 0..argc { args.push(pop!()); }
                args.reverse();
                let target = pop!();
                push!(self.invoke(target, &name, args)?);
            }
            OpCode::Instantiate => {
                let argc = read_u8!() as usize;
                let mut args = Vec::with_capacity(argc);
                for _ in 0..argc { args.push(pop!()); }
                args.reverse();
                let class_val = pop!();
                let class = match class_val { Value::Class(c) => c, _ => return Err(rt_err(format!("cannot instantiate {}: not a class", type_name(&class_val)))) };
                let instance = Rc::new(RefCell::new(InstanceObj { class: class.clone(), fields: HashMap::new() }));
                if let Some(init) = find_method(&class, "init") {
                    self.invoke_closure(&init, Value::Instance(instance.clone()), args)?;
                }
                push!(Value::Instance(instance));
            }
            OpCode::Return => { return Ok(Some(pop!())); }
            OpCode::JumpIfFalsePop => { let target = read_u16!(); let v = pop!(); if !truthy(&v) { *ip = target as usize; } }
        }
        Ok(None)
    }

    fn get_property(&self, target: &Value, name: &str) -> Result<Value, Value> {
        match target {
            Value::Instance(inst) => {
                let b = inst.borrow();
                if let Some(v) = b.fields.get(name) { return Ok(v.clone()); }
                if let Some(m) = find_method(&b.class, name) { return Ok(Value::BoundMethod(Box::new(target.clone()), m)); }
                Err(rt_err(format!("no field or method '{}' on {}", name, b.class.name)))
            }
            Value::Dict(d) => d.borrow().get(name).cloned().ok_or_else(|| rt_err(format!("key '{}' not found", name))),
            _ => Err(rt_err(format!("cannot access field '{}' on {}", name, type_name(target)))),
        }
    }

    fn get_index(&self, target: &Value, index: &Value) -> Result<Value, Value> {
        match (target, index) {
            (Value::List(l), Value::Int(i)) => {
                let l = l.borrow();
                let (len, ii) = norm_index(l.len(), *i);
                if ii < 0 || ii as usize >= len { return Err(rt_err("list index out of range")); }
                Ok(l[ii as usize].clone())
            }
            (Value::Str(s), Value::Int(i)) => {
                let chars: Vec<char> = s.chars().collect();
                let (len, ii) = norm_index(chars.len(), *i);
                if ii < 0 || ii as usize >= len { return Err(rt_err("string index out of range")); }
                Ok(Value::Str(Rc::new(chars[ii as usize].to_string())))
            }
            (Value::Dict(d), Value::Str(k)) => d.borrow().get(k.as_str()).cloned().ok_or_else(|| rt_err(format!("key '{}' not found", k))),
            _ => Err(rt_err(format!("value of type {} is not indexable", type_name(target)))),
        }
    }

    fn set_index(&self, target: &Value, index: &Value, value: Value) -> Result<(), Value> {
        match (target, index) {
            (Value::List(l), Value::Int(i)) => {
                let mut l = l.borrow_mut();
                let (len, ii) = norm_index(l.len(), *i);
                if ii < 0 || ii as usize >= len { return Err(rt_err("list index out of range")); }
                l[ii as usize] = value;
                Ok(())
            }
            (Value::Dict(d), Value::Str(k)) => { d.borrow_mut().insert((**k).clone(), value); Ok(()) }
            _ => Err(rt_err("invalid index assignment target")),
        }
    }

    fn call_value(&mut self, callee: Value, args: Vec<Value>) -> Result<Value, Value> {
        match callee {
            Value::Closure(c) => self.invoke_closure(&c, Value::Null, args),
            Value::BoundMethod(receiver, c) => self.invoke_closure(&c, *receiver, args),
            Value::Native(name) => self.call_native(name, args),
            other => Err(rt_err(format!("value of type {} is not callable", type_name(&other)))),
        }
    }

    fn invoke_closure(&mut self, c: &Rc<ClosureObj>, receiver: Value, args: Vec<Value>) -> Result<Value, Value> {
        let mut locals: Vec<Cell> = Vec::with_capacity(c.function.local_count);
        locals.push(Rc::new(RefCell::new(receiver)));
        for i in 0..c.function.arity {
            locals.push(Rc::new(RefCell::new(args.get(i).cloned().unwrap_or(Value::Null))));
        }
        // Slots beyond the parameters belong to `let`/for-loop/catch bindings
        // declared in the body; they start Null and get set when that
        // binding's bytecode actually runs.
        while locals.len() < c.function.local_count {
            locals.push(Rc::new(RefCell::new(Value::Null)));
        }
        self.run_frame(c.clone(), locals)
    }

    fn invoke(&mut self, target: Value, name: &str, args: Vec<Value>) -> Result<Value, Value> {
        match &target {
            Value::Instance(inst) => {
                let class = inst.borrow().class.clone();
                match find_method(&class, name) {
                    Some(m) => self.invoke_closure(&m, target.clone(), args),
                    None => Err(rt_err(format!("no method '{}' on class {}", name, class.name))),
                }
            }
            Value::List(l) => list_method(l, name, args),
            Value::Dict(d) => dict_method(d, name, args),
            Value::Str(s) => str_method(s, name, args),
            _ => Err(rt_err(format!("cannot call method '{}' on {}", name, type_name(&target)))),
        }
    }

    fn call_native(&mut self, name: &str, args: Vec<Value>) -> Result<Value, Value> {
        match name {
            "print" => {
                let line: Vec<String> = args.iter().map(to_display).collect();
                self.output.push_str(&line.join(" "));
                self.output.push('\n');
                Ok(Value::Null)
            }
            "len" => match args.get(0) {
                Some(Value::List(l)) => Ok(Value::Int(l.borrow().len() as i64)),
                Some(Value::Dict(d)) => Ok(Value::Int(d.borrow().len() as i64)),
                Some(Value::Str(s)) => Ok(Value::Int(s.chars().count() as i64)),
                _ => Err(rt_err("len() requires a list, dict, or string")),
            },
            "str" => Ok(Value::Str(Rc::new(to_display(args.get(0).unwrap_or(&Value::Null))))),
            "int" => match args.get(0) {
                Some(Value::Int(i)) => Ok(Value::Int(*i)),
                Some(Value::Float(f)) => Ok(Value::Int(*f as i64)),
                Some(Value::Bool(b)) => Ok(Value::Int(if *b { 1 } else { 0 })),
                Some(Value::Str(s)) => s.trim().parse::<i64>().map(Value::Int).map_err(|_| rt_err(format!("cannot convert '{}' to int", s))),
                _ => Err(rt_err("int() requires a number, bool, or string")),
            },
            "float" => match args.get(0) { Some(v) => Ok(Value::Float(num(v)?)), None => Err(rt_err("float() requires an argument")) },
            "bool" => Ok(Value::Bool(truthy(args.get(0).unwrap_or(&Value::Null)))),
            "type" => Ok(Value::Str(Rc::new(type_name(args.get(0).unwrap_or(&Value::Null)).to_string()))),
            "range" => {
                let (start, end, step) = match args.len() {
                    1 => (0i64, as_int(&args[0])?, 1i64),
                    2 => (as_int(&args[0])?, as_int(&args[1])?, 1i64),
                    3 => (as_int(&args[0])?, as_int(&args[1])?, as_int(&args[2])?),
                    _ => return Err(rt_err("range() takes 1 to 3 arguments")),
                };
                if step == 0 { return Err(rt_err("range() step cannot be 0")); }
                let mut v = Vec::new();
                if step > 0 { let mut i = start; while i < end { v.push(Value::Int(i)); i += step; } }
                else { let mut i = start; while i > end { v.push(Value::Int(i)); i += step; } }
                Ok(Value::List(Rc::new(RefCell::new(v))))
            }
            "abs" => Ok(Value::Float(num(args.get(0).unwrap_or(&Value::Null))?.abs())),
            "min" => reduce_numeric(&args, f64::min),
            "max" => reduce_numeric(&args, f64::max),
            "sum" => match args.get(0) { Some(Value::List(l)) => { let mut t = 0.0; for v in l.borrow().iter() { t += num(v)?; } Ok(Value::Float(t)) } _ => Err(rt_err("sum() requires a list")) },
            "sorted" => match args.get(0) {
                Some(Value::List(l)) => { let mut v = l.borrow().clone(); v.sort_by(|a, b| num(a).unwrap_or(0.0).partial_cmp(&num(b).unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)); Ok(Value::List(Rc::new(RefCell::new(v)))) }
                _ => Err(rt_err("sorted() requires a list")),
            },
            "round" => Ok(Value::Int(num(args.get(0).unwrap_or(&Value::Null))?.round() as i64)),
            "input" => Ok(Value::Str(Rc::new(String::new()))),
            "relu" => Ok(map_elementwise(args.get(0).unwrap_or(&Value::Null), |x| x.max(0.0))),
            "sigmoid" => Ok(map_elementwise(args.get(0).unwrap_or(&Value::Null), |x| 1.0 / (1.0 + (-x).exp()))),
            "tanh" => Ok(map_elementwise(args.get(0).unwrap_or(&Value::Null), |x| x.tanh())),
            "softmax" => softmax(args.get(0).unwrap_or(&Value::Null)),
            "transpose" => transpose(args.get(0).unwrap_or(&Value::Null)),
            "flatten" => Ok(Value::List(Rc::new(RefCell::new(flatten(args.get(0).unwrap_or(&Value::Null)))))),
            _ => Err(rt_err(format!("unknown builtin '{}'", name))),
        }
    }
}

fn str_const(constants: &[Value], idx: u16) -> &str { match &constants[idx as usize] { Value::Str(s) => s, _ => unreachable!("compiler always stores names as string constants") } }
fn rt_err(msg: impl Into<String>) -> Value { Value::Str(Rc::new(msg.into())) }
fn norm_index(len: usize, i: i64) -> (usize, i64) { let ii = if i < 0 { len as i64 + i } else { i }; (len, ii) }

fn num(v: &Value) -> Result<f64, Value> {
    match v {
        Value::Int(i) => Ok(*i as f64),
        Value::Float(f) => Ok(*f),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        other => Err(rt_err(format!("expected a number, got {}", type_name(other)))),
    }
}
fn as_int(v: &Value) -> Result<i64, Value> { match v { Value::Int(i) => Ok(*i), Value::Float(f) => Ok(*f as i64), other => Err(rt_err(format!("expected an integer, got {}", type_name(other)))) } }

fn arith(l: &Value, r: &Value, fi: fn(i64, i64) -> i64, ff: fn(f64, f64) -> f64) -> Result<Value, Value> {
    match (l, r) { (Value::Int(a), Value::Int(b)) => Ok(Value::Int(fi(*a, *b))), _ => Ok(Value::Float(ff(num(l)?, num(r)?))) }
}

fn add_values(l: &Value, r: &Value) -> Result<Value, Value> {
    match (l, r) {
        (Value::Str(a), _) => Ok(Value::Str(Rc::new(format!("{}{}", a, to_display(r))))),
        (_, Value::Str(b)) => Ok(Value::Str(Rc::new(format!("{}{}", to_display(l), b)))),
        (Value::List(a), Value::List(b)) => { let mut v = a.borrow().clone(); v.extend(b.borrow().clone()); Ok(Value::List(Rc::new(RefCell::new(v)))) }
        _ => arith(l, r, i64::wrapping_add, |a, b| a + b),
    }
}

fn list_method(l: &Rid<Vec<Value>>, name: &str, args: Vec<Value>) -> Result<Value, Value> {
    match name {
        "append" | "push" => { l.borrow_mut().push(args.into_iter().next().unwrap_or(Value::Null)); Ok(Value::Null) }
        "pop" => Ok(l.borrow_mut().pop().unwrap_or(Value::Null)),
        "sort" => { l.borrow_mut().sort_by(|a, b| num(a).unwrap_or(0.0).partial_cmp(&num(b).unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)); Ok(Value::Null) }
        "reverse" => { l.borrow_mut().reverse(); Ok(Value::Null) }
        "contains" => { let t = args.into_iter().next().unwrap_or(Value::Null); Ok(Value::Bool(l.borrow().iter().any(|v| value_eq(v, &t)))) }
        _ => Err(rt_err(format!("no list method '{}'", name))),
    }
}
fn dict_method(d: &Rid<HashMap<String, Value>>, name: &str, args: Vec<Value>) -> Result<Value, Value> {
    match name {
        "keys" => Ok(Value::List(Rc::new(RefCell::new(d.borrow().keys().map(|k| Value::Str(Rc::new(k.clone()))).collect())))),
        "values" => Ok(Value::List(Rc::new(RefCell::new(d.borrow().values().cloned().collect())))),
        "get" => { let key = match args.get(0) { Some(Value::Str(s)) => (**s).clone(), _ => return Err(rt_err("get() requires a string key")) }; Ok(d.borrow().get(&key).cloned().unwrap_or_else(|| args.get(1).cloned().unwrap_or(Value::Null))) }
        "remove" => { let key = match args.get(0) { Some(Value::Str(s)) => (**s).clone(), _ => return Err(rt_err("remove() requires a string key")) }; Ok(d.borrow_mut().remove(&key).unwrap_or(Value::Null)) }
        _ => Err(rt_err(format!("no dict method '{}'", name))),
    }
}
fn str_method(s: &str, name: &str, args: Vec<Value>) -> Result<Value, Value> {
    match name {
        "upper" => Ok(Value::Str(Rc::new(s.to_uppercase()))),
        "lower" => Ok(Value::Str(Rc::new(s.to_lowercase()))),
        "trim" => Ok(Value::Str(Rc::new(s.trim().to_string()))),
        "split" => { let sep = match args.get(0) { Some(Value::Str(x)) => (**x).clone(), _ => " ".to_string() }; Ok(Value::List(Rc::new(RefCell::new(s.split(sep.as_str()).map(|p| Value::Str(Rc::new(p.to_string()))).collect())))) }
        "replace" => { let a = match args.get(0) { Some(Value::Str(x)) => (**x).clone(), _ => return Err(rt_err("replace() requires string arguments")) }; let b = match args.get(1) { Some(Value::Str(x)) => (**x).clone(), _ => String::new() }; Ok(Value::Str(Rc::new(s.replace(a.as_str(), b.as_str())))) }
        "contains" => Ok(Value::Bool(matches!(args.get(0), Some(Value::Str(a)) if s.contains(a.as_str())))),
        "startsWith" => Ok(Value::Bool(matches!(args.get(0), Some(Value::Str(a)) if s.starts_with(a.as_str())))),
        "endsWith" => Ok(Value::Bool(matches!(args.get(0), Some(Value::Str(a)) if s.ends_with(a.as_str())))),
        _ => Err(rt_err(format!("no string method '{}'", name))),
    }
}

fn as_matrix(v: &Value) -> Option<Vec<Vec<f64>>> {
    if let Value::List(rows) = v {
        let rows = rows.borrow();
        let mut m = Vec::new();
        for row in rows.iter() {
            if let Value::List(cols) = row { let cols = cols.borrow(); let mut r = Vec::with_capacity(cols.len()); for c in cols.iter() { r.push(num(c).ok()?); } m.push(r); } else { return None; }
        }
        Some(m)
    } else { None }
}
fn matrix_to_value(m: Vec<Vec<f64>>) -> Value { Value::List(Rc::new(RefCell::new(m.into_iter().map(|row| Value::List(Rc::new(RefCell::new(row.into_iter().map(Value::Float).collect())))).collect()))) }
fn matmul(l: &Value, r: &Value) -> Result<Value, Value> {
    let a = as_matrix(l).ok_or_else(|| rt_err("'@' requires two matrices (lists of lists of numbers)"))?;
    let b = as_matrix(r).ok_or_else(|| rt_err("'@' requires two matrices (lists of lists of numbers)"))?;
    if a.is_empty() || b.is_empty() || a[0].len() != b.len() { return Err(rt_err("matrix dimension mismatch for '@' (inner dimensions must match)")); }
    let mut result = vec![vec![0.0; b[0].len()]; a.len()];
    for i in 0..a.len() { for j in 0..b[0].len() { for k in 0..a[0].len() { result[i][j] += a[i][k] * b[k][j]; } } }
    Ok(matrix_to_value(result))
}
fn transpose(v: &Value) -> Result<Value, Value> {
    let m = as_matrix(v).ok_or_else(|| rt_err("transpose() requires a matrix (list of lists)"))?;
    if m.is_empty() { return Ok(matrix_to_value(m)); }
    let (rows, cols) = (m.len(), m[0].len());
    let mut t = vec![vec![0.0; rows]; cols];
    for i in 0..rows { for j in 0..cols { t[j][i] = m[i][j]; } }
    Ok(matrix_to_value(t))
}
fn map_elementwise(v: &Value, f: fn(f64) -> f64) -> Value {
    match v {
        Value::List(l) => Value::List(Rc::new(RefCell::new(l.borrow().iter().map(|x| map_elementwise(x, f)).collect()))),
        Value::Int(i) => Value::Float(f(*i as f64)),
        Value::Float(x) => Value::Float(f(*x)),
        other => other.clone(),
    }
}
fn softmax(v: &Value) -> Result<Value, Value> {
    if let Value::List(l) = v {
        let nums: Vec<f64> = l.borrow().iter().map(num).collect::<Result<_, _>>()?;
        let max = nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let exps: Vec<f64> = nums.iter().map(|x| (x - max).exp()).collect();
        let sum: f64 = exps.iter().sum();
        Ok(Value::List(Rc::new(RefCell::new(exps.into_iter().map(|x| Value::Float(x / sum)).collect()))))
    } else { Err(rt_err("softmax() requires a list of numbers")) }
}
fn flatten(v: &Value) -> Vec<Value> { match v { Value::List(l) => l.borrow().iter().flat_map(flatten).collect(), other => vec![other.clone()] } }
fn reduce_numeric(args: &[Value], f: fn(f64, f64) -> f64) -> Result<Value, Value> {
    let nums: Vec<f64> = if args.len() == 1 {
        if let Value::List(l) = &args[0] { l.borrow().iter().map(|v| num(v).unwrap_or(0.0)).collect() } else { vec![num(&args[0])?] }
    } else { let mut v = Vec::new(); for a in args { v.push(num(a)?); } v };
    if nums.is_empty() { return Err(rt_err("min()/max() require at least one value")); }
    let mut acc = nums[0];
    for n in &nums[1..] { acc = f(acc, *n); }
    Ok(Value::Float(acc))
}

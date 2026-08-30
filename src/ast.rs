#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone)]
pub enum InterpPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Null,
    Interp(Vec<InterpPart>),
    List(Vec<Expr>),
    Dict(Vec<(Expr, Expr)>),
    Ident(String),
    Unary { op: String, expr: Box<Expr> },
    Binary { op: String, left: Box<Expr>, right: Box<Expr> },
    Logical { op: String, left: Box<Expr>, right: Box<Expr> },
    Assign { name: String, value: Box<Expr> },
    IndexAssign { target: Box<Expr>, index: Box<Expr>, value: Box<Expr> },
    FieldAssign { target: Box<Expr>, field: String, value: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    Index { target: Box<Expr>, index: Box<Expr> },
    Field { target: Box<Expr>, name: String },
    New { class: String, args: Vec<Expr> },
    FuncExpr { params: Vec<Param>, body: Vec<Stmt> },
}

#[derive(Debug, Clone)]
pub enum Stmt {
    ExprStmt(Expr),
    Let { name: String, value: Expr },
    Const { name: String, value: Expr },
    If { branches: Vec<(Expr, Vec<Stmt>)>, else_branch: Option<Vec<Stmt>> },
    While { cond: Expr, body: Vec<Stmt> },
    For { var: String, iter: Expr, body: Vec<Stmt> },
    FuncDecl { name: String, params: Vec<Param>, body: Vec<Stmt> },
    ClassDecl { name: String, parent: Option<String>, methods: Vec<Stmt> },
    Return(Option<Expr>),
    Break,
    Continue,
    TryCatch { try_block: Vec<Stmt>, err_name: String, catch_block: Vec<Stmt> },
    Throw(Expr),
    Import(String),
}

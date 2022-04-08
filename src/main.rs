use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::io::Write;
use std::io::{stdin, stdout};

#[macro_use]
mod lexer;

use lexer::*;

#[derive(Debug, Copy, Clone, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

impl Op {
    const MAX_PRECEDENCE: usize = 2;

    fn from_token_kind(kind: TokenKind) -> Option<Self> {
        match kind {
            TokenKind::Plus => Some(Op::Add),
            TokenKind::Dash => Some(Op::Sub),
            TokenKind::Asterisk => Some(Op::Mul),
            TokenKind::Slash => Some(Op::Div),
            TokenKind::Caret => Some(Op::Pow),
            _ => None,
        }
    }

    fn precedence(&self) -> usize {
        use Op::*;
        match self {
            Add | Sub => 0,
            Mul | Div => 1,
            Pow => 2,
        }
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Op::Add => write!(f, "+"),
            Op::Sub => write!(f, "-"),
            Op::Mul => write!(f, "*"),
            Op::Div => write!(f, "/"),
            Op::Pow => write!(f, "^"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Sym(String),
    Var(String),
    Fun(Box<Expr>, Vec<Expr>),
    Op(Op, Box<Expr>, Box<Expr>),
}

#[derive(Debug)]
/// An error that happens during parsing the Noq source code
enum SyntaxError {
    ExpectedToken(TokenKind, Token),
    ExpectedPrimary(Token),
    ExpectedAppliedRule(Token),
    ExpectedCommand(Token),
}

#[derive(Debug)]
/// An error that happens during execution of the Noq source code
enum RuntimeError {
    RuleAlreadyExists(String, Loc, Option<Loc>),
    RuleDoesNotExist(String, Loc),
    AlreadyShaping(Loc),
    NoShapingInPlace(Loc),
    NoHistory(Loc),
    UnknownStrategy(String, Loc),
    IrreversibleRule(Loc),
    StrategyIsNotSym(Expr, Loc),
}

#[derive(Debug)]
enum Error {
    Runtime(RuntimeError),
    Syntax(SyntaxError),
}

impl From<SyntaxError> for Error {
    fn from(err: SyntaxError) -> Self {
        Self::Syntax(err)
    }
}

impl From<RuntimeError> for Error {
    fn from(err: RuntimeError) -> Self {
        Self::Runtime(err)
    }
}

fn var_or_sym_based_on_name(name: &str) -> Expr {
    let x = name
        .chars()
        .next()
        .expect("Empty names are not allowed. This might be a bug in the lexer.");
    if x.is_uppercase() || x == '_' {
        Expr::Var(name.to_string())
    } else {
        Expr::Sym(name.to_string())
    }
}

enum AppliedRule {
    ByName {
        loc: Loc,
        name: String,
        reversed: bool,
    },
    Anonymous {
        loc: Loc,
        head: Expr,
        body: Expr,
    },
}

impl AppliedRule {
    fn reversed(self) -> Self {
        match self {
            Self::ByName {
                loc,
                reversed,
                name,
            } => Self::ByName {
                loc,
                reversed: !reversed,
                name,
            },
            Self::Anonymous { loc, head, body } => Self::Anonymous {
                loc,
                head: body,
                body: head,
            },
        }
    }

    fn parse(lexer: &mut Lexer<impl Iterator<Item = char>>) -> Result<Self, SyntaxError> {
        let token = lexer.next_token();
        match token.kind {
            TokenKind::Reverse => Ok(Self::parse(lexer)?.reversed()),
            TokenKind::Rule => {
                let head = Expr::parse(lexer)?;
                expect_token_kind(lexer, TokenKind::Equals)?;
                let body = Expr::parse(lexer)?;
                Ok(AppliedRule::Anonymous {
                    loc: token.loc,
                    head,
                    body,
                })
            }
            TokenKind::Ident => Ok(AppliedRule::ByName {
                loc: token.loc,
                name: token.text,
                reversed: false,
            }),
            _ => Err(SyntaxError::ExpectedAppliedRule(token)),
        }
    }
}

impl Expr {
    pub fn human_name(&self) -> &'static str {
        match self {
            Self::Sym(_) => "a symbol",
            Self::Var(_) => "a variable",
            Self::Fun(_, _) => "a functor",
            Self::Op(_, _, _) => "a binary operator",
        }
    }

    fn parse_fun_args(
        lexer: &mut Lexer<impl Iterator<Item = char>>,
    ) -> Result<Vec<Self>, SyntaxError> {
        use TokenKind::*;
        let mut args = Vec::new();
        expect_token_kind(lexer, OpenParen)?;
        if lexer.peek_token().kind == CloseParen {
            lexer.next_token();
            return Ok(args);
        }
        args.push(Self::parse(lexer)?);
        while lexer.peek_token().kind == Comma {
            lexer.next_token();
            args.push(Self::parse(lexer)?);
        }
        let close_paren = lexer.next_token();
        if close_paren.kind == CloseParen {
            Ok(args)
        } else {
            Err(SyntaxError::ExpectedToken(CloseParen, close_paren))
        }
    }

    fn parse_fun_or_var_or_sym(
        lexer: &mut Lexer<impl Iterator<Item = char>>,
    ) -> Result<Self, SyntaxError> {
        let mut head = {
            let token = lexer.peek_token().clone();
            match token.kind {
                TokenKind::OpenParen => {
                    lexer.next_token();
                    let result = Self::parse(lexer)?;
                    expect_token_kind(lexer, TokenKind::CloseParen)?;
                    result
                }

                TokenKind::Ident => {
                    lexer.next_token();
                    var_or_sym_based_on_name(&token.text)
                }

                _ => return Err(SyntaxError::ExpectedPrimary(token)),
            }
        };

        while lexer.peek_token().kind == TokenKind::OpenParen {
            head = Expr::Fun(Box::new(head), Self::parse_fun_args(lexer)?)
        }
        Ok(head)
    }

    fn parse_binary_operator(
        lexer: &mut Lexer<impl Iterator<Item = char>>,
        current_precedence: usize,
    ) -> Result<Self, SyntaxError> {
        if current_precedence > Op::MAX_PRECEDENCE {
            return Self::parse_fun_or_var_or_sym(lexer);
        }

        let mut result = Self::parse_binary_operator(lexer, current_precedence + 1)?;

        while let Some(op) = Op::from_token_kind(lexer.peek_token().kind) {
            if current_precedence != op.precedence() {
                break;
            }

            lexer.next_token();

            result = Expr::Op(
                op,
                Box::new(result),
                Box::new(Self::parse_binary_operator(lexer, current_precedence)?),
            );
        }

        Ok(result)
    }

    pub fn parse(lexer: &mut Lexer<impl Iterator<Item = char>>) -> Result<Self, SyntaxError> {
        Self::parse_binary_operator(lexer, 0)
    }
}

#[allow(unused_macros)]
macro_rules! fun_args {
    () => { vec![] };
    ($name:ident) => { vec![expr!($name)] };
    ($name:ident,$($rest:tt)*) => {
        {
            let mut t = vec![expr!($name)];
            t.append(&mut fun_args!($($rest)*));
            t
        }
    };
    ($name:ident($($args:tt)*)) => {
        vec![expr!($name($($args)*))]
    };
    ($name:ident($($args:tt)*),$($rest:tt)*) => {
        {
            let mut t = vec![expr!($name($($args)*))];
            t.append(&mut fun_args!($($rest)*));
            t
        }
    }
}

#[allow(unused_macros)]
macro_rules! expr {
    ($name:ident) => {
        var_or_sym_based_on_name(stringify!($name))
    };
    ($name:ident($($args:tt)*)) => {
        Expr::Fun(Box::new(var_or_sym_based_on_name(stringify!($name))), fun_args!($($args)*))
    };
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Expr::Sym(name) | Expr::Var(name) => write!(f, "{}", name),
            Expr::Fun(head, args) => {
                match &**head {
                    Expr::Sym(name) | Expr::Var(name) => write!(f, "{}", name)?,
                    other => write!(f, "({})", other)?,
                }
                write!(f, "(")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            Expr::Op(op, lhs, rhs) => {
                match **lhs {
                    Expr::Op(sub_op, _, _) => {
                        if sub_op.precedence() <= op.precedence() {
                            write!(f, "({})", lhs)?
                        } else {
                            write!(f, "{}", lhs)?
                        }
                    }
                    _ => write!(f, "{}", lhs)?,
                }
                if op.precedence() == 0 {
                    write!(f, " {} ", op)?;
                } else {
                    write!(f, "{}", op)?;
                }
                match **rhs {
                    Expr::Op(sub_op, _, _) => {
                        if sub_op.precedence() <= op.precedence() {
                            write!(f, "({})", rhs)
                        } else {
                            write!(f, "{}", rhs)
                        }
                    }
                    _ => write!(f, "{}", rhs),
                }
            }
        }
    }
}

enum Action {
    Skip,
    Apply,
}

enum State {
    /// Stop the current recursion branch and try other braunches
    Bail,
    /// Continue applying the rule to the result of the application
    Cont,
    /// Completely stop the application process
    Halt,
}

struct Resolution {
    action: Action,
    state: State,
}

#[derive(Debug, Clone)]
enum Rule {
    User { loc: Loc, head: Expr, body: Expr },
    Replace,
}

enum Strategy {
    All,
    Deep,
    Nth { current: usize, target: usize },
}

impl Strategy {
    fn by_name(name: &str) -> Option<Self> {
        match name {
            "all" => Some(Self::All),
            "first" => Some(Self::nth(0)),
            "deep" => Some(Self::Deep),
            x => x.parse().map(Self::nth).ok(),
        }
    }

    fn nth(target: usize) -> Self {
        Self::Nth { current: 0, target }
    }

    fn matched(&mut self) -> Resolution {
        match self {
            Self::All => Resolution {
                action: Action::Apply,
                state: State::Bail,
            },

            Self::Deep => Resolution {
                action: Action::Apply,
                state: State::Cont,
            },

            Self::Nth { current, target } => {
                if current == target {
                    Resolution {
                        action: Action::Apply,
                        state: State::Halt,
                    }
                } else if current > target {
                    Resolution {
                        action: Action::Skip,
                        state: State::Halt,
                    }
                } else {
                    *current += 1;
                    Resolution {
                        action: Action::Skip,
                        state: State::Cont,
                    }
                }
            }
        }
    }
}

impl Rule {
    fn apply(
        &self,
        expr: &Expr,
        strategy: &mut Strategy,
        apply_command_loc: &Loc,
    ) -> Result<Expr, RuntimeError> {
        fn apply_to_subexprs(
            rule: &Rule,
            expr: &Expr,
            strategy: &mut Strategy,
            apply_command_loc: &Loc,
        ) -> Result<(Expr, bool), RuntimeError> {
            use Expr::*;
            match expr {
                Sym(_) | Var(_) => Ok((expr.clone(), false)),
                Op(op, lhs, rhs) => {
                    let (new_lhs, halt) = apply_impl(rule, lhs, strategy, apply_command_loc)?;
                    if halt {
                        return Ok((Op(*op, Box::new(new_lhs), rhs.clone()), true));
                    }
                    let (new_rhs, halt) = apply_impl(rule, rhs, strategy, apply_command_loc)?;
                    Ok((Op(*op, Box::new(new_lhs), Box::new(new_rhs)), halt))
                }
                Fun(head, args) => {
                    let (new_head, halt) = apply_impl(rule, head, strategy, apply_command_loc)?;
                    if halt {
                        Ok((Fun(Box::new(new_head), args.clone()), true))
                    } else {
                        let mut new_args = Vec::<Expr>::new();
                        let mut halt_args = false;
                        for arg in args {
                            if halt_args {
                                new_args.push(arg.clone())
                            } else {
                                let (new_arg, halt) =
                                    apply_impl(rule, arg, strategy, apply_command_loc)?;
                                new_args.push(new_arg);
                                halt_args = halt;
                            }
                        }
                        Ok((Fun(Box::new(new_head), new_args), false))
                    }
                }
            }
        }

        fn apply_impl(
            rule: &Rule,
            expr: &Expr,
            strategy: &mut Strategy,
            apply_command_loc: &Loc,
        ) -> Result<(Expr, bool), RuntimeError> {
            match rule {
                Rule::User { loc: _, head, body } => {
                    if let Some(bindings) = pattern_match(head, expr) {
                        let resolution = strategy.matched();
                        let new_expr = match resolution.action {
                            Action::Apply => substitute_bindings(&bindings, body),
                            Action::Skip => expr.clone(),
                        };
                        match resolution.state {
                            State::Bail => Ok((new_expr, false)),
                            State::Cont => {
                                apply_to_subexprs(rule, &new_expr, strategy, apply_command_loc)
                            }
                            State::Halt => Ok((new_expr, true)),
                        }
                    } else {
                        apply_to_subexprs(rule, expr, strategy, apply_command_loc)
                    }
                }

                Rule::Replace => {
                    if let Some(bindings) =
                        pattern_match(&expr!(apply_rule(Strategy, Head, Body, Expr)), expr)
                    {
                        let meta_rule = Rule::User {
                            loc: loc_here!(),
                            head: bindings
                                .get("Head")
                                .expect("Variable `Head` is present in the meta pattern")
                                .clone(),
                            body: bindings
                                .get("Body")
                                .expect("Variable `Body` is present in the meta pattern")
                                .clone(),
                        };
                        let meta_strategy = bindings
                            .get("Strategy")
                            .expect("Variable `Strategy` is present in the meta pattern");
                        if let Expr::Sym(meta_strategy_name) = meta_strategy {
                            let meta_expr = bindings
                                .get("Expr")
                                .expect("Variable `Expr` is present in the meta pattern");
                            let result = match Strategy::by_name(meta_strategy_name) {
                                Some(mut strategy) => {
                                    meta_rule.apply(meta_expr, &mut strategy, apply_command_loc)
                                }
                                None => Err(RuntimeError::UnknownStrategy(
                                    meta_strategy_name.to_string(),
                                    apply_command_loc.clone(),
                                )),
                            };
                            Ok((result?, false))
                        } else {
                            Err(RuntimeError::StrategyIsNotSym(
                                meta_strategy.clone(),
                                apply_command_loc.clone(),
                            ))
                        }
                    } else {
                        apply_to_subexprs(rule, expr, strategy, apply_command_loc)
                    }
                }
            }
        }
        Ok((apply_impl(self, expr, strategy, apply_command_loc)?).0)
    }
}

fn substitute_bindings(bindings: &Bindings, expr: &Expr) -> Expr {
    use Expr::*;
    match expr {
        Sym(_) => expr.clone(),

        Var(name) => {
            if let Some(value) = bindings.get(name) {
                value.clone()
            } else {
                expr.clone()
            }
        }

        Op(op, lhs, rhs) => Op(
            *op,
            Box::new(substitute_bindings(bindings, lhs)),
            Box::new(substitute_bindings(bindings, rhs)),
        ),

        Fun(head, args) => {
            let new_head = substitute_bindings(bindings, head);
            let mut new_args = Vec::new();
            for arg in args {
                new_args.push(substitute_bindings(bindings, arg))
            }
            Fun(Box::new(new_head), new_args)
        }
    }
}

fn expect_token_kind(
    lexer: &mut Lexer<impl Iterator<Item = char>>,
    kind: TokenKind,
) -> Result<Token, SyntaxError> {
    let token = lexer.next_token();
    if kind == token.kind {
        Ok(token)
    } else {
        Err(SyntaxError::ExpectedToken(kind, token))
    }
}

type Bindings = HashMap<String, Expr>;

fn pattern_match(pattern: &Expr, value: &Expr) -> Option<Bindings> {
    fn pattern_match_impl(pattern: &Expr, value: &Expr, bindings: &mut Bindings) -> bool {
        use Expr::*;
        match (pattern, value) {
            (Sym(name1), Sym(name2)) => name1 == name2,
            (Var(name), _) => {
                if name == "_" {
                    true
                } else if let Some(bound_value) = bindings.get(name) {
                    bound_value == value
                } else {
                    bindings.insert(name.clone(), value.clone());
                    true
                }
            }
            (Op(op1, lhs1, rhs1), Op(op2, lhs2, rhs2)) => {
                *op1 == *op2
                    && pattern_match_impl(lhs1, lhs2, bindings)
                    && pattern_match_impl(rhs1, rhs2, bindings)
            }
            (Fun(name1, args1), Fun(name2, args2)) => {
                if pattern_match_impl(name1, name2, bindings) && args1.len() == args2.len() {
                    for i in 0..args1.len() {
                        if !pattern_match_impl(&args1[i], &args2[i], bindings) {
                            return false;
                        }
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    let mut bindings = HashMap::new();

    if pattern_match_impl(pattern, value, &mut bindings) {
        Some(bindings)
    } else {
        None
    }
}

enum Command {
    DefineRule(Loc, String, Rule),
    StartShaping(Loc, Expr),
    ApplyRule {
        loc: Loc,
        strategy_name: String,
        applied_rule: AppliedRule,
    },
    FinishShaping(Loc),
    UndoRule(Loc),
    Quit,
    DeleteRule(Loc, String),
}

impl Command {
    fn parse(lexer: &mut Lexer<impl Iterator<Item = char>>) -> Result<Command, SyntaxError> {
        let keyword = lexer.next_token();
        match keyword.kind {
            TokenKind::Rule => {
                let name = expect_token_kind(lexer, TokenKind::Ident)?;
                let head = Expr::parse(lexer)?;
                expect_token_kind(lexer, TokenKind::Equals)?;
                let body = Expr::parse(lexer)?;
                Ok(Command::DefineRule(
                    keyword.loc.clone(),
                    name.text,
                    Rule::User {
                        loc: keyword.loc,
                        head,
                        body,
                    },
                ))
            }
            TokenKind::Shape => Ok(Command::StartShaping(keyword.loc, Expr::parse(lexer)?)),
            TokenKind::Apply => {
                let strategy_name = expect_token_kind(lexer, TokenKind::Ident)?.text;
                let applied_rule = AppliedRule::parse(lexer)?;
                Ok(Command::ApplyRule {
                    loc: keyword.loc,
                    strategy_name,
                    applied_rule,
                })
            }
            TokenKind::Done => Ok(Command::FinishShaping(keyword.loc)),
            TokenKind::Undo => Ok(Command::UndoRule(keyword.loc)),
            TokenKind::Quit => Ok(Command::Quit),
            TokenKind::Delete => Ok(Command::DeleteRule(
                keyword.loc,
                expect_token_kind(lexer, TokenKind::Ident)?.text,
            )),
            _ => Err(SyntaxError::ExpectedCommand(keyword)),
        }
    }
}

struct Context {
    rules: HashMap<String, Rule>,
    current_expr: Option<Expr>,
    shaping_history: Vec<Expr>,
    quit: bool,
}

impl Context {
    fn new() -> Self {
        let mut rules = HashMap::new();
        rules.insert("replace".to_string(), Rule::Replace);
        Self {
            rules,
            current_expr: None,
            shaping_history: Vec::new(),
            quit: false,
        }
    }

    fn materialize_applied_rule(&self, applied_rule: AppliedRule) -> Result<Rule, RuntimeError> {
        match applied_rule {
            AppliedRule::ByName {
                loc,
                name,
                reversed,
            } => match self.rules.get(&name) {
                Some(rule) => {
                    if reversed {
                        match rule.clone() {
                            Rule::User { loc, head, body } => Ok(Rule::User {
                                loc,
                                head: body,
                                body: head,
                            }),
                            Rule::Replace => Err(RuntimeError::IrreversibleRule(loc)),
                        }
                    } else {
                        Ok(rule.clone())
                    }
                }

                None => Err(RuntimeError::RuleDoesNotExist(name, loc)),
            },
            AppliedRule::Anonymous { loc, head, body } => Ok(Rule::User { loc, head, body }),
        }
    }

    fn process_command(&mut self, command: Command) -> Result<(), RuntimeError> {
        match command {
            Command::DefineRule(rule_loc, rule_name, rule) => {
                if let Some(existing_rule) = self.rules.get(&rule_name) {
                    let loc = match existing_rule {
                        Rule::User { loc, .. } => Some(loc),
                        Rule::Replace => None,
                    };
                    return Err(RuntimeError::RuleAlreadyExists(
                        rule_name,
                        rule_loc,
                        loc.cloned(),
                    ));
                }
                self.rules.insert(rule_name, rule);
            }
            Command::StartShaping(loc, expr) => {
                if self.current_expr.is_some() {
                    return Err(RuntimeError::AlreadyShaping(loc));
                }
                println!(" => {}", &expr);
                self.current_expr = Some(expr);
            }
            Command::ApplyRule {
                loc,
                strategy_name,
                applied_rule,
            } => {
                if let Some(expr) = &self.current_expr {
                    let rule = self.materialize_applied_rule(applied_rule)?;

                    // todo!("Throw an error if not a single match for the rule was found")
                    let new_expr = match Strategy::by_name(&strategy_name) {
                        Some(mut strategy) => rule.apply(expr, &mut strategy, &loc)?,
                        None => return Err(RuntimeError::UnknownStrategy(strategy_name, loc)),
                    };
                    println!(" => {}", &new_expr);
                    self.shaping_history.push(
                        self.current_expr
                            .replace(new_expr)
                            .expect("current_expr must have something"),
                    );
                } else {
                    return Err(RuntimeError::NoShapingInPlace(loc));
                }
            }
            Command::FinishShaping(loc) => {
                if self.current_expr.is_some() {
                    self.current_expr = None;
                    self.shaping_history.clear();
                } else {
                    return Err(RuntimeError::NoShapingInPlace(loc));
                }
            }
            Command::UndoRule(loc) => {
                if self.current_expr.is_some() {
                    if let Some(previous_expr) = self.shaping_history.pop() {
                        println!(" => {}", &previous_expr);
                        self.current_expr.replace(previous_expr);
                    } else {
                        return Err(RuntimeError::NoHistory(loc));
                    }
                } else {
                    return Err(RuntimeError::NoShapingInPlace(loc));
                }
            }
            Command::Quit => {
                self.quit = true;
            }
            Command::DeleteRule(loc, name) => {
                if self.rules.contains_key(&name) {
                    self.rules.remove(&name);
                } else {
                    return Err(RuntimeError::RuleDoesNotExist(name, loc));
                }
            }
        }
        Ok(())
    }
}

fn eprint_repl_loc_cursor(prompt: &str, loc: &Loc) {
    assert!(loc.row == 1);
    eprintln!("{:>width$}^", "", width = prompt.len() + loc.col - 1);
}

fn start_parser_debugger() {
    let prompt = "expr> ";
    let mut command = String::new();
    loop {
        command.clear();
        print!("{}", prompt);
        stdout().flush().unwrap();
        stdin().read_line(&mut command).unwrap();

        let mut lexer = Lexer::new(command.trim().chars(), None);
        if lexer.peek_token().kind != TokenKind::End {
            match Expr::parse(&mut lexer) {
                Err(err) => report_error_in_repl(&err.into(), prompt),
                Ok(expr) => {
                    println!("  Display:  {}", expr);
                    println!("  Debug:    {:?}", expr);
                    println!(
                        "  Unparsed: {:?}",
                        lexer.map(|t| t.kind).collect::<Vec<_>>()
                    );
                }
            }
        }
    }
}

fn report_error_in_repl(err: &Error, prompt: &str) {
    match err {
        Error::Syntax(SyntaxError::ExpectedToken(expected, actual)) => {
            eprint_repl_loc_cursor(prompt, &actual.loc);
            eprintln!(
                "ERROR: expected {} but got {} '{}'",
                expected, actual.kind, actual.text
            );
        }
        Error::Syntax(SyntaxError::ExpectedPrimary(token)) => {
            eprint_repl_loc_cursor(prompt, &token.loc);
            eprintln!("ERROR: expected Primary Expression (which is either functor, symbol or variable), but got {}", token.kind)
        }
        Error::Syntax(SyntaxError::ExpectedAppliedRule(token)) => {
            eprint_repl_loc_cursor(prompt, &token.loc);
            eprintln!(
                "ERROR: expected applied rule argument, but got {}",
                token.kind
            )
        }
        Error::Syntax(SyntaxError::ExpectedCommand(token)) => {
            eprint_repl_loc_cursor(prompt, &token.loc);
            eprintln!("ERROR: expected command, but got {}", token.kind)
        }
        Error::Runtime(RuntimeError::RuleAlreadyExists(name, new_loc, _old_loc)) => {
            eprint_repl_loc_cursor(prompt, new_loc);
            eprintln!("ERROR: redefinition of existing rule {}", name);
        }
        Error::Runtime(RuntimeError::AlreadyShaping(loc)) => {
            eprint_repl_loc_cursor(prompt, loc);
            eprintln!(
                "ERROR: already shaping an expression. Finish the current shaping with {} first.",
                TokenKind::Done
            );
        }
        Error::Runtime(RuntimeError::NoShapingInPlace(loc)) => {
            eprint_repl_loc_cursor(prompt, loc);
            eprintln!("ERROR: no shaping in place.");
        }
        Error::Runtime(RuntimeError::RuleDoesNotExist(name, loc)) => {
            eprint_repl_loc_cursor(prompt, loc);
            eprintln!("ERROR: rule {} does not exist", name);
        }
        Error::Runtime(RuntimeError::NoHistory(loc)) => {
            eprint_repl_loc_cursor(prompt, loc);
            eprintln!("ERROR: no history");
        }
        Error::Runtime(RuntimeError::UnknownStrategy(name, loc)) => {
            eprint_repl_loc_cursor(prompt, loc);
            eprintln!("ERROR: unknown rule application strategy '{}'", name);
        }
        Error::Runtime(RuntimeError::IrreversibleRule(loc)) => {
            eprint_repl_loc_cursor(prompt, loc);
            eprintln!("ERROR: irreversible rule");
        }
        Error::Runtime(RuntimeError::StrategyIsNotSym(expr, loc)) => {
            eprint_repl_loc_cursor(prompt, loc);
            eprintln!(
                "ERROR: strategy must be a symbol but got {} {}",
                expr.human_name(),
                &expr
            );
        }
    }
}

fn parse_and_process_command(
    context: &mut Context,
    lexer: &mut Lexer<impl Iterator<Item = char>>,
) -> Result<(), Error> {
    let command = Command::parse(lexer)?;
    context.process_command(command)?;
    Ok(())
}

fn interpret_file(file_path: &str) {
    let mut context = Context::new();
    let source = fs::read_to_string(&file_path).unwrap();
    let mut lexer = Lexer::new(source.chars(), Some(file_path.to_string()));
    while !context.quit && lexer.peek_token().kind != TokenKind::End {
        if let Err(err) = parse_and_process_command(&mut context, &mut lexer) {
            match err {
                Error::Syntax(SyntaxError::ExpectedToken(expected_kinds, actual_token)) => {
                    eprintln!(
                        "{}: ERROR: expected {} but got {} '{}'",
                        actual_token.loc, expected_kinds, actual_token.kind, actual_token.text
                    );
                }
                Error::Syntax(SyntaxError::ExpectedPrimary(token)) => {
                    eprintln!("{}: ERROR: expected Primary Expression (which is either functor, symbol or variable), but got {}", token.loc, token.kind)
                }
                Error::Syntax(SyntaxError::ExpectedAppliedRule(token)) => {
                    eprintln!(
                        "{}: ERROR: expected applied rule argument, but got {}",
                        token.loc, token.kind
                    )
                }
                Error::Syntax(SyntaxError::ExpectedCommand(token)) => {
                    eprintln!(
                        "{}: ERROR: expected command, but got {}",
                        token.loc, token.kind
                    )
                }
                Error::Runtime(RuntimeError::RuleAlreadyExists(name, new_loc, old_loc)) => {
                    eprintln!("{}: ERROR: redefinition of existing rule {}", new_loc, name);
                    if let Some(loc) = old_loc {
                        eprintln!("{}: Previous definition is located here", loc);
                    }
                }
                Error::Runtime(RuntimeError::RuleDoesNotExist(name, loc)) => {
                    eprintln!("{}: ERROR: rule {} does not exist", loc, name);
                }
                Error::Runtime(RuntimeError::AlreadyShaping(loc)) => {
                    eprintln!("{}: ERROR: already shaping an expression. Finish the current shaping with {} first.",
                              loc, TokenKind::Done);
                }
                Error::Runtime(RuntimeError::NoShapingInPlace(loc)) => {
                    eprintln!("{}: ERROR: no shaping in place.", loc);
                }
                Error::Runtime(RuntimeError::NoHistory(loc)) => {
                    eprintln!("{}: ERROR: no history", loc);
                }
                Error::Runtime(RuntimeError::UnknownStrategy(name, loc)) => {
                    eprintln!(
                        "{}: ERROR: unknown rule application strategy '{}'",
                        loc, name
                    );
                }
                Error::Runtime(RuntimeError::IrreversibleRule(loc)) => {
                    eprintln!("{}: ERROR: irreversible rule", loc);
                }
                Error::Runtime(RuntimeError::StrategyIsNotSym(expr, loc)) => {
                    eprintln!(
                        "{}: ERROR: strategy must be a symbol but got {} `{}`",
                        loc,
                        expr.human_name(),
                        &expr
                    );
                }
            }
            std::process::exit(1);
        }
    }
}

fn start_repl() {
    let mut context = Context::new();
    let mut command = String::new();

    let default_prompt = "noq> ";
    let shaping_prompt = "> ";
    let mut prompt: &str;

    while !context.quit {
        command.clear();
        if context.current_expr.is_some() {
            prompt = shaping_prompt;
        } else {
            prompt = default_prompt;
        }
        print!("{}", prompt);
        stdout().flush().unwrap();
        stdin().read_line(&mut command).unwrap();
        let mut lexer = Lexer::new(command.trim().chars(), None);
        if lexer.peek_token().kind != TokenKind::End {
            let result = parse_and_process_command(&mut context, &mut lexer)
                .and_then(|()| expect_token_kind(&mut lexer, TokenKind::End).map_err(|e| e.into()));
            if let Err(err) = result {
                report_error_in_repl(&err, prompt);
            }
        }
    }
}

#[derive(Default)]
struct Config {
    file_path: Option<String>,
    debug_parser: bool,
}

impl Config {
    fn from_iter(args: &mut impl Iterator<Item = String>) -> Self {
        args.next().expect("Program name should be always present");
        let mut config: Self = Default::default();

        for arg in args {
            match arg.as_str() {
                "--debug-parser" => config.debug_parser = true,
                other => {
                    if config.file_path.is_none() {
                        config.file_path = Some(other.to_string())
                    } else {
                        eprintln!("ERROR: file path was already provided. Interpreting several files is not supported yet");
                        std::process::exit(1)
                    }
                }
            }
        }

        config
    }
}

fn main() {
    let config = Config::from_iter(&mut env::args());

    if config.debug_parser {
        start_parser_debugger()
    } else if let Some(file_path) = config.file_path {
        interpret_file(&file_path)
    } else {
        start_repl()
    }
}

// TODO: Load rules from files
// TODO: Define shapes as rules
// TODO: Custom arbitrary operators like in Haskell
// TODO: Save session to file
// TODO: Conditional matching of rules. Some sort of ability to combine several rules into one which tries all the provided rules sequentially and pickes the one that matches

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::io::Write;
use std::io::{stdin, stdout};

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
enum Error {
    UnexpectedToken(TokenKind, Token),
    RuleAlreadyExists(String, Loc, Loc),
    RuleDoesNotExist(String, Loc),
    AlreadyShaping(Loc),
    NoShapingInPlace(Loc),
    NoHistory(Loc),
    UnknownStrategy(String, Loc),
    ExpectedFunSymVar(Token),
    ExpectedAppliedRule(Token),
    ExpectedCommand(Token),
}

impl Expr {
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

    fn parse_fun_args(lexer: &mut Lexer<impl Iterator<Item = char>>) -> Result<Vec<Self>, Error> {
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
            Err(Error::UnexpectedToken(CloseParen, close_paren))
        }
    }

    fn parse_fun_or_var_or_sym(
        lexer: &mut Lexer<impl Iterator<Item = char>>,
    ) -> Result<Self, Error> {
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
                    Self::var_or_sym_based_on_name(&token.text)
                }

                _ => return Err(Error::ExpectedFunSymVar(token)),
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
    ) -> Result<Self, Error> {
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

    pub fn parse(lexer: &mut Lexer<impl Iterator<Item = char>>) -> Result<Self, Error> {
        Self::parse_binary_operator(lexer, 0)
    }
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

trait Strategy {
    fn matched(&mut self) -> Resolution;
}

#[derive(Debug, Clone)]
struct Rule {
    loc: Loc,
    head: Expr,
    body: Expr,
}

struct ApplyAll;

impl Strategy for ApplyAll {
    fn matched(&mut self) -> Resolution {
        Resolution {
            action: Action::Apply,
            state: State::Bail,
        }
    }
}

struct ApplyDeep;

impl Strategy for ApplyDeep {
    fn matched(&mut self) -> Resolution {
        Resolution {
            action: Action::Apply,
            state: State::Cont,
        }
    }
}

struct ApplyNth {
    current: usize,
    target: usize,
}

impl ApplyNth {
    fn new(target: usize) -> Self {
        Self { current: 0, target }
    }
}

impl Strategy for ApplyNth {
    fn matched(&mut self) -> Resolution {
        if self.current == self.target {
            Resolution {
                action: Action::Apply,
                state: State::Halt,
            }
        } else if self.current > self.target {
            Resolution {
                action: Action::Skip,
                state: State::Halt,
            }
        } else {
            self.current += 1;
            Resolution {
                action: Action::Skip,
                state: State::Cont,
            }
        }
    }
}

impl Rule {
    fn apply(&self, expr: &Expr, strategy: &mut impl Strategy) -> Expr {
        fn apply_to_subexprs(
            rule: &Rule,
            expr: &Expr,
            strategy: &mut impl Strategy,
        ) -> (Expr, bool) {
            use Expr::*;
            match expr {
                Sym(_) | Var(_) => (expr.clone(), false),
                Op(op, lhs, rhs) => {
                    let (new_lhs, halt) = apply_impl(rule, lhs, strategy);
                    if halt {
                        return (Op(*op, Box::new(new_lhs), rhs.clone()), true);
                    }
                    let (new_rhs, halt) = apply_impl(rule, rhs, strategy);
                    (Op(*op, Box::new(new_lhs), Box::new(new_rhs)), halt)
                }
                Fun(head, args) => {
                    let (new_head, halt) = apply_impl(rule, head, strategy);
                    if halt {
                        (Fun(Box::new(new_head), args.clone()), true)
                    } else {
                        let mut new_args = Vec::<Expr>::new();
                        let mut halt_args = false;
                        for arg in args {
                            if halt_args {
                                new_args.push(arg.clone())
                            } else {
                                let (new_arg, halt) = apply_impl(rule, arg, strategy);
                                new_args.push(new_arg);
                                halt_args = halt;
                            }
                        }
                        (Fun(Box::new(new_head), new_args), false)
                    }
                }
            }
        }

        fn apply_impl(rule: &Rule, expr: &Expr, strategy: &mut impl Strategy) -> (Expr, bool) {
            if let Some(bindings) = pattern_match(&rule.head, expr) {
                let resolution = strategy.matched();
                let new_expr = match resolution.action {
                    Action::Apply => substitute_bindings(&bindings, &rule.body),
                    Action::Skip => expr.clone(),
                };
                match resolution.state {
                    State::Bail => (new_expr, false),
                    State::Cont => apply_to_subexprs(rule, &new_expr, strategy),
                    State::Halt => (new_expr, true),
                }
            } else {
                apply_to_subexprs(rule, expr, strategy)
            }
        }
        apply_impl(self, expr, strategy).0
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} = {}", self.head, self.body)
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
                new_args.push(substitute_bindings(bindings, &arg))
            }
            Fun(Box::new(new_head), new_args)
        }
    }
}

fn expect_token_kind(
    lexer: &mut Lexer<impl Iterator<Item = char>>,
    kind: TokenKind,
) -> Result<Token, Error> {
    let token = lexer.next_token();
    if kind == token.kind {
        Ok(token)
    } else {
        Err(Error::UnexpectedToken(kind, token))
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

#[derive(Default)]
struct Context {
    rules: HashMap<String, Rule>,
    current_expr: Option<Expr>,
    shaping_history: Vec<Expr>,
    quit: bool,
}

impl Context {
    fn parse_applied_rule(
        &self,
        lexer: &mut Lexer<impl Iterator<Item = char>>,
    ) -> Result<Rule, Error> {
        let token = lexer.next_token();
        match token.kind {
            TokenKind::Reverse => {
                let rule = self.parse_applied_rule(lexer)?;
                Ok(Rule {
                    loc: token.loc,
                    head: rule.body,
                    body: rule.head,
                })
            }

            TokenKind::Rule => {
                let head = Expr::parse(lexer)?;
                expect_token_kind(lexer, TokenKind::Equals)?;
                let body = Expr::parse(lexer)?;
                Ok(Rule {
                    loc: token.loc,
                    head,
                    body,
                })
            }

            TokenKind::Ident => {
                if let Some(rule) = self.rules.get(&token.text) {
                    Ok(rule.clone())
                } else {
                    Err(Error::RuleDoesNotExist(token.text, token.loc))
                }
            }

            _ => Err(Error::ExpectedAppliedRule(token)),
        }
    }

    fn process_command(
        &mut self,
        lexer: &mut Lexer<impl Iterator<Item = char>>,
    ) -> Result<(), Error> {
        let keyword = lexer.next_token();
        match keyword.kind {
            TokenKind::Rule => {
                let name = expect_token_kind(lexer, TokenKind::Ident)?;
                if let Some(existing_rule) = self.rules.get(&name.text) {
                    return Err(Error::RuleAlreadyExists(
                        name.text,
                        name.loc,
                        existing_rule.loc.clone(),
                    ));
                }
                let head = Expr::parse(lexer)?;
                expect_token_kind(lexer, TokenKind::Equals)?;
                let body = Expr::parse(lexer)?;
                let rule = Rule {
                    loc: keyword.loc,
                    head,
                    body,
                };
                self.rules.insert(name.text, rule);
            }
            TokenKind::Shape => {
                if let Some(_) = self.current_expr {
                    return Err(Error::AlreadyShaping(keyword.loc));
                }

                let expr = Expr::parse(lexer)?;
                println!(" => {}", &expr);
                self.current_expr = Some(expr);
            }
            TokenKind::Apply => {
                if let Some(expr) = &self.current_expr {
                    let strategy_name = expect_token_kind(lexer, TokenKind::Ident)?;

                    let rule = self.parse_applied_rule(lexer)?;
                    // todo!("Throw an error if not a single match for the rule was found")

                    let new_expr = match &strategy_name.text as &str {
                        "all" => rule.apply(&expr, &mut ApplyAll),
                        "first" => rule.apply(&expr, &mut ApplyNth::new(0)),
                        "deep" => rule.apply(&expr, &mut ApplyDeep),
                        x => match x.parse() {
                            Ok(x) => rule.apply(&expr, &mut ApplyNth::new(x)),
                            _ => {
                                return Err(Error::UnknownStrategy(
                                    strategy_name.text,
                                    strategy_name.loc,
                                ))
                            }
                        },
                    };
                    println!(" => {}", &new_expr);
                    self.shaping_history.push(
                        self.current_expr
                            .replace(new_expr)
                            .expect("current_expr must have something"),
                    );
                } else {
                    return Err(Error::NoShapingInPlace(keyword.loc));
                }
            }
            TokenKind::Done => {
                if let Some(_) = &self.current_expr {
                    self.current_expr = None;
                    self.shaping_history.clear();
                } else {
                    return Err(Error::NoShapingInPlace(keyword.loc));
                }
            }
            TokenKind::Undo => {
                if let Some(_) = &self.current_expr {
                    if let Some(previous_expr) = self.shaping_history.pop() {
                        println!(" => {}", &previous_expr);
                        self.current_expr.replace(previous_expr);
                    } else {
                        return Err(Error::NoHistory(keyword.loc));
                    }
                } else {
                    return Err(Error::NoShapingInPlace(keyword.loc));
                }
            }
            TokenKind::Quit => {
                self.quit = true;
            }
            _ => return Err(Error::ExpectedCommand(keyword)),
        }
        Ok(())
    }
}

fn eprint_repl_loc_cursor(prompt: &str, loc: &Loc) {
    assert!(loc.row == 1);
    eprintln!("{:>width$}^", "", width = prompt.len() + loc.col - 1);
}

fn main() {
    let mut args = env::args();
    args.next(); // skip program

    let mut context = Context::default();

    if let Some(file_path) = args.next() {
        let source = fs::read_to_string(&file_path).unwrap();
        let mut lexer = Lexer::new(source.chars(), Some(file_path));
        while !context.quit && lexer.peek_token().kind != TokenKind::End {
            if let Err(err) = context.process_command(&mut lexer) {
                match err {
                    Error::UnexpectedToken(expected_kinds, actual_token) => {
                        eprintln!(
                            "{}: ERROR: expected {} but got {} '{}'",
                            actual_token.loc, expected_kinds, actual_token.kind, actual_token.text
                        );
                    }
                    Error::RuleAlreadyExists(name, new_loc, old_loc) => {
                        eprintln!("{}: ERROR: redefinition of existing rule {}", new_loc, name);
                        eprintln!("{}: Previous definition is located here", old_loc);
                    }
                    Error::RuleDoesNotExist(name, loc) => {
                        eprintln!("{}: ERROR: rule {} does not exist", loc, name);
                    }
                    Error::AlreadyShaping(loc) => {
                        eprintln!("{}: ERROR: already shaping an expression. Finish the current shaping with {} first.",
                                  loc, TokenKind::Done);
                    }
                    Error::NoShapingInPlace(loc) => {
                        eprintln!("{}: ERROR: no shaping in place.", loc);
                    }
                    Error::NoHistory(loc) => {
                        eprintln!("{}: ERROR: no history", loc);
                    }
                    Error::UnknownStrategy(name, loc) => {
                        eprintln!(
                            "{}: ERROR: unknown rule application strategy '{}'",
                            loc, name
                        );
                    }
                    Error::ExpectedFunSymVar(token) => {
                        eprintln!("{}: ERROR: expected Primary Expression (which is either functor, symbol or variable), but got {}", token.loc, token.kind)
                    }
                    Error::ExpectedAppliedRule(token) => {
                        eprintln!(
                            "{}: ERROR: expected applied rule argument, but got {}",
                            token.loc, token.kind
                        )
                    }
                    Error::ExpectedCommand(token) => {
                        eprintln!(
                            "{}: ERROR: expected command, but got {}",
                            token.loc, token.kind
                        )
                    }
                }
                std::process::exit(1);
            }
        }
    } else {
        let mut command = String::new();

        let default_prompt = "noq> ";
        let shaping_prompt = "> ";
        let mut prompt: &str;

        while !context.quit {
            command.clear();
            if let Some(_) = &context.current_expr {
                prompt = shaping_prompt;
            } else {
                prompt = default_prompt;
            }
            print!("{}", prompt);
            stdout().flush().unwrap();
            stdin().read_line(&mut command).unwrap();
            let mut lexer = Lexer::new(command.trim().chars(), None);
            if lexer.peek_token().kind != TokenKind::End {
                let result = context
                    .process_command(&mut lexer)
                    .and_then(|()| expect_token_kind(&mut lexer, TokenKind::End));
                match result {
                    Err(Error::UnexpectedToken(expected, actual)) => {
                        eprint_repl_loc_cursor(prompt, &actual.loc);
                        eprintln!(
                            "ERROR: expected {} but got {} '{}'",
                            expected, actual.kind, actual.text
                        );
                    }
                    Err(Error::RuleAlreadyExists(name, new_loc, _old_loc)) => {
                        eprint_repl_loc_cursor(prompt, &new_loc);
                        eprintln!("ERROR: redefinition of existing rule {}", name);
                    }
                    Err(Error::AlreadyShaping(loc)) => {
                        eprint_repl_loc_cursor(prompt, &loc);
                        eprintln!("ERROR: already shaping an expression. Finish the current shaping with {} first.",
                                  TokenKind::Done);
                    }
                    Err(Error::NoShapingInPlace(loc)) => {
                        eprint_repl_loc_cursor(prompt, &loc);
                        eprintln!("ERROR: no shaping in place.");
                    }
                    Err(Error::RuleDoesNotExist(name, loc)) => {
                        eprint_repl_loc_cursor(prompt, &loc);
                        eprintln!("ERROR: rule {} does not exist", name);
                    }
                    Err(Error::NoHistory(loc)) => {
                        eprint_repl_loc_cursor(prompt, &loc);
                        eprintln!("ERROR: no history");
                    }
                    Err(Error::UnknownStrategy(name, loc)) => {
                        eprint_repl_loc_cursor(prompt, &loc);
                        eprintln!("ERROR: unknown rule application strategy '{}'", name);
                    }
                    Err(Error::ExpectedFunSymVar(token)) => {
                        eprint_repl_loc_cursor(prompt, &token.loc);
                        eprintln!("ERROR: expected Primary Expression (which is either functor, symbol or variable), but got {}", token.kind)
                    }
                    Err(Error::ExpectedAppliedRule(token)) => {
                        eprint_repl_loc_cursor(prompt, &token.loc);
                        eprintln!(
                            "ERROR: expected applied rule argument, but got {}",
                            token.kind
                        )
                    }
                    Err(Error::ExpectedCommand(token)) => {
                        eprint_repl_loc_cursor(prompt, &token.loc);
                        eprintln!("ERROR: expected command, but got {}", token.kind)
                    }
                    Ok(_) => {}
                }
            }
        }
    }
}

// TODO: Implement replace! macro
// TODO: Load rules from files
// TODO: Custom arbitrary operators like in Haskell
// TODO: Save session to file
// TODO: Special mode for testing the parsing of the expressions
// TODO: Conditional matching of rules. Some sort of ability to combine several rules into one which tries all the provided rules sequentially and pickes the one that matches

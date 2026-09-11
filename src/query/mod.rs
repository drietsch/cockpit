//! A small query language over the derived state:
//!
//! ```text
//! [features|tests|benchmarks where] <expr> [order by <field> [asc|desc]] [limit <n>]
//! expr  := term (('and'|'or') term)* ; term := 'not' term | '(' expr ')' | field op value
//! op    := = != ~ > >= < <= in has
//! ```
//!
//! Feature fields: `id title state confidence importance status lifecycle tag owner adr path
//! component group claim.<component> evidence.<kind> changed_since attention reasons tests
//! performance.regression visual`. Values: bare words, quoted strings, numbers, durations
//! (`7d`, `24h`), lists `(a, b)` for `in`.

use crate::model::*;
use crate::state::{BenchmarkView, FeatureView, ProjectState, TestView};
use jiff::Timestamp;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Num(f64),
    List(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Eq,
    Ne,
    Like,
    Gt,
    Ge,
    Lt,
    Le,
    In,
    Has,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Cmp { field: String, op: Op, value: Value },
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    True,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub entity: String,
    pub expr: Expr,
    pub order_by: Option<(String, bool)>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Str(String),
    Num(f64),
    Op(Op),
    LParen,
    RParen,
    Comma,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' => i += 1,
            '(' => {
                out.push(Tok::LParen);
                i += 1
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1
            }
            '"' | '\'' => {
                let q = c;
                let mut s = String::new();
                i += 1;
                while i < chars.len() && chars[i] != q {
                    s.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err("unterminated string".into());
                }
                i += 1;
                out.push(Tok::Str(s));
            }
            '=' => {
                out.push(Tok::Op(Op::Eq));
                i += 1
            }
            '~' => {
                out.push(Tok::Op(Op::Like));
                i += 1
            }
            '!' if chars.get(i + 1) == Some(&'=') => {
                out.push(Tok::Op(Op::Ne));
                i += 2
            }
            '>' if chars.get(i + 1) == Some(&'=') => {
                out.push(Tok::Op(Op::Ge));
                i += 2
            }
            '<' if chars.get(i + 1) == Some(&'=') => {
                out.push(Tok::Op(Op::Le));
                i += 2
            }
            '>' => {
                out.push(Tok::Op(Op::Gt));
                i += 1
            }
            '<' => {
                out.push(Tok::Op(Op::Lt));
                i += 1
            }
            _ if c.is_ascii_digit()
                || (c == '-' && chars.get(i + 1).map(|d| d.is_ascii_digit()).unwrap_or(false)) =>
            {
                let start = i;
                i += 1;
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_')
                {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                match s.parse::<f64>() {
                    Ok(n) => out.push(Tok::Num(n)),
                    Err(_) => out.push(Tok::Word(s)),
                }
            }
            _ if c.is_alphanumeric()
                || c == '_'
                || c == '.'
                || c == '-'
                || c == '/'
                || c == '*'
                || c == ':' =>
            {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric()
                        || matches!(chars[i], '_' | '.' | '-' | '/' | '*' | ':' | '@'))
                {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                match s.to_ascii_lowercase().as_str() {
                    "in" => out.push(Tok::Op(Op::In)),
                    "has" => out.push(Tok::Op(Op::Has)),
                    _ => out.push(Tok::Word(s)),
                }
            }
            other => return Err(format!("unexpected character {other:?}")),
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn word_is(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(x)) if x.eq_ignore_ascii_case(w))
    }
    fn expr(&mut self) -> Result<Expr, String> {
        let mut left = self.term()?;
        loop {
            if self.word_is("and") {
                self.next();
                let r = self.term()?;
                left = Expr::And(Box::new(left), Box::new(r));
            } else if self.word_is("or") {
                self.next();
                let r = self.term()?;
                left = Expr::Or(Box::new(left), Box::new(r));
            } else {
                break;
            }
        }
        Ok(left)
    }
    fn term(&mut self) -> Result<Expr, String> {
        if self.word_is("not") {
            self.next();
            return Ok(Expr::Not(Box::new(self.term()?)));
        }
        if self.peek() == Some(&Tok::LParen) {
            self.next();
            let e = self.expr()?;
            if self.next() != Some(Tok::RParen) {
                return Err("expected )".into());
            }
            return Ok(e);
        }
        let field = match self.next() {
            Some(Tok::Word(w)) => w.to_ascii_lowercase(),
            other => return Err(format!("expected a field name, got {other:?}")),
        };
        let op = match self.next() {
            Some(Tok::Op(op)) => op,
            other => return Err(format!("expected an operator after {field}, got {other:?}")),
        };
        let value = if op == Op::In {
            if self.next() != Some(Tok::LParen) {
                return Err("expected ( after in".into());
            }
            let mut items = Vec::new();
            loop {
                match self.next() {
                    Some(Tok::Word(w)) => items.push(w),
                    Some(Tok::Str(s)) => items.push(s),
                    Some(Tok::Num(n)) => items.push(n.to_string()),
                    Some(Tok::Comma) => {}
                    Some(Tok::RParen) => break,
                    other => return Err(format!("unexpected {other:?} in list")),
                }
            }
            Value::List(items)
        } else {
            match self.next() {
                Some(Tok::Word(w)) => Value::Str(w),
                Some(Tok::Str(s)) => Value::Str(s),
                Some(Tok::Num(n)) => Value::Num(n),
                other => return Err(format!("expected a value after {field}, got {other:?}")),
            }
        };
        Ok(Expr::Cmp { field, op, value })
    }
}

pub fn parse(src: &str) -> Result<Query, String> {
    let toks = tokenize(src)?;
    let mut p = Parser { toks, pos: 0 };
    let mut entity = "features".to_string();
    if let Some(Tok::Word(w)) = p.peek() {
        let lw = w.to_ascii_lowercase();
        if matches!(lw.as_str(), "features" | "tests" | "benchmarks" | "adrs")
            && matches!(p.toks.get(1), Some(Tok::Word(x)) if x.eq_ignore_ascii_case("where"))
        {
            entity = lw;
            p.next();
            p.next();
        } else if matches!(lw.as_str(), "features" | "tests" | "benchmarks" | "adrs")
            && p.toks.len() == 1
        {
            entity = lw;
            p.next();
        }
    }
    let expr = if p.peek().is_none() || p.word_is("order") || p.word_is("limit") {
        Expr::True
    } else {
        p.expr()?
    };
    let mut order_by = None;
    let mut limit = None;
    while let Some(t) = p.peek().cloned() {
        match t {
            Tok::Word(w) if w.eq_ignore_ascii_case("order") => {
                p.next();
                if !p.word_is("by") {
                    return Err("expected 'by' after order".into());
                }
                p.next();
                let f = match p.next() {
                    Some(Tok::Word(f)) => f.to_ascii_lowercase(),
                    other => return Err(format!("expected a field after order by, got {other:?}")),
                };
                let (f, mut desc) = match f.strip_prefix('-') {
                    Some(rest) => (rest.to_string(), true),
                    None => (f, false),
                };
                if p.word_is("desc") {
                    p.next();
                    desc = true;
                } else if p.word_is("asc") {
                    p.next();
                }
                order_by = Some((f, desc));
            }
            Tok::Word(w) if w.eq_ignore_ascii_case("limit") => {
                p.next();
                match p.next() {
                    Some(Tok::Num(n)) => limit = Some(n as usize),
                    other => return Err(format!("expected a number after limit, got {other:?}")),
                }
            }
            other => return Err(format!("unexpected {other:?}")),
        }
    }
    Ok(Query { entity, expr, order_by, limit })
}

/// A field's value on an entity: strings (possibly several), a number, or a timestamp.
#[derive(Debug, Clone)]
pub enum Field {
    Strs(Vec<String>),
    Num(f64),
    Time(Timestamp),
    Missing,
}

fn duration_secs(s: &str) -> Option<i64> {
    crate::time::parse_span(s).map(|d| d.as_secs())
}

fn cmp(field: &Field, op: &Op, value: &Value, now: Timestamp) -> bool {
    match (field, value) {
        (Field::Missing, _) => matches!(op, Op::Ne),
        (Field::Strs(vs), Value::Str(v)) => {
            let lv = v.to_ascii_lowercase();
            match op {
                Op::Eq | Op::Has => vs.iter().any(|x| x.to_ascii_lowercase() == lv),
                Op::Ne => !vs.iter().any(|x| x.to_ascii_lowercase() == lv),
                Op::Like => {
                    let g = globset::Glob::new(&format!("*{}*", v.trim_matches('*')))
                        .ok()
                        .map(|g| g.compile_matcher());
                    vs.iter().any(|x| {
                        x.to_ascii_lowercase().contains(&lv)
                            || g.as_ref().map(|g| g.is_match(x)).unwrap_or(false)
                    })
                }
                Op::Gt | Op::Ge | Op::Lt | Op::Le => vs.iter().any(|x| {
                    let o = x.to_ascii_lowercase().cmp(&lv);
                    match op {
                        Op::Gt => o.is_gt(),
                        Op::Ge => o.is_ge(),
                        Op::Lt => o.is_lt(),
                        _ => o.is_le(),
                    }
                }),
                Op::In => false,
            }
        }
        (Field::Strs(vs), Value::List(items)) => {
            let hit = vs.iter().any(|x| items.iter().any(|i| i.eq_ignore_ascii_case(x)));
            match op {
                Op::In | Op::Eq | Op::Has => hit,
                Op::Ne => !hit,
                _ => false,
            }
        }
        (Field::Strs(vs), Value::Num(n)) => {
            vs.iter().any(|x| x.parse::<f64>().map(|x| num_cmp(x, op, *n)).unwrap_or(false))
        }
        (Field::Num(x), Value::Num(n)) => num_cmp(*x, op, *n),
        (Field::Num(x), Value::Str(s)) => {
            s.parse::<f64>().map(|n| num_cmp(*x, op, n)).unwrap_or(false)
        }
        (Field::Num(x), Value::List(items)) => {
            items.iter().any(|i| i.parse::<f64>().map(|n| n == *x).unwrap_or(false))
                == matches!(op, Op::In | Op::Eq)
        }
        (Field::Time(t), Value::Str(s)) => {
            // `changed_since 7d` style: a duration means "within the last …"; else a timestamp.
            let age = now.duration_since(*t).as_secs();
            if let Some(secs) = duration_secs(s) {
                match op {
                    Op::Eq | Op::Ge | Op::Gt | Op::Like => age <= secs,
                    Op::Lt | Op::Le => age > secs,
                    Op::Ne => age > secs,
                    _ => false,
                }
            } else if let Some(ts) = crate::time::parse_timestamp(s)
                .or_else(|| crate::time::parse_timestamp(&format!("{s}T00:00:00Z")))
            {
                match op {
                    Op::Gt | Op::Ge => *t >= ts,
                    Op::Lt | Op::Le => *t <= ts,
                    Op::Eq => t.to_string().starts_with(s),
                    Op::Ne => !t.to_string().starts_with(s),
                    _ => false,
                }
            } else {
                false
            }
        }
        (Field::Time(_), _) => false,
    }
}

fn num_cmp(x: f64, op: &Op, n: f64) -> bool {
    match op {
        Op::Eq | Op::In | Op::Has => (x - n).abs() < 1e-9,
        Op::Ne => (x - n).abs() >= 1e-9,
        Op::Gt => x > n,
        Op::Ge => x >= n,
        Op::Lt => x < n,
        Op::Le => x <= n,
        Op::Like => false,
    }
}

fn eval(e: &Expr, get: &dyn Fn(&str) -> Field, now: Timestamp) -> bool {
    match e {
        Expr::True => true,
        Expr::And(a, b) => eval(a, get, now) && eval(b, get, now),
        Expr::Or(a, b) => eval(a, get, now) || eval(b, get, now),
        Expr::Not(a) => !eval(a, get, now),
        Expr::Cmp { field, op, value } => cmp(&get(field), op, value, now),
    }
}

pub fn feature_field(st: &ProjectState, v: &FeatureView, name: &str) -> Field {
    let f = &v.feature;
    let strs = |v: Vec<String>| Field::Strs(v);
    match name {
        "id" => strs(vec![f.id.to_string()]),
        "title" | "name" => strs(vec![f.title.clone()]),
        "state" | "health" => strs(vec![v.health.state.as_str().into()]),
        "confidence" => strs(vec![v.health.confidence.as_str().into()]),
        "importance" => strs(vec![f.importance.as_str().into()]),
        "status" | "lifecycle" => strs(vec![f.lifecycle.as_str().into()]),
        "tag" | "tags" => strs(f.tags.clone()),
        "owner" | "owners" => strs(f.owners.clone()),
        "adr" | "adrs" => strs(v.adrs.iter().map(|a| a.to_string()).collect()),
        "path" | "paths" => strs(f.paths.clone()),
        "group" => strs(f.group.clone()),
        "component" | "components" => {
            strs(f.claims.iter().map(|c| c.component.to_string()).collect())
        }
        "changed" | "changed_since" | "changed_at" => {
            v.impl_committed_at.map(Field::Time).unwrap_or(Field::Missing)
        }
        "attention" | "reasons" => strs(
            v.health
                .reasons
                .iter()
                .filter(|r| r.severity != Severity::Informational)
                .map(|r| r.reason.as_str().to_string())
                .collect(),
        ),
        "tests" => Field::Num(v.tests.len() as f64),
        "benchmarks" => Field::Num(v.benchmarks.len() as f64),
        "visual" | "visuals" => Field::Num(v.visuals.len() as f64),
        "performance.regression" => Field::Strs(
            v.benchmarks
                .iter()
                .filter_map(|b| st.benchmarks.get(b))
                .filter_map(|b| b.comparison.as_ref())
                .filter(|c| matches!(c.classification.as_str(), "regression" | "budget_breach"))
                .map(|c| c.classification.as_str().to_string())
                .collect(),
        ),
        _ => {
            if let Some(c) = name.strip_prefix("claim.") {
                return f
                    .claims
                    .iter()
                    .find(|x| x.component.as_str().eq_ignore_ascii_case(c))
                    .map(|x| strs(vec![x.status.as_str().into()]))
                    .unwrap_or(Field::Missing);
            }
            if let Some(k) = name.strip_prefix("evidence.") {
                let cat = EvidenceCategory::parse(k).or(match k {
                    "perf" | "performance" => Some(EvidenceCategory::Perf),
                    _ => None,
                });
                return cat
                    .and_then(|cat| v.health.rollups.iter().find(|r| r.category == cat))
                    .map(|r| strs(vec![r.status.as_str().into()]))
                    .unwrap_or(Field::Missing);
            }
            Field::Missing
        }
    }
}

pub fn test_field(st: &ProjectState, t: &TestView, name: &str) -> Field {
    let strs = |v: Vec<String>| Field::Strs(v);
    match name {
        "id" => strs(vec![t.test.id.to_string()]),
        "name" | "title" => strs(vec![t.test.name.clone()]),
        "status" | "state" => strs(vec![
            t.latest
                .as_ref()
                .map(|r| r.status.as_str().to_string())
                .unwrap_or_else(|| "unknown".into()),
        ]),
        "kind" => strs(vec![t.test.kind.as_str().into()]),
        "suite" => strs(vec![t.test.suite.to_string()]),
        "feature" | "features" => strs(t.features.iter().map(|(f, _)| f.to_string()).collect()),
        "flaky" => strs(vec![t.flaky.to_string()]),
        "duration" | "duration_ms" => t
            .latest
            .as_ref()
            .and_then(|r| r.duration_ms)
            .map(|d| Field::Num(d as f64))
            .unwrap_or(Field::Missing),
        "path" | "source" => {
            t.test.source_path.as_ref().map(|p| strs(vec![p.to_string()])).unwrap_or(Field::Missing)
        }
        "tag" | "tags" => strs(t.test.tags.clone()),
        "component" => {
            t.test.component.as_ref().map(|c| strs(vec![c.to_string()])).unwrap_or(Field::Missing)
        }
        "last_run" | "ran" | "changed_since" => {
            t.latest.as_ref().map(|r| Field::Time(r.timestamp.value)).unwrap_or(Field::Missing)
        }
        "runs" => Field::Num(t.history.len() as f64),
        "feature_state" => strs(
            t.features
                .iter()
                .filter_map(|(f, _)| st.features.get(f))
                .map(|f| f.health.state.as_str().to_string())
                .collect(),
        ),
        _ => Field::Missing,
    }
}

pub fn benchmark_field(_st: &ProjectState, b: &BenchmarkView, name: &str) -> Field {
    let strs = |v: Vec<String>| Field::Strs(v);
    let c = b.comparison.as_ref();
    match name {
        "id" => strs(vec![b.benchmark.id.to_string()]),
        "name" | "title" | "label" => strs(vec![b.label()]),
        "metric" => strs(vec![b.benchmark.metric.name.clone()]),
        "subject" => {
            b.benchmark.subject.as_ref().map(|s| strs(vec![s.clone()])).unwrap_or(Field::Missing)
        }
        "state" | "classification" => strs(vec![
            c.map(|c| c.classification.as_str().to_string()).unwrap_or_else(|| "unknown".into()),
        ]),
        "severity" => strs(vec![
            c.map(|c| c.severity.as_str().to_string()).unwrap_or_else(|| "informational".into()),
        ]),
        "determinism" => strs(vec![b.benchmark.metric.determinism.as_str().into()]),
        "unit" => strs(vec![b.benchmark.metric.unit.clone()]),
        "value" => {
            b.latest.as_ref().map(|r| Field::Num(r.measurement.value)).unwrap_or(Field::Missing)
        }
        "change" | "change_pct" => c
            .and_then(|c| c.relative_change)
            .map(|r| Field::Num(r * 100.0))
            .unwrap_or(Field::Missing),
        "feature" | "features" => strs(b.features.iter().map(|(f, _)| f.to_string()).collect()),
        "env" | "environment" => strs(vec![b.environment_label.clone()]),
        "last_run" | "changed_since" => {
            b.latest.as_ref().map(|r| Field::Time(r.timestamp.value)).unwrap_or(Field::Missing)
        }
        _ => Field::Missing,
    }
}

fn sort_key(f: &Field) -> (u8, f64, String) {
    match f {
        Field::Num(n) => (0, *n, String::new()),
        Field::Time(t) => (0, crate::time::millis(*t) as f64, String::new()),
        Field::Strs(v) => (1, 0.0, v.first().cloned().unwrap_or_default().to_ascii_lowercase()),
        Field::Missing => (2, 0.0, String::new()),
    }
}

pub enum Rows<'a> {
    Features(Vec<&'a FeatureView>),
    Tests(Vec<&'a TestView>),
    Benchmarks(Vec<&'a BenchmarkView>),
    Adrs(Vec<&'a crate::state::AdrView>),
}

pub fn run<'a>(st: &'a ProjectState, q: &Query) -> Result<Rows<'a>, String> {
    let now = crate::time::now();
    match q.entity.as_str() {
        "features" => {
            let mut rows: Vec<&FeatureView> = st
                .features
                .values()
                .filter(|v| eval(&q.expr, &|f| feature_field(st, v, f), now))
                .collect();
            if let Some((f, desc)) = &q.order_by {
                rows.sort_by(|a, b| {
                    let ka = sort_key(&feature_field(st, a, f));
                    let kb = sort_key(&feature_field(st, b, f));
                    let o =
                        ka.0.cmp(&kb.0)
                            .then(ka.1.partial_cmp(&kb.1).unwrap_or(std::cmp::Ordering::Equal))
                            .then(ka.2.cmp(&kb.2));
                    if *desc { o.reverse() } else { o }
                });
            } else {
                rows.sort_by(|a, b| {
                    b.health
                        .rank_score
                        .cmp(&a.health.rank_score)
                        .then_with(|| a.feature.id.cmp(&b.feature.id))
                });
            }
            if let Some(n) = q.limit {
                rows.truncate(n);
            }
            Ok(Rows::Features(rows))
        }
        "tests" => {
            let mut rows: Vec<&TestView> = st
                .tests
                .values()
                .filter(|t| eval(&q.expr, &|f| test_field(st, t, f), now))
                .collect();
            if let Some((f, desc)) = &q.order_by {
                rows.sort_by(|a, b| {
                    let ka = sort_key(&test_field(st, a, f));
                    let kb = sort_key(&test_field(st, b, f));
                    let o =
                        ka.0.cmp(&kb.0)
                            .then(ka.1.partial_cmp(&kb.1).unwrap_or(std::cmp::Ordering::Equal))
                            .then(ka.2.cmp(&kb.2));
                    if *desc { o.reverse() } else { o }
                });
            }
            if let Some(n) = q.limit {
                rows.truncate(n);
            }
            Ok(Rows::Tests(rows))
        }
        "benchmarks" => {
            let mut rows: Vec<&BenchmarkView> = st
                .benchmarks
                .values()
                .filter(|b| eval(&q.expr, &|f| benchmark_field(st, b, f), now))
                .collect();
            if let Some((f, desc)) = &q.order_by {
                rows.sort_by(|a, b| {
                    let ka = sort_key(&benchmark_field(st, a, f));
                    let kb = sort_key(&benchmark_field(st, b, f));
                    let o =
                        ka.0.cmp(&kb.0)
                            .then(ka.1.partial_cmp(&kb.1).unwrap_or(std::cmp::Ordering::Equal))
                            .then(ka.2.cmp(&kb.2));
                    if *desc { o.reverse() } else { o }
                });
            }
            if let Some(n) = q.limit {
                rows.truncate(n);
            }
            Ok(Rows::Benchmarks(rows))
        }
        "adrs" => {
            let mut rows: Vec<&crate::state::AdrView> = st
                .adrs
                .values()
                .filter(|a| {
                    eval(
                        &q.expr,
                        &|f| match f {
                            "id" => Field::Strs(vec![a.adr.id.to_string()]),
                            "title" | "name" => Field::Strs(vec![a.adr.title.clone()]),
                            "status" | "state" => Field::Strs(vec![a.adr.status.as_str().into()]),
                            "date" | "changed_since" => a
                                .adr
                                .date
                                .and_then(|d| {
                                    d.at(0, 0, 0, 0).to_zoned(jiff::tz::TimeZone::UTC).ok()
                                })
                                .map(|z| Field::Time(z.timestamp()))
                                .unwrap_or(Field::Missing),
                            "feature" | "features" => {
                                Field::Strs(a.features.iter().map(|f| f.to_string()).collect())
                            }
                            "tag" | "tags" => Field::Strs(a.adr.tags.clone()),
                            _ => Field::Missing,
                        },
                        now,
                    )
                })
                .collect();
            if let Some(n) = q.limit {
                rows.truncate(n);
            }
            Ok(Rows::Adrs(rows))
        }
        other => Err(format!("unknown entity {other}; use features, tests, benchmarks or adrs")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_grammar() {
        let q =
            parse("state = unknown and importance in (critical, high) order by -changed limit 5")
                .unwrap();
        assert_eq!(q.entity, "features");
        assert_eq!(q.order_by, Some(("changed".into(), true)));
        assert_eq!(q.limit, Some(5));
        let q = parse("tests where status = failed or (flaky = true and not kind = e2e)").unwrap();
        assert_eq!(q.entity, "tests");
        let q = parse("features where claim.core.parser = shipped and evidence.unit != pass and title ~ 'layer'").unwrap();
        assert!(matches!(q.expr, Expr::And(..)));
        assert!(parse("state =").is_err());
        assert!(parse("state = unknown order limit").is_err());
        let q = parse("changed_since >= 7d").unwrap();
        assert!(matches!(q.expr, Expr::Cmp { .. }));
    }

    #[test]
    fn compares_values() {
        let now = crate::time::now();
        assert!(cmp(
            &Field::Strs(vec!["Unknown".into()]),
            &Op::Eq,
            &Value::Str("unknown".into()),
            now
        ));
        assert!(cmp(
            &Field::Strs(vec!["a".into(), "b".into()]),
            &Op::In,
            &Value::List(vec!["b".into()]),
            now
        ));
        assert!(cmp(
            &Field::Strs(vec!["Layers panel".into()]),
            &Op::Like,
            &Value::Str("layer".into()),
            now
        ));
        assert!(cmp(&Field::Num(3.0), &Op::Gt, &Value::Num(2.0), now));
        let t = now - jiff::SignedDuration::from_secs(3 * 86_400);
        assert!(cmp(&Field::Time(t), &Op::Ge, &Value::Str("7d".into()), now));
        assert!(!cmp(&Field::Time(t), &Op::Ge, &Value::Str("1d".into()), now));
        assert!(cmp(&Field::Missing, &Op::Ne, &Value::Str("x".into()), now));
    }
}

//! `vyrmQL`: a small, explicit query grammar that lowers into `vyrmMX`.
//!
//! Parsing is intentionally separate from binding and execution. The parser
//! validates syntax and identifiers, but only a catalog can decide whether a
//! type or property exists. Every query must state valid time and known state;
//! there is no ambient "latest" hidden in the language contract.

use serde::{Deserialize, Serialize};
use std::fmt;
use vyrm_core::{Predicate, RuntimeType, RuntimeValue};

pub const QUERY_CONTRACT_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    pub contract_version: u16,
    pub source: Source,
    pub temporal: TemporalSelector,
    #[serde(default)]
    pub filters: Vec<Filter>,
    pub projection: Projection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default)]
    pub explain_contract: bool,
}

impl Query {
    pub fn new(source: Source, temporal: TemporalSelector) -> Self {
        Self {
            contract_version: QUERY_CONTRACT_VERSION,
            source,
            temporal,
            filters: Vec::new(),
            projection: Projection::All,
            limit: None,
            explain_contract: false,
        }
    }

    pub fn validate(&self) -> Result<(), ParseError> {
        if self.contract_version != QUERY_CONTRACT_VERSION {
            return Err(ParseError::new(
                0,
                format!(
                    "unsupported query contract version {}",
                    self.contract_version
                ),
            ));
        }
        if self.limit == Some(0) {
            return Err(ParseError::new(0, "LIMIT must be greater than zero"));
        }
        if let Projection::Fields(fields) = &self.projection {
            if fields.is_empty() {
                return Err(ParseError::new(0, "PROJECT requires at least one field"));
            }
            for field in fields {
                validate_field(field, 0)?;
            }
        }
        for filter in &self.filters {
            validate_field(&filter.field, 0)?;
            if let ValueExpr::Literal(value) = &filter.value {
                if !matches!(
                    value,
                    RuntimeValue::Null
                        | RuntimeValue::Bool(_)
                        | RuntimeValue::Integer(_)
                        | RuntimeValue::Unsigned(_)
                        | RuntimeValue::String(_)
                ) {
                    return Err(ParseError::new(
                        0,
                        "this query contract only supports scalar filter literals",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn canonical(&self) -> String {
        let mut out = format!("FROM {} AT VALID ", self.source.canonical());
        out.push_str(&self.temporal.valid_at.canonical());
        out.push_str(" KNOWN ");
        out.push_str(&self.temporal.known_at.canonical());
        if !self.filters.is_empty() {
            out.push_str(" WHERE ");
            for (index, filter) in self.filters.iter().enumerate() {
                if index > 0 {
                    out.push_str(" AND ");
                }
                out.push_str(&filter.field);
                out.push_str(" = ");
                out.push_str(&filter.value.canonical());
            }
        }
        out.push_str(" PROJECT ");
        match &self.projection {
            Projection::All => out.push('*'),
            Projection::Fields(fields) => out.push_str(&fields.join(", ")),
        }
        if let Some(limit) = self.limit {
            out.push_str(&format!(" LIMIT {limit}"));
        }
        if self.explain_contract {
            out.push_str(" EXPLAIN CONTRACT");
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "family", rename_all = "snake_case")]
pub enum Source {
    Record {
        kind: RuntimeType,
    },
    Relation {
        kind: RuntimeType,
    },
    Event {
        kind: RuntimeType,
    },
    Claim {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        predicate: Option<Predicate>,
    },
}

impl Source {
    fn canonical(&self) -> String {
        match self {
            Self::Record { kind } => format!("record:{kind}"),
            Self::Relation { kind } => format!("relation:{kind}"),
            Self::Event { kind } => format!("event:{kind}"),
            Self::Claim {
                predicate: Some(predicate),
            } => format!("claim:{predicate}"),
            Self::Claim { predicate: None } => "claim".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporalSelector {
    pub valid_at: TimeExpr,
    pub known_at: CursorExpr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TimeExpr {
    Literal(u64),
    Parameter(String),
}

impl TimeExpr {
    fn canonical(&self) -> String {
        match self {
            Self::Literal(value) => value.to_string(),
            Self::Parameter(value) => format!("${value}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CursorExpr {
    Head,
    Literal(u64),
    Parameter(String),
}

impl CursorExpr {
    fn canonical(&self) -> String {
        match self {
            Self::Head => "HEAD".into(),
            Self::Literal(value) => value.to_string(),
            Self::Parameter(value) => format!("${value}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    pub field: String,
    pub value: ValueExpr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ValueExpr {
    Literal(RuntimeValue),
    Parameter(String),
}

impl ValueExpr {
    fn canonical(&self) -> String {
        match self {
            Self::Parameter(value) => format!("${value}"),
            Self::Literal(RuntimeValue::Null) => "null".into(),
            Self::Literal(RuntimeValue::Bool(value)) => value.to_string(),
            Self::Literal(RuntimeValue::Integer(value)) => value.to_string(),
            Self::Literal(RuntimeValue::Unsigned(value)) => value.to_string(),
            Self::Literal(RuntimeValue::String(value)) => quote(value),
            Self::Literal(value) => quote(&format!("{value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "fields", rename_all = "snake_case")]
pub enum Projection {
    All,
    Fields(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub offset: usize,
    pub message: String,
}

impl ParseError {
    fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "query parse error at byte {}: {}",
            self.offset, self.message
        )
    }
}

impl std::error::Error for ParseError {}

pub fn parse(input: &str) -> Result<Query, ParseError> {
    let tokens = tokenize(input)?;
    let mut parser = Parser { tokens, cursor: 0 };
    let query = parser.query()?;
    if let Some(token) = parser.peek() {
        return Err(ParseError::new(
            token.offset,
            format!("unexpected token {}", token.describe()),
        ));
    }
    query.validate()?;
    Ok(query)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    offset: usize,
    kind: TokenKind,
}

impl Token {
    fn describe(&self) -> String {
        match &self.kind {
            TokenKind::Word(value) => format!("{value:?}"),
            TokenKind::String(value) => format!("string {value:?}"),
            TokenKind::Number(value) => value.to_string(),
            TokenKind::Colon => "':'".into(),
            TokenKind::Comma => "','".into(),
            TokenKind::Equals => "'='".into(),
            TokenKind::Star => "'*'".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    Word(String),
    String(String),
    Number(u64),
    Colon,
    Comma,
    Equals,
    Star,
}

fn tokenize(input: &str) -> Result<Vec<Token>, ParseError> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        let offset = cursor;
        let kind = match bytes[cursor] {
            b':' => {
                cursor += 1;
                TokenKind::Colon
            }
            b',' => {
                cursor += 1;
                TokenKind::Comma
            }
            b'=' => {
                cursor += 1;
                TokenKind::Equals
            }
            b'*' => {
                cursor += 1;
                TokenKind::Star
            }
            b'"' => {
                cursor += 1;
                let mut value = String::new();
                let mut closed = false;
                while cursor < bytes.len() {
                    match bytes[cursor] {
                        b'"' => {
                            cursor += 1;
                            closed = true;
                            break;
                        }
                        b'\\' => {
                            cursor += 1;
                            let escaped = *bytes.get(cursor).ok_or_else(|| {
                                ParseError::new(offset, "unterminated string escape")
                            })?;
                            value.push(match escaped {
                                b'"' => '"',
                                b'\\' => '\\',
                                b'n' => '\n',
                                b'r' => '\r',
                                b't' => '\t',
                                _ => {
                                    return Err(ParseError::new(
                                        cursor,
                                        "unsupported string escape",
                                    ))
                                }
                            });
                            cursor += 1;
                        }
                        byte if byte.is_ascii() => {
                            value.push(byte as char);
                            cursor += 1;
                        }
                        _ => {
                            let rest = &input[cursor..];
                            let character = rest.chars().next().expect("non-empty UTF-8 tail");
                            value.push(character);
                            cursor += character.len_utf8();
                        }
                    }
                }
                if !closed {
                    return Err(ParseError::new(offset, "unterminated string literal"));
                }
                TokenKind::String(value)
            }
            byte if byte.is_ascii_digit() => {
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    cursor += 1;
                }
                let value = input[offset..cursor]
                    .parse()
                    .map_err(|_| ParseError::new(offset, "integer is outside u64 range"))?;
                TokenKind::Number(value)
            }
            _ => {
                while cursor < bytes.len()
                    && !bytes[cursor].is_ascii_whitespace()
                    && !matches!(bytes[cursor], b':' | b',' | b'=' | b'*' | b'"')
                {
                    cursor += 1;
                }
                if cursor == offset {
                    return Err(ParseError::new(offset, "unsupported query character"));
                }
                TokenKind::Word(input[offset..cursor].to_owned())
            }
        };
        tokens.push(Token { offset, kind });
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    fn query(&mut self) -> Result<Query, ParseError> {
        self.keyword("FROM")?;
        let source = self.source()?;
        self.keyword("AT")?;
        self.keyword("VALID")?;
        let valid_at = self.time_expr()?;
        self.keyword("KNOWN")?;
        let known_at = self.cursor_expr()?;
        let mut query = Query::new(source, TemporalSelector { valid_at, known_at });
        let mut saw_where = false;
        let mut saw_project = false;
        let mut saw_limit = false;
        let mut saw_explain = false;
        while let Some(token) = self.peek() {
            if token.is_keyword("WHERE") {
                if saw_where {
                    return Err(ParseError::new(token.offset, "duplicate WHERE clause"));
                }
                saw_where = true;
                self.cursor += 1;
                query.filters = self.filters()?;
            } else if token.is_keyword("PROJECT") {
                if saw_project {
                    return Err(ParseError::new(token.offset, "duplicate PROJECT clause"));
                }
                saw_project = true;
                self.cursor += 1;
                query.projection = self.projection()?;
            } else if token.is_keyword("LIMIT") {
                if saw_limit {
                    return Err(ParseError::new(token.offset, "duplicate LIMIT clause"));
                }
                saw_limit = true;
                let clause_offset = token.offset;
                self.cursor += 1;
                let value = self.number("row limit")?;
                query.limit = Some(usize::try_from(value).map_err(|_| {
                    ParseError::new(clause_offset, "row limit exceeds this platform's capacity")
                })?);
            } else if token.is_keyword("EXPLAIN") {
                if saw_explain {
                    return Err(ParseError::new(token.offset, "duplicate EXPLAIN clause"));
                }
                saw_explain = true;
                self.cursor += 1;
                self.keyword("CONTRACT")?;
                query.explain_contract = true;
            } else {
                break;
            }
        }
        Ok(query)
    }

    fn source(&mut self) -> Result<Source, ParseError> {
        let token = self.next("source family")?;
        let TokenKind::Word(family) = &token.kind else {
            return Err(ParseError::new(token.offset, "expected source family"));
        };
        let family = family.to_ascii_lowercase();
        if family == "claim" && !self.peek_is(TokenKind::Colon) {
            return Ok(Source::Claim { predicate: None });
        }
        self.punctuation(TokenKind::Colon, "':' after source family")?;
        let name = self.word("source type")?;
        match family.as_str() {
            "record" => RuntimeType::new(name)
                .map(|kind| Source::Record { kind })
                .map_err(|error| ParseError::new(token.offset, error.to_string())),
            "relation" => RuntimeType::new(name)
                .map(|kind| Source::Relation { kind })
                .map_err(|error| ParseError::new(token.offset, error.to_string())),
            "event" => RuntimeType::new(name)
                .map(|kind| Source::Event { kind })
                .map_err(|error| ParseError::new(token.offset, error.to_string())),
            "claim" => Predicate::new(name)
                .map(|predicate| Source::Claim {
                    predicate: Some(predicate),
                })
                .map_err(|error| ParseError::new(token.offset, error.to_string())),
            _ => Err(ParseError::new(
                token.offset,
                format!("unknown source family {family:?}"),
            )),
        }
    }

    fn time_expr(&mut self) -> Result<TimeExpr, ParseError> {
        let token = self.next("valid-time literal or parameter")?;
        match &token.kind {
            TokenKind::Number(value) => Ok(TimeExpr::Literal(*value)),
            TokenKind::Word(value) => parameter(value, token.offset).map(TimeExpr::Parameter),
            _ => Err(ParseError::new(
                token.offset,
                "expected valid-time literal or $parameter",
            )),
        }
    }

    fn cursor_expr(&mut self) -> Result<CursorExpr, ParseError> {
        let token = self.next("known cursor")?;
        match &token.kind {
            TokenKind::Number(value) => Ok(CursorExpr::Literal(*value)),
            TokenKind::Word(value) if value.eq_ignore_ascii_case("HEAD") => Ok(CursorExpr::Head),
            TokenKind::Word(value) => parameter(value, token.offset).map(CursorExpr::Parameter),
            _ => Err(ParseError::new(
                token.offset,
                "expected cursor, HEAD, or $parameter",
            )),
        }
    }

    fn filters(&mut self) -> Result<Vec<Filter>, ParseError> {
        let mut filters = Vec::new();
        loop {
            let field = self.word("filter field")?;
            validate_field(&field, self.previous_offset())?;
            self.punctuation(TokenKind::Equals, "'=' after filter field")?;
            let value = self.value_expr()?;
            filters.push(Filter { field, value });
            if self.peek().is_some_and(|token| token.is_keyword("AND")) {
                self.cursor += 1;
            } else {
                break;
            }
        }
        Ok(filters)
    }

    fn value_expr(&mut self) -> Result<ValueExpr, ParseError> {
        let token = self.next("filter value")?;
        match &token.kind {
            TokenKind::String(value) => Ok(ValueExpr::Literal(RuntimeValue::String(value.clone()))),
            TokenKind::Number(value) => Ok(ValueExpr::Literal(RuntimeValue::Unsigned(*value))),
            TokenKind::Word(value) if value.starts_with('$') => {
                parameter(value, token.offset).map(ValueExpr::Parameter)
            }
            TokenKind::Word(value) if value.eq_ignore_ascii_case("true") => {
                Ok(ValueExpr::Literal(RuntimeValue::Bool(true)))
            }
            TokenKind::Word(value) if value.eq_ignore_ascii_case("false") => {
                Ok(ValueExpr::Literal(RuntimeValue::Bool(false)))
            }
            TokenKind::Word(value) if value.eq_ignore_ascii_case("null") => {
                Ok(ValueExpr::Literal(RuntimeValue::Null))
            }
            _ => Err(ParseError::new(
                token.offset,
                "expected string, unsigned integer, boolean, null, or $parameter",
            )),
        }
    }

    fn projection(&mut self) -> Result<Projection, ParseError> {
        if self.peek_is(TokenKind::Star) {
            self.cursor += 1;
            return Ok(Projection::All);
        }
        let mut fields = Vec::new();
        loop {
            let field = self.word("projection field")?;
            validate_field(&field, self.previous_offset())?;
            fields.push(field);
            if self.peek_is(TokenKind::Comma) {
                self.cursor += 1;
            } else {
                break;
            }
        }
        Ok(Projection::Fields(fields))
    }

    fn keyword(&mut self, expected: &str) -> Result<(), ParseError> {
        let token = self.next(expected)?;
        if token.is_keyword(expected) {
            Ok(())
        } else {
            Err(ParseError::new(
                token.offset,
                format!("expected {expected}, found {}", token.describe()),
            ))
        }
    }

    fn punctuation(&mut self, expected: TokenKind, description: &str) -> Result<(), ParseError> {
        let token = self.next(description)?;
        if token.kind == expected {
            Ok(())
        } else {
            Err(ParseError::new(
                token.offset,
                format!("expected {description}, found {}", token.describe()),
            ))
        }
    }

    fn word(&mut self, description: &str) -> Result<String, ParseError> {
        let token = self.next(description)?;
        match &token.kind {
            TokenKind::Word(value) => Ok(value.clone()),
            _ => Err(ParseError::new(
                token.offset,
                format!("expected {description}, found {}", token.describe()),
            )),
        }
    }

    fn number(&mut self, description: &str) -> Result<u64, ParseError> {
        let token = self.next(description)?;
        match token.kind {
            TokenKind::Number(value) => Ok(value),
            _ => Err(ParseError::new(
                token.offset,
                format!("expected {description}, found {}", token.describe()),
            )),
        }
    }

    fn next(&mut self, expected: &str) -> Result<Token, ParseError> {
        let token = self.tokens.get(self.cursor).cloned().ok_or_else(|| {
            ParseError::new(
                self.tokens.last().map_or(0, |token| token.offset + 1),
                format!("expected {expected}, found end of query"),
            )
        })?;
        self.cursor += 1;
        Ok(token)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }

    fn peek_is(&self, expected: TokenKind) -> bool {
        self.peek().is_some_and(|token| token.kind == expected)
    }

    fn previous_offset(&self) -> usize {
        self.tokens
            .get(self.cursor.saturating_sub(1))
            .map_or(0, |token| token.offset)
    }
}

impl Token {
    fn is_keyword(&self, expected: &str) -> bool {
        matches!(&self.kind, TokenKind::Word(value) if value.eq_ignore_ascii_case(expected))
    }
}

fn parameter(value: &str, offset: usize) -> Result<String, ParseError> {
    let Some(name) = value.strip_prefix('$') else {
        return Err(ParseError::new(offset, "parameter must begin with '$'"));
    };
    validate_field(name, offset)?;
    Ok(name.to_owned())
}

fn validate_field(value: &str, offset: usize) -> Result<(), ParseError> {
    if value.is_empty()
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '.'))
    {
        return Err(ParseError::new(
            offset,
            format!("invalid field or parameter name {value:?}"),
        ));
    }
    Ok(())
}

fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '\"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            value => out.push(value),
        }
    }
    out.push('\"');
    out
}

use crate::contract::invalid;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use vyrm_core::{Result, RuntimeProperties, RuntimeValue};

const MAX_FILTER_DEPTH: usize = 32;
const MAX_FILTER_NODES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operator", rename_all = "snake_case")]
pub enum FilterOperator {
    Equals {
        value: RuntimeValue,
    },
    NotEquals {
        value: RuntimeValue,
    },
    In {
        values: Vec<RuntimeValue>,
    },
    Range {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gt: Option<RuntimeValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gte: Option<RuntimeValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lt: Option<RuntimeValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lte: Option<RuntimeValue>,
    },
    Exists {
        value: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterCondition {
    pub property: String,
    #[serde(flatten)]
    pub operator: FilterOperator,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FilterExpression {
    Condition { condition: FilterCondition },
    All { filters: Vec<FilterExpression> },
    Any { filters: Vec<FilterExpression> },
    Not { filter: Box<FilterExpression> },
}

impl FilterExpression {
    pub fn validate(&self) -> Result<()> {
        let mut nodes = 0;
        self.validate_at(0, &mut nodes)
    }

    pub fn matches(&self, properties: &RuntimeProperties) -> bool {
        match self {
            Self::Condition { condition } => condition.matches(properties),
            Self::All { filters } => filters.iter().all(|filter| filter.matches(properties)),
            Self::Any { filters } => filters.iter().any(|filter| filter.matches(properties)),
            Self::Not { filter } => !filter.matches(properties),
        }
    }

    pub fn referenced_properties(&self) -> Vec<String> {
        let mut values = Vec::new();
        self.collect_properties(&mut values);
        values.sort();
        values.dedup();
        values
    }

    fn validate_at(&self, depth: usize, nodes: &mut usize) -> Result<()> {
        *nodes += 1;
        if depth > MAX_FILTER_DEPTH || *nodes > MAX_FILTER_NODES {
            return invalid("vector filter exceeds depth or node limit");
        }
        match self {
            Self::Condition { condition } => condition.validate(),
            Self::All { filters } | Self::Any { filters } => {
                if filters.is_empty() {
                    return invalid("vector all/any filter must contain at least one child");
                }
                for filter in filters {
                    filter.validate_at(depth + 1, nodes)?;
                }
                Ok(())
            }
            Self::Not { filter } => filter.validate_at(depth + 1, nodes),
        }
    }

    fn collect_properties(&self, values: &mut Vec<String>) {
        match self {
            Self::Condition { condition } => values.push(condition.property.clone()),
            Self::All { filters } | Self::Any { filters } => {
                for filter in filters {
                    filter.collect_properties(values);
                }
            }
            Self::Not { filter } => filter.collect_properties(values),
        }
    }
}

impl FilterCondition {
    fn validate(&self) -> Result<()> {
        if self.property.trim().is_empty() || self.property.as_bytes().contains(&0) {
            return invalid("vector filter property must be non-empty and contain no NUL bytes");
        }
        match &self.operator {
            FilterOperator::In { values } if values.is_empty() => {
                invalid("vector in filter must contain at least one value")
            }
            FilterOperator::Range { gt, gte, lt, lte } => {
                if gt.is_some() && gte.is_some() {
                    return invalid("vector range filter cannot set both gt and gte");
                }
                if lt.is_some() && lte.is_some() {
                    return invalid("vector range filter cannot set both lt and lte");
                }
                if gt.is_none() && gte.is_none() && lt.is_none() && lte.is_none() {
                    return invalid("vector range filter must declare a bound");
                }
                for value in [gt, gte, lt, lte].into_iter().flatten() {
                    if !is_orderable(value) {
                        return invalid(
                            "vector range bounds must be integer, unsigned, decimal, or string",
                        );
                    }
                }
                let lower = gt.as_ref().or(gte.as_ref());
                let upper = lt.as_ref().or(lte.as_ref());
                if let (Some(lower), Some(upper)) = (lower, upper) {
                    let ordering = compare_values(lower, upper).ok_or_else(|| {
                        vyrm_core::Error::InvalidRuntime {
                            reason: "vector range bounds use incompatible value types".into(),
                        }
                    })?;
                    if ordering == Ordering::Greater
                        || (ordering == Ordering::Equal && (gt.is_some() || lt.is_some()))
                    {
                        return invalid("vector range lower bound must precede its upper bound");
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn matches(&self, properties: &RuntimeProperties) -> bool {
        let actual = properties.get(&self.property);
        match &self.operator {
            FilterOperator::Equals { value } => actual == Some(value),
            FilterOperator::NotEquals { value } => actual.is_some_and(|actual| actual != value),
            FilterOperator::In { values } => {
                actual.is_some_and(|actual| values.iter().any(|value| value == actual))
            }
            FilterOperator::Exists { value } => actual.is_some() == *value,
            FilterOperator::Range { gt, gte, lt, lte } => actual.is_some_and(|actual| {
                gt.as_ref()
                    .is_none_or(|bound| compare_values(actual, bound) == Some(Ordering::Greater))
                    && gte.as_ref().is_none_or(|bound| {
                        compare_values(actual, bound)
                            .is_some_and(|ordering| ordering != Ordering::Less)
                    })
                    && lt
                        .as_ref()
                        .is_none_or(|bound| compare_values(actual, bound) == Some(Ordering::Less))
                    && lte.as_ref().is_none_or(|bound| {
                        compare_values(actual, bound)
                            .is_some_and(|ordering| ordering != Ordering::Greater)
                    })
            }),
        }
    }
}

fn is_orderable(value: &RuntimeValue) -> bool {
    match value {
        RuntimeValue::Integer(_) | RuntimeValue::Unsigned(_) | RuntimeValue::String(_) => true,
        RuntimeValue::Decimal(value) => decimal_parts(value).is_some(),
        _ => false,
    }
}

fn compare_values(left: &RuntimeValue, right: &RuntimeValue) -> Option<Ordering> {
    match (left, right) {
        (RuntimeValue::Integer(left), RuntimeValue::Integer(right)) => Some(left.cmp(right)),
        (RuntimeValue::Unsigned(left), RuntimeValue::Unsigned(right)) => Some(left.cmp(right)),
        (RuntimeValue::Integer(left), RuntimeValue::Unsigned(right)) => {
            if *left < 0 {
                Some(Ordering::Less)
            } else {
                Some((*left as u64).cmp(right))
            }
        }
        (RuntimeValue::Unsigned(left), RuntimeValue::Integer(right)) => compare_values(
            &RuntimeValue::Integer(*right),
            &RuntimeValue::Unsigned(*left),
        )
        .map(Ordering::reverse),
        (RuntimeValue::Decimal(left), RuntimeValue::Decimal(right)) => compare_decimal(left, right),
        (RuntimeValue::String(left), RuntimeValue::String(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn compare_decimal(left: &str, right: &str) -> Option<Ordering> {
    let (left_negative, left_digits, left_exponent) = decimal_parts(left)?;
    let (right_negative, right_digits, right_exponent) = decimal_parts(right)?;
    if left_digits == "0" && right_digits == "0" {
        return Some(Ordering::Equal);
    }
    if left_negative != right_negative {
        return Some(if left_negative {
            Ordering::Less
        } else {
            Ordering::Greater
        });
    }
    let left_magnitude = left_digits.len() as i64 + left_exponent;
    let right_magnitude = right_digits.len() as i64 + right_exponent;
    let mut ordering = left_magnitude.cmp(&right_magnitude);
    if ordering == Ordering::Equal {
        let width = left_digits.len().max(right_digits.len());
        ordering = (0..width)
            .map(|index| {
                let left = left_digits.as_bytes().get(index).copied().unwrap_or(b'0');
                let right = right_digits.as_bytes().get(index).copied().unwrap_or(b'0');
                left.cmp(&right)
            })
            .find(|ordering| *ordering != Ordering::Equal)
            .unwrap_or(Ordering::Equal);
    }
    Some(if left_negative {
        ordering.reverse()
    } else {
        ordering
    })
}

fn decimal_parts(value: &str) -> Option<(bool, String, i64)> {
    let (mantissa, exponent) = match value.find(['e', 'E']) {
        Some(index) => {
            if value[index + 1..].contains(['e', 'E']) {
                return None;
            }
            let exponent = value[index + 1..].parse::<i64>().ok()?;
            if exponent.unsigned_abs() > 1_000_000 {
                return None;
            }
            (&value[..index], exponent)
        }
        None => (value, 0),
    };
    let (negative, mantissa) = match mantissa.as_bytes().first() {
        Some(b'-') => (true, &mantissa[1..]),
        Some(b'+') => (false, &mantissa[1..]),
        _ => (false, mantissa),
    };
    let mut parts = mantissa.split('.');
    let whole = parts.next()?;
    let fraction = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || (whole.is_empty() && fraction.is_empty())
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let mut digits = format!("{whole}{fraction}");
    let leading = digits.bytes().take_while(|byte| *byte == b'0').count();
    digits.drain(..leading);
    if digits.is_empty() {
        return Some((false, "0".into(), 0));
    }
    let mut exponent = exponent.checked_sub(fraction.len() as i64)?;
    while digits.ends_with('0') {
        digits.pop();
        exponent = exponent.checked_add(1)?;
    }
    Some((negative, digits, exponent))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_ranges_compare_without_floating_point_loss() {
        assert_eq!(
            compare_decimal("9007199254740993", "9007199254740992"),
            Some(Ordering::Greater)
        );
        assert_eq!(compare_decimal("1.20e2", "120"), Some(Ordering::Equal));
        assert_eq!(compare_decimal("-0.001", "-0.0009"), Some(Ordering::Less));
    }

    #[test]
    fn missing_properties_have_explicit_boolean_semantics() {
        let properties = RuntimeProperties::new();
        let exists = FilterExpression::Condition {
            condition: FilterCondition {
                property: "missing".into(),
                operator: FilterOperator::Exists { value: false },
            },
        };
        assert!(exists.matches(&properties));
    }
}

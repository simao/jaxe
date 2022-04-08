use nom::{IResult, AsChar, branch};

use nom::bytes::complete::tag;
use nom::character::complete::{multispace0, char};
use nom::multi::separated_list1;
use nom::sequence::{tuple, delimited, separated_pair};
use serde_json::Value;
use nom::InputTakeAtPosition;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;

use nom_locate::LocatedSpan;

type Span<'a> = LocatedSpan<&'a str>;

fn path_segment(input: Span) -> IResult<Span, String> {
    let (rest, v) = input.split_at_position_complete(|item| ! item.is_alphanum() && item != '_' && item != '-')?;
    Ok((rest, v.to_string()))
}

fn path(input: Span) -> IResult<Span, EPath> {
    let (rest, matched) = separated_list1(tag("."), path_segment)(input)?;
    Ok((rest, EPath(matched)))
}

fn unquoted_value(input: Span) -> IResult<Span, &str> {
    let (rest, v) = input.split_at_position_complete(|item| item.is_whitespace() || item == ',' || item == ')' || item == '"' )?;
    Ok((rest, v.fragment()))
}

fn end_quoted_string(input: Span) -> IResult<Span, &str> {
    let (rest, v) = input.split_at_position_complete(|item| item == '"' )?;
    Ok((rest, v.fragment()))
}

// TODO: This is not right... Would need to parse escaped chars etc
fn value(input: Span) -> IResult<Span, &str> {
    branch::alt((
        delimited(char('"'), end_quoted_string, char('"')),
        unquoted_value,
    ))(input)
}

fn operation_equals(input: Span) -> IResult<Span, Exp> {
    let (input, (path, value)) = separated_pair(path, tuple((multispace0, tag("=="), multispace0)), value)(input)?;
    Ok((input, Exp::Equals(path, value.to_owned())))
}

fn operation_not_equals(input: Span) -> IResult<Span, Exp> {
    let (input, (path, value)) = separated_pair(path, tuple((multispace0, tag("!="), multispace0)), value)(input)?;
    Ok((input, Exp::NotEquals(path, value.to_string())))
}

fn operation(input: Span) -> IResult<Span, Exp> {
    operation_not_equals(input).or(operation_equals(input))
}

fn exists(input: Span) -> IResult<Span, Exp> {
    let (rest, (_, path)) = tuple((tag("exists"), delimited(tag("("), path, tag(")"))))(input)?;
    Ok((rest, Exp::Exists(path)))
}

fn exp(input: Span) -> IResult<Span, Exp> {
    branch::alt((or, and, not, contains, exists, operation))(input)
}

fn comma(input: Span) -> IResult<Span, ()> {
    let (res, _) = tuple((multispace0, tag(","), multispace0))(input)?;
    Ok((res, ()))
}

fn not(input: Span) -> IResult<Span, Exp> {
    let (rest, (_, _, exp)) = tuple((tag("not"), multispace0, delimited(tag("("), exp, tag(")"))))(input)?;
    Ok((rest, Exp::Not(exp.into())))
}

fn and(input: Span) -> IResult<Span, Exp> {
    let (input, _) = tag("and")(input)?;
    let (input, exps) = delimited(tag("("), separated_list1(comma, exp), tag(")"))(input)?;
    Ok((input, Exp::And(exps)))
}

fn or(input: Span) -> IResult<Span, Exp> {
    let (input, _) = tag("or")(input)?;
    let (input, exps) = delimited(tag("("), separated_list1(comma, exp), tag(")"))(input)?;
    Ok((input, Exp::Or(exps)))
}

fn contains(input: Span) -> IResult<Span, Exp> {
    let (input, _) = tag("contains")(input)?;
    let (input, (path, val)) = delimited(tag("("), separated_pair(path, comma, value), tag(")"))(input)?;
    Ok((input, Exp::Contains(path, val.into())))
}

pub (crate) fn parse(input: &str) -> Result<Exp> {
    let input = Span::new(input);
    let (rest, op) = exp(input).map_err(|err| anyhow!("Could not parse filter: {}", err))?;

    if rest.len() != 0 {
        bail!("Could not parse the complete filter: {}, left over: {}", input, rest)
    }

    Ok(op)
}

pub (crate) fn filter(exp: &Exp, target: &Value) -> Result<bool> {
    let evalued = eval(exp, target);
    let as_bool = evalued.as_bool().unwrap_or(false);
    Ok(as_bool)
}

#[derive(Debug, PartialEq)]
pub struct EPath(Vec<String>);

#[derive(Debug, PartialEq)]
pub enum Exp {
    Equals(EPath, String),
    NotEquals(EPath, String),
    Exists(EPath),
    Not(Box<Exp>),
    And(Vec<Exp>),
    Or(Vec<Exp>),
    Contains(EPath, String)
}

fn descend_to<'a>(path: &EPath, target: &'a Value) -> Option<&'a Value> {
    let mut pointer = path.0.join("/");
    pointer.insert_str(0, "/");
    target.pointer(&pointer)
}

fn string_value(value: &Value) -> Option<String> {
    match value {
        Value::Bool(b) =>
            Some(b.to_string()),
        Value::String(s) =>
            Some(s.to_string()),
        Value::Number(n) =>
            Some(n.to_string()),
        _ =>
            None
    }
}

pub fn eval_equals<'a>(path: &EPath, value: &str, target: &'a Value) -> &'a Value {
    let path_value = descend_to(path, target).unwrap_or(&Value::Null);

    if let Some(p) = string_value(path_value) {
        if p == value {
            &Value::Bool(true)
        } else {
            &Value::Bool(false)
        }
    } else {
        &Value::Bool(false)
    }
}

pub fn eval_not_equals<'a>(path: &EPath, value: &str, target: &'a Value) -> &'a Value {
    if *eval_equals(path, value, target) == Value::Bool(true) {
        &Value::Bool(false)
    } else {
        &Value::Bool(true)
    }
}

pub fn eval_exists<'a>(path: &EPath, target: &'a Value) -> &'a Value {
    let v = descend_to(path, target);

    if v.is_some() {
        &Value::Bool(true)
    } else {
        &Value::Bool(false)
    }
}

fn eval_not<'a>(exp: &Exp, target: &'a Value) -> &'a Value {
    let new_val = eval(exp, target);
    if new_val.as_bool().unwrap_or(false) == true {
        &Value::Bool(false)
    } else {
        &Value::Bool(true)
    }
}

fn eval_and<'a>(conditions: &Vec<Exp>, target: &'a Value) -> &'a Value {
    for cond in conditions {
        let left_val = eval(cond, target);

        if left_val.as_bool().unwrap_or(false) == false {
            return &Value::Bool(false)
        }
    };

    &Value::Bool(true)
}

fn eval_or<'a>(conditions: &Vec<Exp>, target: &'a Value) -> &'a Value {
    for cond in conditions {
        let left_val = eval(cond, target);

        if left_val.as_bool().unwrap_or(false) {
            return &Value::Bool(true)
        }
    }

    &Value::Bool(false)
}

fn eval_contains<'a>(path: &EPath, val: &String, target: &'a Value) -> &'a Value {
    let path_value = descend_to(path, target).unwrap_or(&Value::Null);

    if let Some(p) = string_value(path_value) {
        if p.contains(val) {
            &Value::Bool(true)
        } else {
            &Value::Bool(false)
        }
    } else {
        &Value::Bool(false)
    }
}

pub fn eval<'a>(exp: &Exp, target: &'a Value) -> &'a Value {
    match exp {
        Exp::Contains(ref path, ref val) =>
            eval_contains(path, val, target),
        Exp::Or(ref conditions) =>
            eval_or(conditions, target),
        Exp::And(ref conditions) =>
            eval_and(conditions, target),
        Exp::Not(ref exp) => {
            eval_not(exp, target)
        },
        Exp::Exists(ref path) =>
            eval_exists(path, target),
        Exp::Equals(path, value) =>
            eval_equals(&path, value, target),
        Exp::NotEquals(path, value) =>
            eval_not_equals(&path, value, target)
    }
}

#[cfg(test)]
mod test {

    use serde_json::json;
    use super::*;

    #[test]
    fn pathsegment_test() {
        let i = LocatedSpan::from("mykey0");

        let v = super::path_segment(i).unwrap();

        assert_eq!(v.0.len(), 0);
        assert_eq!(v.1, "mykey0");
    }

    #[test]
    fn path_test() {
        let i = LocatedSpan::from("mykey_0.otherkey1");

        let (rest, m) = super::path(i).unwrap();

        assert!(rest.is_empty());
        assert_eq!(m.0, vec!["mykey_0", "otherkey1"]);
    }

    #[test]
    fn operation_equals_test() {
        let i = LocatedSpan::from("mykey0.otherkey1 == myval");

        let (_, m) = operation_equals(i).unwrap();

        if let Exp::Equals(path, value) = m {
            assert_eq!(path, EPath(vec!["mykey0".to_string(), "otherkey1".to_string()]));

            assert_eq!(value, "myval".to_string());
        } else {
            panic!("Could not match: {:?}", m)
        }
    }

    #[test]
    fn operation_not_equals_test() {
        let i = LocatedSpan::from("mykey0.otherkey1 != myval");

        let (_, m) = super::operation_not_equals(i).unwrap();

        if let Exp::NotEquals(path, value) = m {

            assert_eq!(path, super::EPath(vec!["mykey0".to_string(), "otherkey1".to_string()]));

            assert_eq!(value, "myval".to_string());
        } else {
            panic!("Could not match: {:?}", m)
        }
    }

    #[test]
    fn exp_test() {
        let i = "mykey0.otherkey1 == myval";

        let m = super::parse(i).unwrap();

        if let Exp::Equals(path, value) = m {
            assert_eq!(path, super::EPath(vec!["mykey0".to_string(), "otherkey1".to_string()]));

            assert_eq!(value, "myval".to_string());
        } else {
            panic!("Could not match: {:?}", m)
        }
    }

    #[test]
    fn exp_not_equals_test() {
        let i = "mykey0.otherkey1 != myval";

        let m = super::parse(i).unwrap();

        if let super::Exp::NotEquals(path, value) = m {
            assert_eq!(path, super::EPath(vec!["mykey0".to_string(), "otherkey1".to_string()]));

            assert_eq!(value, "myval".to_string());
        } else {
            panic!("Could not match: {:?}", m)
        }
    }


    #[test]
    fn eval_equals_test() {
        let payload = json!({
            "mykey0": {
                "otherkey1": "myval"
            }
        });

        let i = "mykey0.otherkey1 == myval";
        let m = super::parse(i).unwrap();
        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true))
    }

    #[test]
    fn eval_equals_not_str_test() {
        let payload = json!({
            "mykey0": {
                "otherkey1": 1
            }
        });

        let i = "mykey0.otherkey1 == 1";
        let m = super::parse(i).unwrap();
        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true))
    }

    #[test]
    fn eval_equals_number_test() {
        let payload = json!({
            "mykey0": {
                "otherkey1": 1
            }
        });

        let i = "mykey0.otherkey1 == 1";
        let m = super::parse(i).unwrap();
        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true))
    }

    #[test]
    fn eval_bool_test() {
        let i = "mykey0.otherkey1 == true";
        let m = super::parse(i).unwrap();

        let payload = json!({
            "mykey0": {
                "otherkey1": true
            }
        });

        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true));
    }

    #[test]
    fn eval_equals_no_found_test() {
        let i = "mykey0.otherkey1 == myval";
        let m = super::parse(i).unwrap();

        let payload = json!({
            "mykey0": {
                "randomkey": "myval"
            }
        });

        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(false));
    }

    #[test]
    fn exists_parse_test() {
        let i = "exists(mykey0.mykey1)";
        let m = super::parse(i).unwrap();

        assert_eq!(m, Exp::Exists(EPath(vec!["mykey0".into(), "mykey1".into()])));
    }

    #[test]
    fn exists_eval_test() {
        let i = "exists(mykey0.randomkey)";
        let m = super::parse(i).unwrap();

        let payload = json!({
            "mykey0": {
                "randomkey": "myval"
            }
        });

        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true));
    }

    #[test]
    fn not_equals_eval_test() {
        let i = "not(mykey0.randomkey == myval)";
        let m = super::parse(i).unwrap();

        let payload = json!({
            "mykey0": {
                "randomkey": "myval"
            }
        });

        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(false));

        let i = "not(mykey0.randomkey == myval0)";
        let m = super::parse(i).unwrap();

        let payload = json!({
            "mykey0": {
                "randomkey": "myval"
            }
        });

        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true));
    }

    #[test]
    fn and_eval_test() {
        let i = "and(not(mykey0.randomkey == something), mykey0.randomkey != somethingelse)";
        let m = super::parse(i).unwrap();

        let payload = json!({
            "mykey0": {
                "randomkey": "myval"
            }
        });

        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true));
    }

    #[test]
    fn triple_and_eval_test() {
        let i = "and(not(mykey0.randomkey == something), mykey0.randomkey != somethingelse, mykey0.randomkey == myval)";
        let m = super::parse(i).unwrap();

        let payload = json!({
            "mykey0": {
                "randomkey": "myval"
            }
        });

        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true));
    }

    #[test]
    fn or_eval_test() {
        let i = "or(mykey0.randomkey == something, mykey0.randomkey == myval0)";
        let m = super::parse(i).unwrap();

        let payload = json!({
            "mykey0": {
                "randomkey": "myval"
            }
        });

        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(false));
    }

    #[test]
    fn triple_or_eval_test() {
        let i = "or(mykey0.randomkey == something, mykey0.randomkey == myval0, mykey0.randomkey == myval)";
        let m = super::parse(i).unwrap();

        let payload = json!({
            "mykey0": {
                "randomkey": "myval"
            }
        });

        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true));
    }

    #[test]
    fn contains_test() {
        let i = "contains(mykey0.randomkey, my)";
        let m = super::parse(i).unwrap();

        let payload = json!({
            "mykey0": {
                "randomkey": "myval"
            }
        });

        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true));

        let i = "contains(mykey0.randomkey, somethingelse)";
        let m = super::parse(i).unwrap();
        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(false));
    }

    #[test]
    fn contains_int_test() {
        let i = "contains(mykey0.randomkey, 11)";
        let m = super::parse(i).unwrap();

        let payload = json!({
            "mykey0": {
                "randomkey": 1111
            }
        });

        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true));
    }

    #[test]
    fn not_contains_int_test() {
        let i = "not(contains(mykey0.randomkey, 2))";
        let m = super::parse(i).unwrap();

        let payload = json!({
            "mykey0": {
                "randomkey": 1111
            }
        });

        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true));
    }

    #[test]
    fn index_arrays_test() {
        let i = "mykey.0 == myval1";
        let m = super::parse(i).unwrap();

        let payload = json!({
            "mykey": ["myval1", "myval2"]
        });

        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true));
    }

    #[test]
    fn array_path_exists_test() {
        let payload = json!({
            "mykey": ["myval1", "myval2"]
        });

        let i = "exists(mykey.0)";
        let m = super::parse(i).unwrap();
        let res = super::eval(&m, &payload);
        assert_eq!(*res, serde_json::Value::Bool(true));
    }

    #[test]
    fn filter_test() {
        let payload = json!({
            "mykey0": 1
        });

        let i = "contains(mykey0, 1)";
        let res = filter(i, &payload).unwrap();
        assert_eq!(res, true);
    }

    #[test]
    fn contains_string_non_alphanumeric_test() {
        let payload = json!({
            "mykey": "myval1.something-else"
        });

        let i = "contains(mykey, myval1.something-el)";
        let res = filter(i, &payload).unwrap();
        assert_eq!(res, true);
    }

    #[test]
    fn quoted_values_test() {
        let payload = json!({
            "mykey": "myval1.some()thing-else"
        });

        let i = "contains(mykey, \"some()\")";
        let res = filter(i, &payload).unwrap();
        assert_eq!(res, true);
    }
}

use std::str::FromStr;
use anyhow::Result;
use std::io;

#[derive(Debug)]
pub (crate) struct MultOpt<T : Sized>(pub(crate) Vec<T>);

impl Default for MultOpt<String> {
    fn default() -> Self {
        Self(Vec::new())
    }
}


impl std::fmt::Display for MultOpt<String> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl FromStr for MultOpt<String> {
    type Err = io::Error;

    fn from_str(src: &str) -> Result<Self, Self::Err> {
        if src == "[]" {
            Ok(MultOpt::default())
        } else {
            let v: Vec<String> = src.split(",").map(|v| v.to_string()).collect();
            Ok(MultOpt(v))
        }
    }
}

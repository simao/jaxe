use serde_json::Value;
use crate::Result;
use crate::parser;

pub (crate) trait Filter {
    fn apply(&mut self, line: &Value) -> Result<bool>;
}

pub (crate) struct Filters(Vec<Box<dyn Filter>>);

impl Filters {
    pub (crate) fn from_opts(opts: &crate::Opt) -> Filters {
        let mut filters: Vec<Box<dyn Filter>> = vec![];

        #[cfg(feature = "jq")]
        if opts.jq {
            Self::add_jq_filters(opts, &mut filters);
        } else {
            Self::add_filters(opts, &mut filters);
        }

        #[cfg(not(feature = "jq"))]
        Self::add_filters(opts, &mut filters);

        Filters(filters)
    }

    fn add_filters(opts: &crate::Opt, filters: &mut Vec<Box<dyn Filter>>) {
        for f in &opts.filter {
            let exp = parser::parse(f).unwrap();

            filters.push(
                Box::new(JaxeFilter { filter: exp })
            )
        }
    }

    #[cfg(feature = "jq")]
    fn add_jq_filters(opts: &crate::Opt, filters: &mut Vec<Box<dyn Filter>>) {
        for f in &opts.filter {
            filters.push(
                Box::new(JqFilter { inner: jq_rs::compile(&f).unwrap() })
            )
        }
    }
}

impl Filter for Filters {
    fn apply(&mut self, line: &Value) -> Result<bool> {
        for filter in self.0.iter_mut() {
            let res = filter.apply(line)?;

            if ! res {
                // TODO: Implement debug for filter
                // log::debug!("Line ignored, it does not match filter {:?}", filter);
                log::debug!("Line ignored, it does not match filter");
                return Ok(false)
            }
        }

        Ok(true)
    }
}


#[cfg(feature = "jq")]
struct JqFilter {
    inner: jq_rs::JqProgram,
}

#[cfg(feature = "jq")]
impl Filter for JqFilter {
    fn apply(&mut self, line: &Value) -> Result<bool> {
        let seri = serde_json::to_string(line)?; // TODO: uh
        let res = self.inner.run(&seri).unwrap();

        if res.is_empty() || res == "null\n" || res == "false\n" {
            log::debug!("Line ignored, it does not match jq filter");
            Ok(false)
        } else {
            log::debug!("Line not ignored, jq returns {:?}", res);
            Ok(true)
        }
    }
}

struct JaxeFilter {
    filter: parser::Exp,
}


impl Filter for JaxeFilter {
    fn apply(&mut self, line: &Value) -> Result<bool> {
        parser::filter(&self.filter, line)
    }
}

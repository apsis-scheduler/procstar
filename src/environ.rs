use std::collections::BTreeMap;
use std::vec::Vec;

use crate::spec;

//------------------------------------------------------------------------------

pub type Env = BTreeMap<String, String>; // FIXME: Use OsString instead?

lazy_static! {
    /// Env vars that are not inherited.
    static ref EXCLUSIONS: Vec<&'static str> = {
        let mut x = Vec::new();
        x.push("PROCSTAR_WS_CERT");
        x.push("PROCSTAR_WS_KEY");
        x.push("PROCSTAR_WS_TOKEN");
        x
    };
}

pub fn build<I: Iterator<Item = (String, String)>>(start_env: I, spec: &spec::Env) -> Env {
    let mut env: Env = match &spec.inherit {
        spec::EnvInherit::None => Env::new(),
        spec::EnvInherit::All => start_env.collect(),
        spec::EnvInherit::Vars(vars) => start_env.filter(|(v, _)| vars.contains(v)).collect(),
    };
    for k in EXCLUSIONS.iter() {
        _ = env.remove(*k);
    }
    for (k, v) in spec.vars.iter() {
        if let Some(v) = v {
            env.insert(k.to_owned(), v.to_owned());
        } else {
            _ = env.remove(k);
        }
    }
    env
}

//------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::spec::EnvInherit::*;
    use super::*;

    fn assert_json(json: &'static str, expected: spec::Env) {
        assert_eq!(
            serde_json::from_str::<'static, spec::Env>(json).unwrap(),
            expected
        );
    }

    #[test]
    fn empty() {
        assert_json(
            r#" {} "#,
            spec::Env {
                inherit: All,
                ..Default::default()
            },
        );
    }

    #[test]
    fn inherit_none() {
        assert_json(
            r#" {"inherit": false} "#,
            spec::Env {
                inherit: None,
                ..Default::default()
            },
        );
    }

    #[test]
    fn inherit_vars() {
        assert_json(
            r#" {"inherit": ["HOME", "USER", "PATH"]} "#,
            spec::Env {
                inherit: Vars(vec![
                    "HOME".to_owned(),
                    "USER".to_owned(),
                    "PATH".to_owned(),
                ]),
                ..Default::default()
            },
        );
    }

    #[test]
    fn vars() {
        assert_json(
            r#" {"vars": {"FOO": "42", "BAR": "somewhere with drinks"}} "#,
            spec::Env {
                vars: BTreeMap::from([
                    ("FOO".to_owned(), Some("42".to_owned())),
                    ("BAR".to_owned(), Some("somewhere with drinks".to_owned())),
                ]),
                ..Default::default()
            },
        );
    }

    #[test]
    fn var_remove() {
        let start_env: Vec<(String, String)> = vec![
            ("FOO".to_owned(), "42".to_owned()),
            ("BAR".to_owned(), "17".to_owned()),
            ("BAZ".to_owned(), "mango".to_owned()),
        ];
        let mut vars = BTreeMap::<String, Option<String>>::new();
        // FOO not specified.
        vars.insert("BAZ".to_owned(), Some("pineapple".to_owned()));
        vars.insert("BAR".to_owned(), Option::None);
        vars.insert("BIF".to_owned(), Some("purple".to_owned()));
        vars.insert("BOF".to_owned(), Option::None);

        let env_spec = spec::Env {
            inherit: spec::EnvInherit::All,
            vars,
        };
        let env = build(start_env.into_iter(), &env_spec);

        assert_eq!(env.get("FOO"), Some(&"42".to_owned()));
        assert_eq!(env.get("BAR"), Option::None);
        assert_eq!(env.get("BAZ"), Some(&"pineapple".to_owned()));
    }
}

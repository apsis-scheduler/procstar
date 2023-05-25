use crate::spec;
use std::collections::BTreeMap;

//------------------------------------------------------------------------------

pub type Env = BTreeMap<String, String>; // FIXME: Use OsString instead?

//------------------------------------------------------------------------------

pub fn build(start_env: std::env::Vars, spec: &spec::Env) -> Env {
    start_env
        .filter(|(env_var, _)| match &spec.inherit {
            spec::EnvInherit::None => false,
            spec::EnvInherit::All => true,
            spec::EnvInherit::Vars(vars) => vars.contains(env_var),
        })
        .chain(
            (&spec.vars)
                .into_iter()
                .map(|(n, v)| (n.clone(), v.clone())),
        )
        .collect()
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
                    "HOME".to_string(),
                    "USER".to_string(),
                    "PATH".to_string(),
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
                vars: btreemap! {
                    "FOO".to_string() => "42".to_string(),
                    "BAR".to_string() => "somewhere with drinks".to_string(),
                },
                ..Default::default()
            },
        );
    }
}

use crate::input;
use std::collections::BTreeMap;

//------------------------------------------------------------------------------

pub type Env = BTreeMap<String, String>; // FIXME: Use OsString instead?

//------------------------------------------------------------------------------

pub fn build(start_env: std::env::Vars, input: &input::Env) -> Env {
    start_env
        .filter(|(env_var, _)| match &input.inherit {
            input::EnvInherit::None => false,
            input::EnvInherit::All => true,
            input::EnvInherit::Vars(vars) => vars.contains(env_var),
        })
        .chain(
            (&input.vars)
                .into_iter()
                .map(|(n, v)| (n.clone(), v.clone())),
        )
        .collect()
}

//------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::input::EnvInherit::*;
    use super::*;

    fn assert_json(json: &'static str, expected: input::Env) {
        assert_eq!(
            serde_json::from_str::<'static, input::Env>(json).unwrap(),
            expected
        );
    }

    #[test]
    fn empty() {
        assert_json(
            r#" {} "#,
            input::Env {
                inherit: All,
                ..Default::default()
            },
        );
    }

    #[test]
    fn inherit_none() {
        assert_json(
            r#" {"inherit": false} "#,
            input::Env {
                inherit: None,
                ..Default::default()
            },
        );
    }

    #[test]
    fn inherit_vars() {
        assert_json(
            r#" {"inherit": ["HOME", "USER", "PATH"]} "#,
            input::Env {
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
            input::Env {
                vars: btreemap! {
                    "FOO".to_string() => "42".to_string(),
                    "BAR".to_string() => "somewhere with drinks".to_string(),
                },
                ..Default::default()
            },
        );
    }
}

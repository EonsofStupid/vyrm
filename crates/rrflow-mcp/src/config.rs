use rrd_contract::CanonicalId;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeConfig {
    Embedded {
        database: PathBuf,
        project_root: PathBuf,
    },
    Daemon {
        url: String,
        instance: CanonicalId,
        principal: CanonicalId,
        api_key_file: PathBuf,
    },
}

pub(crate) fn parse_args(
    mut arguments: impl Iterator<Item = String>,
) -> Result<RuntimeConfig, String> {
    let mode = arguments.next().ok_or_else(usage)?;
    let mut values = std::collections::BTreeMap::new();
    while let Some(name) = arguments.next() {
        if !name.starts_with("--") {
            return Err(format!(
                "unexpected positional argument {name:?}; {}",
                usage()
            ));
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("{name} requires one value"))?;
        if values.insert(name.clone(), value).is_some() {
            return Err(format!("duplicate argument {name}"));
        }
    }

    match mode.as_str() {
        "embedded" => {
            reject_unknown(&values, &["--db", "--root"])?;
            Ok(RuntimeConfig::Embedded {
                database: required(&values, "--db")?.into(),
                project_root: required(&values, "--root")?.into(),
            })
        }
        "daemon" => {
            reject_unknown(
                &values,
                &["--url", "--instance", "--principal", "--api-key-file"],
            )?;
            Ok(RuntimeConfig::Daemon {
                url: required(&values, "--url")?.into(),
                instance: CanonicalId::new(required(&values, "--instance")?)
                    .map_err(|error| error.to_string())?,
                principal: CanonicalId::new(required(&values, "--principal")?)
                    .map_err(|error| error.to_string())?,
                api_key_file: required(&values, "--api-key-file")?.into(),
            })
        }
        _ => Err(format!("unknown runtime mode {mode:?}; {}", usage())),
    }
}

fn required<'a>(
    values: &'a std::collections::BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, String> {
    values
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

fn reject_unknown(
    values: &std::collections::BTreeMap<String, String>,
    allowed: &[&str],
) -> Result<(), String> {
    if let Some(name) = values.keys().find(|name| !allowed.contains(&name.as_str())) {
        return Err(format!(
            "argument {name} does not belong to the selected mode"
        ));
    }
    Ok(())
}

fn usage() -> String {
    "usage: rrflow-mcp embedded --db PATH --root PROJECT | rrflow-mcp daemon --url http://LOOPBACK:PORT --instance ID --principal ID --api-key-file ABSOLUTE_PATH".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<RuntimeConfig, String> {
        parse_args(arguments.iter().map(|argument| (*argument).to_owned()))
    }

    #[test]
    fn modes_are_explicit_complete_and_mutually_exclusive() {
        assert!(matches!(
            parse(&["embedded", "--db", "db", "--root", "project"]).unwrap(),
            RuntimeConfig::Embedded { .. }
        ));
        assert!(matches!(
            parse(&[
                "daemon",
                "--url",
                "http://127.0.0.1:9477",
                "--instance",
                "project",
                "--principal",
                "agent",
                "--api-key-file",
                "/run/secrets/rrd"
            ])
            .unwrap(),
            RuntimeConfig::Daemon { .. }
        ));
        for invalid in [
            vec!["--db", "db"],
            vec!["embedded", "--db", "db"],
            vec![
                "embedded",
                "--db",
                "db",
                "--root",
                "project",
                "--url",
                "http://127.0.0.1:9477",
            ],
            vec![
                "daemon",
                "--url",
                "http://127.0.0.1:9477",
                "--instance",
                "project",
                "--principal",
                "agent",
                "--api-key-file",
                "/secret",
                "--db",
                "db",
            ],
        ] {
            assert!(parse(&invalid).is_err(), "accepted {invalid:?}");
        }
    }
}

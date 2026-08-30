use rrd_contract::CanonicalId;
use rrd_engine::RrdEngine;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
enum RecoveryAction {
    Prune {
        evaluated_at: u64,
    },
    Restore {
        backup_sha256: String,
        restore_id: CanonicalId,
        started_at: u64,
    },
}

#[derive(Debug)]
struct Args {
    action: RecoveryAction,
    db: PathBuf,
    authority_instance: CanonicalId,
    state_root: PathBuf,
    policy: PathBuf,
    key: PathBuf,
    estate: CanonicalId,
    instance: CanonicalId,
    at: u64,
    request_id: String,
    operation_id: CanonicalId,
    idempotency_key: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rrd-recovery-controller: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args(std::env::args().skip(1))?;
    let json = match args.action {
        RecoveryAction::Prune { evaluated_at } => {
            serde_json::to_string(&RrdEngine::prune_estate_backups_store(
                &args.db,
                args.authority_instance,
                &args.state_root,
                &args.policy,
                &args.key,
                args.estate,
                args.instance,
                evaluated_at,
                args.at,
                args.request_id,
                args.operation_id,
                args.idempotency_key,
            )?)?
        }
        RecoveryAction::Restore {
            backup_sha256,
            restore_id,
            started_at,
        } => serde_json::to_string(&RrdEngine::restore_estate_backup_store(
            &args.db,
            args.authority_instance,
            &args.state_root,
            &args.policy,
            &args.key,
            args.estate,
            args.instance,
            backup_sha256,
            restore_id,
            started_at,
            args.at,
            args.request_id,
            args.operation_id,
            args.idempotency_key,
        )?)?,
    };
    println!("{json}");
    Ok(())
}

fn parse_args(mut arguments: impl Iterator<Item = String>) -> Result<Args, String> {
    let action_name = arguments.next().ok_or_else(|| usage().to_owned())?;
    let mut db = None;
    let mut authority_instance = None;
    let mut state_root = None;
    let mut policy = None;
    let mut key = None;
    let mut estate = None;
    let mut instance = None;
    let mut at = None;
    let mut request_id = None;
    let mut operation_id = None;
    let mut idempotency_key = None;
    let mut evaluated_at = None;
    let mut backup_sha256 = None;
    let mut restore_id = None;
    let mut started_at = None;
    while let Some(option) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("{option} requires a value"))?;
        match option.as_str() {
            "--db" => db = Some(PathBuf::from(value)),
            "--authority-instance" => {
                authority_instance = Some(canonical(value, "--authority-instance")?)
            }
            "--state-root" => state_root = Some(PathBuf::from(value)),
            "--policy" => policy = Some(PathBuf::from(value)),
            "--key" => key = Some(PathBuf::from(value)),
            "--estate" => estate = Some(canonical(value, "--estate")?),
            "--instance" => instance = Some(canonical(value, "--instance")?),
            "--at" => at = Some(parse_u64(&value, "--at")?),
            "--request" => request_id = Some(value),
            "--operation" => operation_id = Some(canonical(value, "--operation")?),
            "--idempotency" => idempotency_key = Some(value),
            "--evaluated-at" => evaluated_at = Some(parse_u64(&value, "--evaluated-at")?),
            "--backup-sha256" => backup_sha256 = Some(value),
            "--restore" => restore_id = Some(canonical(value, "--restore")?),
            "--started-at" => started_at = Some(parse_u64(&value, "--started-at")?),
            _ => return Err(format!("unknown option {option:?}\n{}", usage())),
        }
    }
    let action = match action_name.as_str() {
        "prune" => {
            if backup_sha256.is_some() || restore_id.is_some() || started_at.is_some() {
                return Err("prune does not accept restore options".into());
            }
            RecoveryAction::Prune {
                evaluated_at: evaluated_at.ok_or("--evaluated-at is required")?,
            }
        }
        "restore" => {
            if evaluated_at.is_some() {
                return Err("restore does not accept --evaluated-at".into());
            }
            RecoveryAction::Restore {
                backup_sha256: backup_sha256.ok_or("--backup-sha256 is required")?,
                restore_id: restore_id.ok_or("--restore is required")?,
                started_at: started_at.ok_or("--started-at is required")?,
            }
        }
        _ => return Err(usage().into()),
    };
    Ok(Args {
        action,
        db: db.ok_or("--db is required")?,
        authority_instance: authority_instance.ok_or("--authority-instance is required")?,
        state_root: state_root.ok_or("--state-root is required")?,
        policy: policy.ok_or("--policy is required")?,
        key: key.ok_or("--key is required")?,
        estate: estate.ok_or("--estate is required")?,
        instance: instance.ok_or("--instance is required")?,
        at: at.ok_or("--at is required")?,
        request_id: request_id.ok_or("--request is required")?,
        operation_id: operation_id.ok_or("--operation is required")?,
        idempotency_key: idempotency_key.ok_or("--idempotency is required")?,
    })
}

fn parse_u64(value: &str, option: &str) -> Result<u64, String> {
    value.parse().map_err(|_| format!("{option} must be u64"))
}

fn canonical(value: String, option: &str) -> Result<CanonicalId, String> {
    CanonicalId::new(value).map_err(|error| format!("{option}: {error}"))
}

fn usage() -> &'static str {
    "usage: rrd-recovery-controller <prune|restore> --db PATH --authority-instance ID --state-root PATH --policy PATH --key PATH --estate ID --instance ID --at UNIX_MS --request ID --operation ID --idempotency KEY [--evaluated-at UNIX_MS] [--backup-sha256 SHA256 --restore ID --started-at UNIX_MS]"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn common(action: &str) -> Vec<String> {
        [
            action,
            "--db",
            "/authority",
            "--authority-instance",
            "estate-control",
            "--state-root",
            "/state",
            "--policy",
            "/policy",
            "--key",
            "/key",
            "--estate",
            "estate-a",
            "--instance",
            "instance-a",
            "--at",
            "200",
            "--request",
            "request-a",
            "--operation",
            "operation-a",
            "--idempotency",
            "key-a",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }

    #[test]
    fn parses_strict_prune_and_restore_coordinates() {
        let mut prune = common("prune");
        prune.extend(["--evaluated-at".into(), "150".into()]);
        assert_eq!(
            parse_args(prune.into_iter()).unwrap().action,
            RecoveryAction::Prune { evaluated_at: 150 }
        );

        let mut restore = common("restore");
        restore.extend([
            "--backup-sha256".into(),
            "a".repeat(64),
            "--restore".into(),
            "restore-a".into(),
            "--started-at".into(),
            "100".into(),
        ]);
        assert!(matches!(
            parse_args(restore.into_iter()).unwrap().action,
            RecoveryAction::Restore { .. }
        ));
    }
}

use rrd_contract::{CanonicalId, EstateBackupMutationResult, EstateMutationResult};
use rrd_estate::{
    public_backup_job, public_snapshot, DesiredPhase, DesiredTarget, EstateRepository,
    LocalEstatePermission, LocalOperatorPolicy, MutationContext, ScheduleBackup, SetDesired,
};
use std::path::PathBuf;
use vyrm_store::PersistentEngine;

enum Action {
    Create,
    SetDesired {
        instance: CanonicalId,
        idempotency_key: String,
        phase: DesiredPhase,
        deployment: CanonicalId,
        version: String,
        configuration_sha256: String,
    },
    ScheduleBackup {
        instance: CanonicalId,
        idempotency_key: String,
        label: String,
    },
}

struct Args {
    action: Action,
    db: PathBuf,
    policy: PathBuf,
    key: PathBuf,
    estate: CanonicalId,
    at: u64,
    request_id: String,
    operation_id: CanonicalId,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rrd-estate-admin: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args(std::env::args().skip(1))?;
    let permission = match &args.action {
        Action::Create => LocalEstatePermission::Create,
        Action::SetDesired { .. } => LocalEstatePermission::SetDesired,
        Action::ScheduleBackup { .. } => LocalEstatePermission::ScheduleBackup,
    };
    let policy = LocalOperatorPolicy::load_json(&args.policy)?;
    let authorization =
        policy.authorize_key_file(&args.key, args.estate.clone(), permission, args.at)?;
    let engine = PersistentEngine::open(&args.db)?;
    let repository = EstateRepository::new(&engine, authorization.estate_id);
    let context = MutationContext {
        at: args.at,
        actor: authorization.operator_id.to_string(),
        request_id: args.request_id,
        operation_id: args.operation_id,
    };
    let result = match args.action {
        Action::Create => {
            let outcome = repository.create_idempotent(&context)?;
            serde_json::to_value(EstateMutationResult {
                estate: public_snapshot(&outcome.document),
                idempotent_replay: outcome.idempotent_replay,
            })?
        }
        Action::SetDesired {
            instance,
            idempotency_key,
            phase,
            deployment,
            version,
            configuration_sha256,
        } => {
            let outcome = repository.set_desired(&SetDesired {
                context,
                instance_id: instance,
                idempotency_key,
                target: DesiredTarget {
                    phase,
                    deployment_ref: deployment,
                    version,
                    configuration_sha256,
                },
            })?;
            serde_json::to_value(EstateMutationResult {
                estate: public_snapshot(&outcome.document),
                idempotent_replay: outcome.idempotent_replay,
            })?
        }
        Action::ScheduleBackup {
            instance,
            idempotency_key,
            label,
        } => {
            let outcome = repository.schedule_backup(&ScheduleBackup {
                context,
                instance_id: instance,
                idempotency_key,
                label,
            })?;
            serde_json::to_value(EstateBackupMutationResult {
                estate: public_snapshot(&outcome.document),
                job: public_backup_job(&outcome.job),
                idempotent_replay: outcome.idempotent_replay,
            })?
        }
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

fn parse_args(mut arguments: impl Iterator<Item = String>) -> Result<Args, String> {
    let action_name = arguments.next().ok_or_else(|| usage().to_owned())?;
    let mut db = None;
    let mut policy = None;
    let mut key = None;
    let mut estate = None;
    let mut at = None;
    let mut request_id = None;
    let mut operation_id = None;
    let mut instance = None;
    let mut idempotency_key = None;
    let mut phase = None;
    let mut deployment = None;
    let mut version = None;
    let mut configuration_sha256 = None;
    let mut label = None;
    while let Some(argument) = arguments.next() {
        let value = required(&mut arguments, &argument)?;
        match argument.as_str() {
            "--db" => db = Some(PathBuf::from(value)),
            "--policy" => policy = Some(PathBuf::from(value)),
            "--key" => key = Some(PathBuf::from(value)),
            "--estate" => estate = Some(canonical(value, "--estate")?),
            "--at" => at = Some(value.parse().map_err(|_| "--at must be u64")?),
            "--request" => request_id = Some(value),
            "--operation" => operation_id = Some(canonical(value, "--operation")?),
            "--instance" => instance = Some(canonical(value, "--instance")?),
            "--idempotency" => idempotency_key = Some(value),
            "--phase" => phase = Some(parse_phase(&value)?),
            "--deployment" => deployment = Some(canonical(value, "--deployment")?),
            "--version" => version = Some(value),
            "--configuration-sha256" => configuration_sha256 = Some(value),
            "--label" => label = Some(value),
            _ => return Err(format!("unknown option {argument:?}\n{}", usage())),
        }
    }
    let action = match action_name.as_str() {
        "create" => {
            if instance.is_some()
                || idempotency_key.is_some()
                || phase.is_some()
                || deployment.is_some()
                || version.is_some()
                || configuration_sha256.is_some()
                || label.is_some()
            {
                return Err("create does not accept desired-state options".into());
            }
            Action::Create
        }
        "set-desired" => {
            if label.is_some() {
                return Err("set-desired does not accept --label".into());
            }
            Action::SetDesired {
                instance: instance.ok_or("--instance is required")?,
                idempotency_key: idempotency_key.ok_or("--idempotency is required")?,
                phase: phase.ok_or("--phase is required")?,
                deployment: deployment.ok_or("--deployment is required")?,
                version: version.ok_or("--version is required")?,
                configuration_sha256: configuration_sha256
                    .ok_or("--configuration-sha256 is required")?,
            }
        }
        "schedule-backup" => {
            if phase.is_some()
                || deployment.is_some()
                || version.is_some()
                || configuration_sha256.is_some()
            {
                return Err("schedule-backup does not accept desired-state options".into());
            }
            Action::ScheduleBackup {
                instance: instance.ok_or("--instance is required")?,
                idempotency_key: idempotency_key.ok_or("--idempotency is required")?,
                label: label.ok_or("--label is required")?,
            }
        }
        _ => return Err(usage().into()),
    };
    Ok(Args {
        action,
        db: db.ok_or("--db is required")?,
        policy: policy.ok_or("--policy is required")?,
        key: key.ok_or("--key is required")?,
        estate: estate.ok_or("--estate is required")?,
        at: at.ok_or("--at is required")?,
        request_id: request_id.ok_or("--request is required")?,
        operation_id: operation_id.ok_or("--operation is required")?,
    })
}

fn required(arguments: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn canonical(value: String, option: &str) -> Result<CanonicalId, String> {
    CanonicalId::new(value).map_err(|error| format!("{option}: {error}"))
}

fn parse_phase(value: &str) -> Result<DesiredPhase, String> {
    match value {
        "running" => Ok(DesiredPhase::Running),
        "stopped" => Ok(DesiredPhase::Stopped),
        "absent" => Ok(DesiredPhase::Absent),
        _ => Err("--phase must be running, stopped, or absent".into()),
    }
}

fn usage() -> &'static str {
    "usage: rrd-estate-admin <create|set-desired|schedule-backup> --db PATH --policy PATH --key PATH --estate ID --at UNIX_MS --request ID --operation ID [--instance ID --idempotency KEY] [--phase running|stopped|absent --deployment ID --version VERSION --configuration-sha256 SHA256] [--label LABEL]"
}

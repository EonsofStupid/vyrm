# RRD local process driver v1

Status: F3 alpha driver. The typed driver, bounded managed-child shutdown, real
controller crash matrix, and Linux/Windows/macOS qualification are implemented.
Packaging, an operator mutation API, bounded diagnostic-log retention, and
per-instance backup/restore remain open.

## Trust and launch boundary

`LocalDeploymentCatalog` is an operator-trusted, strict JSON document capped at
one MiB. Each entry binds a canonical deployment ID and version to:

- an already-canonical absolute executable path;
- the executable's exact SHA-256;
- typed arguments (`literal`, instance ID/root/path, desired version, or
  configuration digest); and
- a bounded explicit environment.

The driver authenticates the executable before each start. It uses
`std::process::Command` with one argument per value, clears the inherited
environment, and never invokes a shell. Windows `.bat` and `.cmd` targets are
denied because they can cross an implicit command-shell parsing boundary. This
follows Rust's documented `Command::arg/args` behavior: arguments are passed
literally and shell expansion has no effect. On Windows, the driver restores
only the host `SystemRoot` after clearing the environment because Winsock cannot
initialize its installed providers without that platform location; catalogue
variables remain the only other inherited launch state.

`rrd-deployment-catalog --output PATH` resolves the installed sibling
`rrd-server` by default, canonicalizes and hashes the executable, and emits the
validated RRD argument and graceful-shutdown template. `--server` and
`--version` are explicit overrides for staged upgrades. Publication uses a
synced owner-private staging inode and an atomic create-new hard link, so an
existing target is never replaced and an interrupted partial file is never the
catalogue. This is an operable catalogue surface, not yet an OS installer or
service manager.

Instance-relative paths reject absolute paths, parents, roots and platform
prefixes. Every instance receives a dedicated directory beneath the configured
absolute state root. The initial `delete` behavior stops the verified process
but deliberately retains the instance directory and database. Data erasure
waits for F3 retention/backup jobs and F4 authorization.

## Restart identity and signals

After spawn, the driver must discover the process executable and start time and
survive a bounded startup-stability interval, then durably replace an
owner-private process record before returning success. Per-instance stdout and
stderr files retain startup evidence; an early exit includes the bounded tail
of stderr in the retryable error. If discovery or record persistence fails, the
still-owned child is killed and waited before an error is returned.

A reopened controller treats a process as owned only when all three values
match the record:

1. PID;
2. process start time; and
3. canonical executable path.

The process record additionally binds operation ID, deployment/version and
configuration digest. A start retry with the same operation ID returns the same
effect evidence and preserves the process. Restart/upgrade stops the verified
old identity before spawning the new target. Stop/delete fail closed if the PID
now identifies a different start or executable; the driver never signals that
process. Cross-platform inspection and kill use `sysinfo`; its process API
provides PID, executable, start time and the portable kill operation.

Process records are written with create-new staging, file sync, rename and
parent-directory sync on Unix. Successful removal is also parent-synced.

## Managed-child shutdown

The deployment catalogue can select immediate termination or a paired direct
instance-file contract. RRD deployments use the latter. The driver durably
creates `SHUTDOWN.REQUEST`, waits only for the declared bounded interval, and
accepts a graceful exit only when `SHUTDOWN.COMPLETE` exists. RRD drains Axum,
syncs the completion marker, and then exits. If the deadline expires, the
driver reauthenticates PID, start time, and executable before using the portable
kill fallback. It never fabricates graceful completion, and neither path erases
the instance data directory.

## Current evidence

The `rrd-server` integration test uses the real built server executable. It:

1. persists desired running state;
2. drops/reopens the engine and driver between lease, prepared and applied;
3. starts the RRD child and authenticates its process record;
4. recreates the driver and replays the same operation without changing PID;
5. reopens through observed and completed;
6. writes a second desired stopped generation;
7. reopens through lease/prepared/applied, actually kills the child, then
   records stopped/completed; and
8. verifies the RRD data directory was not deleted.

A second test forges the current test PID with the wrong start identity and
proves the driver returns a permanent failure while leaving that process and
record untouched.

A separate one-step `rrd-estate-controller` executable lets the black-box test
harness kill the actual control-plane process after leased, prepared, applied,
observed and completed transitions for both start and stop. It also holds and
kills the controller after the real external effect but before the `applied`
record. Start resumes with the same child PID; stop resumes after observing the
child already gone, retains its data directory, and completes exactly once.
These debug-only hold points are rejected in release builds.

GitHub Actions run
[`32667681611`](https://github.com/EonsofStupid/vyrm/actions/runs/32667681611)
passes the persistent authority, RRD process contracts, real child/controller
recovery, and strict-clippy steps on Ubuntu, Windows, and macOS. Its aggregate
job also passes all workspace tests and clippy, controlled evaluation evidence,
dependency boundaries, and offline-edge binary budgets.

This qualifies the current local process and controller crash boundary across
the three native desktop/server OS families. It is not the complete F3 product:
installer/service packaging, authorized mutations, per-instance backup/restore,
and a bounded rotation/retention policy for the diagnostic logs remain open.

## Primary implementation references

- Rust [`std::process::Command`](https://doc.rust-lang.org/std/process/struct.Command.html)
- [`sysinfo::Process`](https://docs.rs/sysinfo/latest/sysinfo/struct.Process.html)
- Microsoft [`SystemRoot` environment variable`](https://learn.microsoft.com/windows/deployment/usmt/usmt-recognized-environment-variables)

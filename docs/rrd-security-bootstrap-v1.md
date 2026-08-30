# RRD security bootstrap v1

Status: executable local/Kubernetes-init provisioning boundary.

`rrd-security-bootstrap` initializes the first persistent `rrd-security`
authority for a new instance. It is deliberately separate from `rrd-server` so
the network listener never has an unauthenticated bootstrap mode.

## Invocation

```text
rrd-security-bootstrap \
  --db /var/lib/rrd/project/.rrflow/rrd \
  --instance project-a \
  --manifest /etc/rrd/bootstrap/bootstrap.json \
  --at-unix-ms 1787529600000
```

The manifest is bounded to one MiB and uses this strict shape:

```json
{
  "format_version": 1,
  "revision": 1,
  "principals": [
    {
      "id": "operator",
      "kind": "service",
      "credential_file": "/etc/rrd/bootstrap/operator.key",
      "not_before_unix_ms": 1,
      "expires_at_unix_ms": 18446744073709551615,
      "grants": [
        {
          "action": "session_create",
          "resource_prefix": {
            "segments": [{ "kind": "instance", "id": "project-a" }]
          }
        }
      ]
    }
  ]
}
```

`credential_file` must be an absolute path. The command permits Kubernetes's
in-directory projected-secret symlinks but rejects a resolved path that escapes
the declared mount directory. Credential files are bounded to 64 KiB, must be
non-empty regular files, and on Unix must deny all other access and group
write/execute (owner-only or Kubernetes `fsGroup` read is admitted). Only their
SHA-256 digests enter the persistent principal state.

## Replay and drift

The complete materialized `SecurityState` is validated before storage opens.
An absent authority is initialized through the authenticated RRFlow control
transition. Repeating the identical manifest and credential bytes reports
`unchanged`; any policy or credential drift fails without mutation. The
bootstrap manifest digest supplies deterministic request/operation identities,
while neither raw credential bytes nor their paths enter the stored authority.

This is initial provisioning, not ongoing administration. Principal rotation,
authorized policy mutation APIs, external secret providers, certificate-to-
principal binding, and organization-wide policy remain F4 work.

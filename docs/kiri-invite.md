# Experimental Linux member invitation

`moosuri kiri-invite` is a root-only, explicitly experimental local operator operation. It reserves a stable identity, initializes a private member Store in a dedicated nonroot child, reconciles the exact Kiri profile, checks the owner's socket and kernel UID, and only then activates the route. It does not prepare or format storage, pull images, install host services, or send a credential.

Retain the original `--request-id` and exact profile across retries. The installation registry and operations/UID reservation directories must already exist as root-owned mode0700 canonical paths under trusted, nonwritable root ancestors. `--mounts-root` must be traversable by the dedicated UID. The executable must be root-owned, not setuid/setgid or group/world writable, and installed below trusted root-owned ancestors (for example `/usr/local/bin`).

```sh
sudo /usr/local/bin/moosuri kiri-invite --experimental \
  --request-id "$INVITATION_REQUEST_ID" --name "$MEMBER_NAME" --email "$MEMBER_EMAIL" \
  --uid "$DEDICATED_UID" --image "$LOCAL_SHA256_IMAGE_ID" \
  --registry-directory /srv/moosuri/registry \
  --operations-root /srv/moosuri/invite-operations \
  --allocations-root /srv/moosuri/uid-allocations \
  --mounts-root /srv/moosuri/mounts --ipc-root /srv/moosuri/ipc \
  --max-volume-bytes 10737418240
```

An invitation reserved by the installation administrator UI must retain its normalized name and email. Conflicting or unrelated administrator operation requests are rejected before private initialization or credential input. A raw operator reservation can use the same CLI without a UI invitation.

The first call can return `stage: preparation_required` with a stable allocation and exact paths. Prepare a separate bounded writable filesystem at `<mounts-root>/<volume-id>` with `nodev,nosuid,noexec`; its total capacity must not exceed `--max-volume-bytes`. Create `member-data`, `scratch`, and `ipc` on it, each owned by the dedicated UID/GID with mode0700, then bind its `ipc` directory onto `<ipc-root>/<account-id>`. The private data root accepts only empty storage or the exact existing owner layout. The CLI validates mounts and reserves exclusive host UID/path ownership before opening a private Store.

If `--google-sockets-root` is supplied, prepare exactly the account's root-owned0711 directory containing only its UID-owned0600 `google.sock`; it is mounted at `/google:ro`. The optional model broker uses the corresponding existing profile rules. For member-local Gmail, add `--enable-local-gws` and use the [member GWS relay and registration](../deployment/kiri/google.md). Keep these roots and the image identical on every retry.

After preparation, a noninteractive call without password input returns `password_required`. Run interactively to enter a temporary password without echo, or supply one bounded newline-terminated password on a private pipe with `--password-stdin`. Never put it in arguments, environment, operation metadata, tickets, or shell history. Deliver the temporary credential privately through an operator-controlled channel. The first member login requires a password change.

A retry checks initialization in the dedicated UID child. If initialization committed before an interrupted response, it continues without reading or replacing the password. A ready retry reconciles and verifies the same runtime without credential input. `service_waiting` and `initialization_waiting` retain the same allocation for a later retry. Fenced, suspended, mismatched, or conflicting requests cannot be activated by this operation. This command does not stop an already-running fenced process.

Root-owned operation records contain only immutable identity/profile and a progress stage. The root operator never opens the private SQLite Store; the child clears inherited environment/groups, drops to the dedicated UID/GID, enables no-new-privileges, and receives the password only on anonymous stdin. The command emits metadata and fixed status/reason codes only.

[Linux synthetic invitation and interruption observations](../deployment/kiri/linux-vm-smoke/invite-evidence.json)

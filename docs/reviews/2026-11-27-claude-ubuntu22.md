# Claude preparation on Ubuntu22 — verified compatibility

This unit expands I07/M05/M06 on a retained marked Ubuntu22.04.5 guest,
kernel5.15.0-203, Docker29.1.3 and shadow passwd/uidmap4.8.1-2ubuntu2.2.
It is an upgrade compatibility check, not a change to the public Ubuntu24
installer requirement or a claim about the exact production kernel. Production
remains dee84754. No account authentication, provider grant or inference is used.

| ID | Journey | Current result |
| --- | --- | --- |
| J22-01 | Required rootless tools are absent. | Refused before artifact downloads, account or control-directory creation. Passed. |
| J22-02 | Native tools installed; stock login.defs omits subordinate-ID keys. Prepare and replay one owner. | Original candidate failed before preparation. Corrected candidate prepared the rootless engine with UID/GID10000 and native subordinate ranges; exact replay passed. |
| J22-03 | Invalid explicit allocator settings, missing databases, wrong owner or changed request. | Fifteen focused Linux tests passed, including omitted defaults, empty databases and refusal before allocation/download; actual owner/request refusal also passed. |
| J22-04 | Native isolation and byte/inode capacity exhaustion, then recovery. | Native confinement, byte and inode exhaustion, retained data and post-exhaustion readmission passed. |
| J22-05 | Second owner and altered worker service. | Separate service identities/storage, denied sibling-file access, altered-unit refusal and same-owner recovery passed. |
| J22-06 | Actual guest reboot with both prepared owners. | Changed boot ID observed; both owner descriptors persisted exactly and native isolation passed again. |

The retained guest initially lacked uidmap, rootlesskit, slirp4netns and
fuse-overlayfs. Installing these public package prerequisites added five packages
including libslirp0, with zero upgrades/removals. No OS upgrade or login.defs edit
was used. Its older useradd does not offer `--add-subids-for-system`.

The initial product check incorrectly required all six SUB_UID/GID keys to be
explicitly present. Ubuntu22's shipped [useradd manual](https://manpages.ubuntu.com/manpages/jammy/man8/useradd.8.html)
documents native defaults of100000/600100000/65536 when the subordinate-ID
databases exist. The source now recognizes those omitted defaults while leaving
allocation and locking to useradd. Actual allocated ranges still pass the existing
owner, range, overlap and host-ID checks before use.

Explicit malformed, duplicate, trailing-token, too-small or overflowing settings
remain refused. Decimal-only parsing avoids interpreting a shadow octal value as
decimal. Both databases must be existing trusted regular files; empty files are
valid. The first focused run exposed rejection of empty databases (13 pass/2 fail),
also found by the independent review. After correction, all15 cases and the native
build passed. The new source does not write the allocator configuration.

Official Claude Sonnet5.5/medium reviewed the change and its corrections in7/6
turns. The reviews were static source inspection, not execution. No material defect
remained in this change. A suggestion about stricter parsing of pre-existing
root-written subid rows is outside this correction; it was not used to broaden
the product change or imply additional validation.

The next actual run reached the worker artifact check and correctly rejected the
test operator's0700 parent directory around the public candidate binary. Its parent
was changed to0755; the same request and identified account were retained for the
retry. This harness staging error is recorded separately from the product defect.

Candidate SHA-256:
`880e74fdcd4b6bb6b0a1433984adc42fb5bc9ae6964b4a0d68b8e423c902e227`.
The existing `claude-preparation.py` executes the actual preparation, private-file
and isolation journeys. Earlier failures remain in its report. Host disk pressure
was addressed by retaining the exact candidate and cleaning only this task's idle
moosuri package build artifacts; no member data or VM disk was removed.

The next attempt completed product preparation but failed while recording the
result: Python3.10 lacks `hashlib.file_digest`. The harness now streams SHA-256
with the standard APIs available on both supported Ubuntu versions. The next
run retained the original request/account and passed initial preparation, replay,
owner refusal, native confinement and both capacity limits.

The final initial run completed9 checks, then an actual guest reboot completed3
more: exact owner/filesystem replay, native admission and the second owner replay.
All12 passed on the same candidate, while the4 preceding failed entries remain
in the report. After reboot the stock allocator keys are still omitted; the
product did not require editing login.defs. These results close J22-01–J22-06
within the stated credential-free scope. Actual subscription authentication,
model inference and final-source installation/production rollout remain open.

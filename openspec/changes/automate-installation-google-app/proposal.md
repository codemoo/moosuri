# Automate installation Google app setup

An administrator's Google connection currently ends after vault-backed Kiri
sign-in when no installation OAuth app exists. Requiring an external Cloud Shell
or a downloaded client file does not satisfy the requested connection flow.

The baseline is a person installing the public project on a clean Linux host.
No prior Cloud project, OAuth client, billing account, GWS configuration, private
Kiri skill, developer home directory or pre-enrolled member may be assumed.
The public installer currently installs a different runtime; completing the
owner's existing installation alone does not meet this requirement.

Continue in the same isolated Kiri browser through the official Cloud Console,
register the installation web client directly, wait for its existing member GWS
distribution, and continue the initiating member's consent. Reuse that app for
other members while keeping their Google credentials and grants separate.

The Console, Google verification and organization restrictions remain real
external boundaries. An incomplete provider step must be resumable and must not
be reported as a connected account.

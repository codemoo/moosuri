# Deliver separate signed native components

P8/P10 requires optional trusted-native companion and independent vault packages for
self-host users who choose those capabilities. Add separate purpose-tagged, fixed-file
signed component manifests, offline packager evidence and explicit HTTPS download/verify.
Recipients can select prebuilt executables without compiling; distribution authentication
must not imply device pairing, independent deployment or authenticated build execution.

Preserve the assistant installer/update contract and native credential protocols/adapters.
Official authority/artifact publication, legal/platform release gates, automated service
installation, private state upgrades and broader P8/P10 completion remain separate.
References: P8/P10 in docs/plan.md, credential/vault contracts in docs/features.md,
docs/native-credential-github.md and docs/vault-service-client.md.

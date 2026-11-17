# Reliable installation and local controls

Make the existing one-line installation safe to interrupt and easy to recover,
without adding another updater, settings app or diagnostic upload service.
Homebrew's explicit changes, Ollama's working CLI check and Docker Desktop's
ready-state controls and recovery entry point inform the product choices.

Scope: atomic initial runtime publication, durable setup choices, serialized
lifecycle changes, running foreground recognition, bounded readiness/browser
opening and actionable local menu diagnostics. Retain signed stopped updates,
account isolation and data-preserving removal. See docs/reviews/2026-11-17-installation-reliability.md.

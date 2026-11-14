# Incoming PWA photos, files and audio

Extend the implemented F16/P7 one-use text handoff to supported image, document and
audio inputs. Reuse account/session/original-thread fences, media uploads and explicit
speech conversion. Do not create another durable upload queue or automatic chat send.
Actual mobile OS share dispatch and provider/device acceptance remain separate gates.

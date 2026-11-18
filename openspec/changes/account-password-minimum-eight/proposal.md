# Account password minimum of eight characters

## Why
The account setup minimum should accept passwords of eight characters consistently in the terminal, server and web form.

## What Changes
Use a shared Rust enrollment predicate for at least eight Unicode scalar values and at most256 UTF-8 bytes. Match it in the React change form and bilingual setup guidance. Keep existing hash authentication, generated temporary credentials, expiry, rate limits, session revocation and the separate vault password policy.

## Impact
Account bootstrap, password changes and recovery enrollment; no schema migration. Existing shorter Unicode passwords continue to authenticate. R03/R04 and F02 account isolation remain enforced.

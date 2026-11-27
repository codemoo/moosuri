# First Google setup entry

The existing Linux connection controller already prepares a missing installation
app after Vault selection and fresh Google login. The separate settings page
still led only with client JSON upload, making a manual prerequisite appear
mandatory. This correction directs the installer back to the existing service
connection; manual import remains a secondary collapsible option.

The unconfigured action has a distinct callback. It does not call the existing
configured callback, issue a provider request or mark the installation ready.
The selected service/conversation intent survives; an entry without a selected
service returns to the chooser and does not default to Gmail. Existing import
validation, private input clearing and account-switch fences remain intact.

| ID | User journey | Verification |
| --- | --- | --- |
| GE01 | First installer opens settings without JSON | Primary continuation, closed manual section and no POST/configured claim passed Linux UI checks |
| GE02 | Installer returns to selected Drive before configuration | Same service and enabled Google connection, no automatic consent/read or false ready status passed Linux UI checks |
| GE03 | Installer enters from root settings | Service chooser, no default Gmail, provider POST or saved read passed Linux UI checks |
| GE04 | Installer explicitly imports an existing client | Open manual option, callback refusal, valid scoped upload and input clearing passed Linux UI checks |
| GE05 | Existing setup or a different account opens the page | Configured view and late-file/account fences passed Linux UI checks; ordinary-member browser visibility is checked separately |

Linux UI tests passed24. TypeScript/build initially rejected an unsupported
`exact` test-locator option added during main review; it was removed, and the
corrected build/typecheck passed. This was a test typing error, not a product
request failure. Actual Chromium145 passed B-GWS-01/02/05/06/07 at390px and1440px: manual
import, configured return, first service chooser, selected Drive/reload with an
enabled connection button, and ordinary-member administration visibility. All10
runs passed; main inspected mobile/desktop screenshots. API responses are
synthetic and cannot prove Google authentication. The existing browser harness
also needed its retired manual-import button changed to the current details
control and unconfigured Google status represented as404.

Independent official Claude Sonnet5.5/medium reviewed the frozen product diff.
Its potential service-view dead end was checked against the existing unconfigured
admin branch and the test now asserts the returned Google-connect button is
enabled. The selected-service label now says it navigates to the connection.
No new model, authentication transport or provider fallback was added.

This UI correction does not close O01–O07, real member consent or conversational
Gmail. Those remain tracked in the [primary journeys](../../openspec/changes/automate-installation-google-app/journeys.md).

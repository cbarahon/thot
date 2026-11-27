# Google Drive OAuth: developer and release setup

Thot's intended distribution model uses **one OAuth Desktop client registered
by the Thot developer**, with each professor independently consenting to access
their own Drive. The developer creates the Google Cloud project; professors do
not. The client identifies Thot, whereas each professor's access and refresh
tokens authorize that professor's account. A developer's own tokens cannot
authorize another person's Drive.

Builds can bundle a Desktop client using `THOT_GDRIVE_CLIENT_FILE` pointing to Google's JSON, or the `THOT_GDRIVE_CLIENT_ID` and `THOT_GDRIVE_CLIENT_SECRET` build environment variables. A single JSON file in the ignored `secrets/` directory is detected locally. GitHub releases build on GitHub and require repository Actions secrets named `THOT_GDRIVE_CLIENT_ID` and `THOT_GDRIVE_CLIENT_SECRET`; local files are not transferred. Runtime environment variables override bundled values. Professor tokens stay private. Live OAuth and Drive transfer still require qualification.

## Development and testing: fastest pilot

1. Sign in to the [Google Cloud console](https://console.cloud.google.com/)
   with the developer account. Create a dedicated **test** project and select
   it. Enable the **Google Drive API** under APIs & Services → Library.
2. Under Google Auth platform, configure the consent screen. Choose **External**
   for a pilot involving other Google accounts; keep its publishing status in
   **Testing**. Enter the requested app name, support and contact details.
   A Google Cloud project in **Personal** mode can be used by its creator only;
   it cannot authorize a different professor and cannot later be converted to
   External. An eligible Workspace Internal app is limited to its organization.
3. In **Audience → Test users**, add the exact Google account of every pilot
   professor, including your own account if you are testing it. Save. Under
   **Data Access**, configure the scope
   `https://www.googleapis.com/auth/drive.readonly` as applicable to the
   External app. This scope can read all Drive files visible to the user, not
   just the chosen folder. Thot uses it to discover existing and future direct
   children; it currently ignores nested folders.
4. In **Clients**, create an OAuth client of type **Desktop app**. Record its
   client ID and client secret. Do not use a web, device, service-account or
   Personal-mode client for these current instructions. Bundle the client when building Thot as described above.
   Do not give them your personal access or refresh token.
5. Ask the professor to authorize as the added test user, list a folder and
   download one test file. Then test a full run and repeat after token expiry.
   Record success or failure without sharing client secrets, authorization URLs,
   tokens or student data. See the [OVA trial](../../examples/ova-screening-kvm-alpine/README.md).

This does not require production verification before an added test user can
try the flow, but Google may show an unverified-app warning or block access;
there is no guarantee of automatic approval. External Testing authorization,
including refresh tokens for this scope, expires after seven days; repeat
consent when required. Adding a test user is a developer task, not something the
professor can bypass. A Workspace administrator may block the app or scope even
for an added test user. A personal Google account avoids that organization's
account policy, but must be appropriate for the data. See Google's [consent
setup](https://developers.google.com/workspace/guides/configure-oauth-consent),
[audience and token lifetime](https://support.google.com/cloud/answer/15549945?hl=en),
and [OAuth troubleshooting](https://developers.google.com/workspace/drive/activity/v2/troubleshoot-authentication-authorization).

## Production: later work

Register and maintain a production External OAuth app under an appropriate
Thot owner. Configure its branding, support contacts, privacy policy and exact
scope, and follow Google's verification and publishing process before relying
on it for general use. `drive.readonly` is a **restricted** scope; Google may
require scope justification and, depending on the application and data flow,
a security assessment. Confirm current requirements and costs with Google;
verification and Workspace approval are not guaranteed. Do not publish the
test client as a substitute for production readiness. See Google's [Drive
scope guide](https://developers.google.com/workspace/drive/api/guides/api-specific-auth)
and [verification guidance](https://support.google.com/cloud/answer/13463073?hl=en).

The release supplies the registered Desktop client as an
application default, while retaining a documented override for development or
self-hosted clients. Configure GitHub Actions secrets before publishing. Google treats installed applications as public clients: a
distributed client secret can be recovered whether it is in a binary or a
shipped configuration file. It identifies the app and must not be relied on
as a confidential server secret. Follow Google's credential-handling policies
and keep undistributed credentials private. See [Google's desktop OAuth
guidance](https://developers.google.com/identity/protocols/oauth2/native-app)
and [OAuth policies](https://developers.google.com/identity/protocols/oauth2/policies).

Keep individual professor tokens private and outside project configuration,
source control, images, logs, results and guests. Plan client rotation and
re-consent: the current cache rejects a different configured client ID and a
refresh token is tied to the client that obtained it. The existing override
uses environment variables; automatic rotation, account selection and
any guided login command are **not implemented**. No developer token should be
embedded or shared. Verification does not override an institution's app policy.

## Scope and headless tradeoffs

`drive.file` restricts access to files selected or created with the app, but
also permits modifying those files. Google documents selecting folders, but does
not establish that selecting one automatically authorizes every existing and
future descendant. A switch would need a Picker flow and live testing, and
cannot be assumed to preserve the current folder-inventory workflow. The
current `drive.readonly` choice is read-only but broad across the account.
See [Drive scopes](https://developers.google.com/workspace/drive/api/guides/api-specific-auth)
and [desktop Picker](https://developers.google.com/workspace/drive/picker/guides/desktop-mobile-picker).

Today the callback listens on a random localhost port on the machine running
the interactive command. For a headless host, authorize on a desktop using the
same client, then securely transfer the token cache to the host and protect it;
for a container, use persistent private storage. This is a manual workaround,
not a built-in transfer flow. A browser on another machine cannot reach the
headless host's localhost without a correctly configured tunnel or port
forwarding. Thot has no guided tunnel or fixed-port container workflow today.
Google's device-code flow supports `drive.file` but **not** `drive.readonly`,
so it cannot replace the current flow without changing the scope and file
selection model. A browser is still needed somewhere for consent. See
[Google's device flow](https://developers.google.com/identity/protocols/oauth2/limited-input-device).

## Pilot acceptance checks

- Use a separate test account and folder with sample files. Confirm the user
  can consent, list the expected names and download a selected file; compare a
  trusted digest. Verify a second user gets a separate token and cannot use the
  first user's cache as their own identity.
- Test duplicate names, an added file, refresh, revoked consent, an expired
  Testing authorization, network interruption and insufficient storage. Check
  errors and staged-file cleanup. Do not count local-only or synthetic OVA
  tests as live Drive evidence.
- Run two sample students sequentially, inspect provenance and guest transfer,
  and confirm the original Drive files remain. Check that credentials are
  absent from guests and results, and delivery bytes are absent from results.
- Record app/client version, test conditions, exact non-secret errors and
  outcomes. Large OVA transfer and Shared Drives require separate qualification.

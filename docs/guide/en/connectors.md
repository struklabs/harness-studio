# Connectors

**What you do here:** connect your own Gmail, Google Drive, Google Calendar, and Excel (Microsoft Graph) accounts so the Playground and agents can use them as tools, and register the OAuth client IDs that makes possible.

Open **Settings > Tools > Connectors**. Connectors are off until you add an OAuth client ID and connect an account.

## What Each Connector Offers

| Connector | Read and search tools | Tools that change data (write access only) |
| --- | --- | --- |
| Gmail | `gmail_search`, `gmail_read` | `gmail_send` |
| Google Drive | `drive_search`, `drive_read` | `drive_create_text_file` |
| Google Calendar | `calendar_list_events` | `calendar_create_event` |
| Excel (Microsoft Graph) | `excel_find_workbooks`, `excel_read_range` | `excel_write_range` |

Read and search tools follow your tool policy like any other network tool. Tools that send or write exist only if you grant **write access** when you connect, and they are dangerous-class tools, so each call asks for your approval unless you chose a policy that allows it. Calendar events never include attendees and never send invitations. Drive can only create new text files and never edits existing ones.

Mail, files, and cells are written by other people. Results are marked as untrusted content; a model should treat them as data and never as instructions.

## Privacy

- Sign-in uses OAuth 2.0 with PKCE and a redirect to this computer (`127.0.0.1` or `localhost`). Harness never sees your password.
- Access and refresh tokens are stored only in your operating system keychain (Windows Credential Manager, macOS Keychain, or Secret Service). They are never written to a settings file or logged.
- Requests go straight from this computer to Google or Microsoft. There is no Harness server in between.
- Content a tool returns is given to the model you are chatting with. If that is a cloud model, the content leaves your computer.
- **Disconnect and Revoke** revokes the token at Google and deletes the tokens from this computer. Microsoft offers no revoke call for this flow, so Harness only deletes the local tokens; also remove Harness Studio under your Microsoft account's app permissions.

## Connect an Account

1. Register an OAuth client ID (below) and save it under **OAuth Client IDs**.
2. Under the connector, press **Connect**.
3. Read the consent text. Tick **Also allow actions that change data** only if you want the write tools.
4. Tick the confirmation and press **Continue in Browser**. Approve the access in the provider's page. You have five minutes, and **Cancel** stops the wait.

If the provider grants fewer permissions than requested, Harness only offers the tools those permissions allow.

## Register OAuth Client IDs

Harness ships no client ID or secret. You (or the owner of the installation) create them in the provider's console. You can paste them under **Settings > Tools > Connectors > OAuth Client IDs**, or set environment variables before starting Harness Studio:

| Value | Setting | Environment variable |
| --- | --- | --- |
| Google client ID | Google Client ID | `HARNESS_GOOGLE_OAUTH_CLIENT_ID` |
| Google client secret (optional) | Google Client Secret | `HARNESS_GOOGLE_OAUTH_CLIENT_SECRET` |
| Microsoft client ID | Microsoft Client ID | `HARNESS_MICROSOFT_OAUTH_CLIENT_ID` |

A saved client ID wins over the environment variable. The client ID is not secret and is stored in the settings folder; the Google client secret, if you add one, goes to the keychain.

### Google (Gmail, Drive, Calendar)

1. Open the Google Cloud Console and create or select a project.
2. Under **APIs and Services > Library**, enable the **Gmail API**, **Google Drive API**, and **Google Calendar API** for the connectors you want.
3. Under **APIs and Services > OAuth consent screen**, set up the app. For personal use, keep it in **Testing** and add your own Google account under **Test users**. Add the scopes below. Google's verification applies only if you publish the app to other users.
4. Under **APIs and Services > Credentials**, choose **Create Credentials > OAuth client ID** with the application type **Desktop app**. Desktop clients accept the loopback redirect without further setup.
5. Copy the **Client ID**. Google also shows a **Client secret** for desktop clients; Google requires it at the token endpoint even with PKCE. Paste it into **Google Client Secret (Optional)** so it goes to the keychain.

Scopes requested: read: `gmail.readonly`, `drive.readonly`, `calendar.readonly`; write (only if you tick write access): `gmail.send`, `drive.file`, `calendar.events`.

### Microsoft (Excel)

1. Open the Microsoft Entra admin center, then **App registrations > New registration**.
2. For **Supported account types**, choose the option that includes personal Microsoft accounts if you want to use one.
3. Under **Authentication > Add a platform**, choose **Mobile and desktop applications** and add the redirect URI `http://localhost`. Microsoft accepts any port for a loopback redirect. Turn on **Allow public client flows**.
4. Under **API permissions**, add the delegated Microsoft Graph permissions `Files.Read` and `offline_access`, and `Files.ReadWrite` if you want write access.
5. Copy the **Application (client) ID** into **Microsoft Client ID**. A public client has no secret.

Scopes requested: `Files.Read` and `offline_access`; with write access also `Files.ReadWrite`. Excel tools work on `.xlsx` workbooks stored in the connected OneDrive.

## Troubleshooting

- **The browser says the redirect URI does not match:** register the redirect URI exactly as shown under **OAuth Client IDs** and use a Desktop or public client type.
- **"API not enabled" or "access denied" in a tool result:** enable the API for your Google project, or check that your account is a test user.
- **A tool says the sign-in expired:** press **Disconnect and Revoke**, then connect again. Google refresh tokens of apps in **Testing** expire after seven days.
- **The keychain is unavailable:** on Linux, start a Secret Service provider (for example GNOME Keyring). Without a keychain Harness refuses to store tokens.

See also [Tools](06-tools.md) and [GitHub Discovery](github-discovery.md).

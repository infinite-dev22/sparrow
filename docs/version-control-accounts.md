# Version Control Accounts

Open **Settings → Version Control** to add an account for a remote host. Sparrow currently provides GitHub.com and GitHub Enterprise API support, plus generic HTTPS Git credentials for hosts without a supported PR API.

## GitHub and GitHub Enterprise

Choose **GitHub / GitHub Enterprise**, enter the host, paste a personal access token, and select **Test & Add / Replace Token**. Sparrow validates the token with the host's authenticated-user endpoint before saving; the account name and any classic OAuth scope information returned by GitHub are shown without displaying the token. Use **Replace** on a saved account to replace its token. **Test** revalidates a saved GitHub token, and **Remove** deletes its metadata and secret.

The token link opens the host's token settings page. Fine-grained tokens generally need `Pull requests: read` for listing/details, `Pull requests: write` to create a PR, `Issues: read` for conversation comments, `Contents: read` for clone/fetch, and `Contents: write` for push. Classic tokens need `repo` for private Git operations and `read:user` for identity validation. Organization SSO authorization may also be required. Sparrow reports the permission hint from GitHub when the API provides it. A missing classic OAuth scopes header does not reliably identify a token as fine-grained.

The Pull Requests panel follows the repository's HTTPS or SSH remote and lists up to 2,000 requests. **Details** loads the description, file patches, conversation comments, and review comments; **Open** opens the request in a browser. **New PR** creates a pull request from the entered head and base branches. Listing, details, and creation use the selected account for that host. Details are capped at 500 files, 500 conversation comments, and 500 review comments; merge and close actions are not currently provided.

## Generic HTTPS Git and SSH

Choose **Generic HTTPS Git host** for a server that accepts HTTPS Git credentials but is not using Sparrow's GitHub-compatible PR API. Enter the host, username, and secret; Sparrow cannot validate this credential until the host is used by a Git operation. Generic accounts are not used for PR API requests.

HTTPS clone, fetch, pull, and push use Sparrow's askpass flow. Credentials are not added to command-line arguments, remote URLs, `.git/config`, or normal account metadata. Git output is sanitized before it appears in the UI. SSH remotes do not use these tokens; configure an SSH agent or change the remote to HTTPS.

## Secret storage and privacy

Account metadata is stored in Sparrow's configuration directory in `vcs-accounts.toml`; it contains host, username, provider, active selection, status, and storage kind, but no secret. Sparrow uses the operating-system credential store (macOS Keychain, Windows Credential Manager, or Linux Secret Service) for persistent secrets.

**Allow session-only memory fallback** is enabled by default. If the OS store is unavailable, Sparrow explains that the secret will remain in process memory until Sparrow exits and will not persist across restarts. Turn the option off to refuse saving when the secure store is unavailable. Sparrow does not fall back to plaintext or passphrase-encrypted files. Session-only account metadata can remain after restart so the account can be replaced or removed; the missing in-memory secret is not recoverable.

Token checks, account storage, and PR requests run on worker threads. Token values are masked while typing, held in zeroizing buffers where possible, and excluded from API errors and Git output. No live token, account, private remote, clone, fetch, or push was used to validate this implementation; verify keyring behavior and private-repository permissions in your environment before relying on it.

# AI Commit Messages

Sparrow's current AI commit-message integration supports local Ollama only. No cloud endpoint, provider key, OAuth flow, or shared API credential is configured or embedded. If Ollama is not installed/running, use **Settings → Version Control → AI Commit Messages** to configure its loopback URL and a model already installed in Ollama; use **Refresh local models** in the Source Control preview to discover models.

## Review-before-send workflow

1. Select the files you intend to commit in Source Control.
2. Click **AI ✦**. Sparrow builds a diff only for the checked paths, excludes configured sensitive/generated patterns, redacts common credential/private-key forms, and applies the configured character cap.
3. Review the exact system and user prompt shown in the preview, including its selected-path list, diff size, truncation status, and redaction count. Nothing is sent until you click **Generate on local Ollama**.
4. Edit the generated subject if needed, then choose **Replace message** or **Append message**. This only changes the commit-message editor; it does not stage, commit, or push anything.
5. Review the files and message, then use the existing Commit or Commit and Push actions.

The service accepts only HTTP loopback hosts (`localhost`, `127.0.0.1`, or `::1`), disables proxy use, limits diff and response sizes, and does not send unselected files. Exclusion globs are configurable; no automated redactor can guarantee detection of every secret, so inspect the preview before generating. Local inference may take up to 90 seconds; closing the preview hides it but does not interrupt an active blocking request.

## Settings and current limitations

The `ai-commit` settings group contains the enable flag, loopback Ollama URL, installed model, subject style, language, maximum diff characters, and comma-separated exclusions. Common environment files, private keys, credentials, build outputs, generated sources, maps, and lockfiles are excluded by default.

This is a partial provider implementation: OpenRouter PKCE, GitHub Models, user-configured cloud endpoints and consent, streaming/cancellation, per-repository disable, and recent-commit style matching are not available. No live Ollama service or desktop UI was used for acceptance testing; local HTTP mocks cover model discovery and completion.

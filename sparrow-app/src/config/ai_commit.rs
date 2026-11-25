use serde::{Deserialize, Serialize};
use structdesc::FieldNames;

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct AiCommitConfig {
    #[field_names(
        desc = "Enable local AI commit-message generation in Source Control."
    )]
    pub enabled: bool,
    #[field_names(
        desc = "Local Ollama server URL. Sparrow only accepts loopback HTTP endpoints."
    )]
    pub base_url: String,
    #[field_names(desc = "Installed Ollama model name, as shown by `ollama list`.")]
    pub model: String,
    #[field_names(
        desc = "Preferred commit subject style, for example imperative or conventional."
    )]
    pub style: String,
    #[field_names(desc = "Language for the generated commit subject.")]
    pub language: String,
    #[field_names(
        desc = "Maximum number of redacted diff characters sent to the local model."
    )]
    pub max_diff_chars: usize,
    #[field_names(desc = "Comma-separated glob patterns excluded from AI context.")]
    pub exclude_globs: String,
}

impl Default for AiCommitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            base_url: "http://127.0.0.1:11434".into(),
            model: String::new(),
            style: "imperative".into(),
            language: "English".into(),
            max_diff_chars: 20_000,
            exclude_globs: "**/.env*,**/*.pem,**/*.key,**/*.p12,**/*.pfx,**/*.keystore,**/id_rsa*,**/id_ed25519*,**/.ssh/**,**/.aws/**,**/.netrc,**/.npmrc,**/.pypirc,**/secrets/**,**/credentials.json,**/.sparrow/**,**/target/**,**/build/**,**/.dart_tool/**,**/.gradle/**,**/node_modules/**,**/vendor/**,**/Pods/**,**/DerivedData/**,**/generated/**,**/*.g.dart,**/*.min.js,**/*.map,**/Cargo.lock,**/pubspec.lock".into(),
        }
    }
}

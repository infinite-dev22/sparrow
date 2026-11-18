//! Signature help state — "which overload, which parameter" for the
//! current function call.

use floem::{
    peniko::kurbo::Rect,
    reactive::Scope,
    views::editor::id::EditorId,
};
use lsp_types::{SignatureHelp, SignatureInformation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureStatus {
    Inactive,
    Active,
}

#[derive(Clone)]
pub struct SignatureHelpData {
    pub status: SignatureStatus,
    /// Discards stale responses.
    pub request_id: usize,
    pub signatures: Vec<SignatureInformation>,
    pub active_signature: Option<u32>,
    pub active_parameter: Option<u32>,
    /// Measured after render, used for positioning.
    pub layout_rect: Rect,
    pub editor_id: Option<EditorId>,
    /// Offset where the request was initiated. Used by the UI to
    /// position the popup without re-querying the cursor.
    pub offset: usize,
}

impl SignatureHelpData {
    pub fn new(_cx: Scope) -> Self {
        Self {
            status: SignatureStatus::Inactive,
            request_id: 0,
            signatures: Vec::new(),
            active_signature: None,
            active_parameter: None,
            layout_rect: Rect::ZERO,
            editor_id: None,
            offset: 0,
        }
    }

    pub fn receive(
        &mut self,
        request_id: usize,
        help: Option<SignatureHelp>,
    ) {
        if self.request_id != request_id {
            return; // stale
        }
        match help {
            Some(help) if !help.signatures.is_empty() => {
                self.signatures = help.signatures;
                self.active_signature = help.active_signature;
                self.active_parameter = help.active_parameter;
                self.status = SignatureStatus::Active;
            }
            _ => {
                self.cancel();
            }
        }
    }

    pub fn cancel(&mut self) {
        self.status = SignatureStatus::Inactive;
        self.signatures.clear();
        self.active_signature = None;
        self.active_parameter = None;
    }

    pub fn is_active(&self) -> bool {
        self.status == SignatureStatus::Active && !self.signatures.is_empty()
    }

    pub fn current_signature(&self) -> Option<&SignatureInformation> {
        let idx = self.active_signature.unwrap_or(0) as usize;
        self.signatures.get(idx).or_else(|| self.signatures.first())
    }
}
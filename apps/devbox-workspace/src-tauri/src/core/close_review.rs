//! Closing remains a review until the exact current nonce and context approve it.
use product_contract::ProjectContext;
use serde::Serialize;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CloseRequest {
    pub nonce: String,
    pub context: Option<ProjectContext>,
}
#[derive(Default)]
pub struct CloseReview {
    pending: Option<CloseRequest>,
    approved: bool,
}
impl CloseReview {
    pub fn request(&mut self, context: Option<ProjectContext>) -> CloseRequest {
        if let Some(request) = &self.pending {
            if request.context == context {
                return request.clone();
            }
        }
        let request = CloseRequest {
            nonce: uuid::Uuid::new_v4().to_string(),
            context,
        };
        self.pending = Some(request.clone());
        request
    }
    pub fn confirm(
        &mut self,
        nonce: &str,
        context: Option<&ProjectContext>,
    ) -> Result<(), &'static str> {
        if !self
            .pending
            .as_ref()
            .is_some_and(|request| request.nonce == nonce && request.context.as_ref() == context)
        {
            return Err("close_review_stale");
        }
        self.pending = None;
        self.approved = true;
        Ok(())
    }
    pub fn cancel(&mut self, nonce: &str) -> Result<(), &'static str> {
        if !self
            .pending
            .as_ref()
            .is_some_and(|request| request.nonce == nonce)
        {
            return Err("close_review_stale");
        }
        self.pending = None;
        Ok(())
    }
    pub fn approved(&self) -> bool {
        self.approved
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_nonce_never_authorizes_drain_and_cancel_retry_changes_nonce() {
        let mut review = CloseReview::default();
        let first = review.request(None);
        assert!(review.confirm("stale", None).is_err());
        assert!(!review.approved());
        review.cancel(&first.nonce).unwrap();
        let second = review.request(None);
        assert_ne!(first.nonce, second.nonce);
        assert!(review.confirm(&first.nonce, None).is_err());
        review.confirm(&second.nonce, None).unwrap();
        assert!(review.approved());
        assert!(review.confirm(&second.nonce, None).is_err());
    }
}

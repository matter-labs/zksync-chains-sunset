use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::fmt;
use tracing::error;
use zksync_lite_sunset_core::{
    claims::shared::ClaimError, eligibility::EligibilityError, migrations::shared::MigrationError,
};

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    code: &'static str,
    message: String,
}

impl ApiError {
    pub fn public(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn logged<E>(
        status: StatusCode,
        code: &'static str,
        public_message: &'static str,
        context: &'static str,
        err: &E,
    ) -> Self
    where
        E: fmt::Debug,
    {
        error!(code, context, error = ?err, "api request failed");
        Self::public(status, code, public_message)
    }

    pub fn throttled() -> Self {
        Self::public(
            StatusCode::TOO_MANY_REQUESTS,
            "REQUEST_THROTTLED",
            "too many in-flight requests, please retry shortly",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                code: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}

impl From<ClaimError> for ApiError {
    fn from(err: ClaimError) -> Self {
        match err {
            err @ ClaimError::InvalidAddress { .. }
            | err @ ClaimError::NoClaimsSelected
            | err @ ClaimError::TooManyClaims { .. }
            | err @ ClaimError::InvalidClaimOrdering
            | err @ ClaimError::InvalidClaimsHash
            | err @ ClaimError::ExpiredSignature
            | err @ ClaimError::InvalidSignature
            | err @ ClaimError::MerkleRootMismatch { .. }
            | err @ ClaimError::InvalidNonce { .. }
            | err @ ClaimError::NftClaimNotSupported { .. }
            | err @ ClaimError::InvalidTxHash { .. } => {
                Self::public(StatusCode::BAD_REQUEST, err.code(), err.to_string())
            }
            err @ ClaimError::SubmittedTransactionMismatch { .. } => {
                Self::public(StatusCode::CONFLICT, err.code(), err.to_string())
            }
            err @ ClaimError::TokenNotEligible { .. }
            | err @ ClaimError::ClaimNotFound { .. }
            | err @ ClaimError::SubmittedTransactionNotFound { .. } => {
                Self::public(StatusCode::NOT_FOUND, err.code(), err.to_string())
            }
            err @ ClaimError::OnchainConfig { .. } => Self::logged(
                StatusCode::INTERNAL_SERVER_ERROR,
                err.code(),
                "internal server error",
                "claim request hit invalid onchain configuration",
                &err,
            ),
            err @ ClaimError::OnchainRpc { .. } => Self::logged(
                StatusCode::BAD_GATEWAY,
                err.code(),
                "upstream service error",
                "claim request failed while calling the onchain RPC",
                &err,
            ),
            err @ ClaimError::Storage { .. } => Self::logged(
                StatusCode::INTERNAL_SERVER_ERROR,
                err.code(),
                "internal server error",
                "claim request failed while reading or writing storage",
                &err,
            ),
        }
    }
}

impl From<MigrationError> for ApiError {
    fn from(err: MigrationError) -> Self {
        match err {
            err @ MigrationError::InvalidAddress { .. }
            | err @ MigrationError::InvalidTxHash { .. }
            | err @ MigrationError::MissingTokenSelection => {
                Self::public(StatusCode::BAD_REQUEST, err.code(), err.to_string())
            }
            err @ MigrationError::AlreadyMigrated { .. }
            | err @ MigrationError::SubmittedTransactionMismatch { .. } => {
                Self::public(StatusCode::CONFLICT, err.code(), err.to_string())
            }
            err @ MigrationError::MigrationNotFound { .. }
            | err @ MigrationError::SubmittedTransactionNotFound { .. } => {
                Self::public(StatusCode::NOT_FOUND, err.code(), err.to_string())
            }
            err @ MigrationError::OnchainConfig { .. } => Self::logged(
                StatusCode::INTERNAL_SERVER_ERROR,
                err.code(),
                "internal server error",
                "migration request hit invalid onchain configuration",
                &err,
            ),
            err @ MigrationError::OnchainRpc { .. } => Self::logged(
                StatusCode::BAD_GATEWAY,
                err.code(),
                "upstream service error",
                "migration request failed while calling the onchain RPC",
                &err,
            ),
            err @ MigrationError::Storage { .. } => Self::logged(
                StatusCode::INTERNAL_SERVER_ERROR,
                err.code(),
                "internal server error",
                "migration request failed while reading or writing storage",
                &err,
            ),
        }
    }
}

impl From<EligibilityError> for ApiError {
    fn from(err: EligibilityError) -> Self {
        Self::logged(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_STORAGE",
            "internal server error",
            "eligibility request failed while loading data",
            &err,
        )
    }
}

impl From<JsonRejection> for ApiError {
    fn from(err: JsonRejection) -> Self {
        Self::public(
            StatusCode::BAD_REQUEST,
            "INVALID_ADDRESS",
            format!("invalid address: {err}"),
        )
    }
}

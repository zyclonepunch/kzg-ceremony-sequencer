use crate::{
    lobby::{ActiveContributorError, SharedLobbyState},
    storage::{PersistentStorage, StorageError},
    SessionId, SharedTranscript,
};
use axum::{
    response::{IntoResponse, Response},
    Extension, Json,
};
use http::StatusCode;
use kzg_ceremony_crypto::{BatchContribution, ErrorCode};
use serde::Serialize;
use strum::IntoStaticStr;
use thiserror::Error;
use tokio::{task::JoinError, time::Instant};

#[derive(Debug, Error, IntoStaticStr)]
pub enum TryContributeError {
    #[error("unknown session id")]
    UnknownSessionId,
    #[error("call came too early. rate limited")]
    RateLimited,
    #[error("another contribution in progress")]
    AnotherContributionInProgress,
    #[error("lobby is full")]
    LobbyIsFull,
    #[error("error in storage layer: {0}")]
    StorageError(#[from] StorageError),
    #[error("background task error: {0}")]
    TaskError(#[from] JoinError),
}

impl ErrorCode for TryContributeError {
    fn to_error_code(&self) -> String {
        format!("TryContributeError::{}", <&str>::from(self))
    }
}

impl From<ActiveContributorError> for TryContributeError {
    fn from(err: ActiveContributorError) -> Self {
        match err {
            ActiveContributorError::AnotherContributionInProgress
            | ActiveContributorError::NotUsersTurn => Self::AnotherContributionInProgress,
            ActiveContributorError::UserNotInLobby
            | ActiveContributorError::NotActiveContributor => Self::UnknownSessionId,
            ActiveContributorError::SessionCountLimitExceeded
            | ActiveContributorError::LobbySizeLimitExceeded => Self::LobbyIsFull,
            ActiveContributorError::RateLimited => Self::RateLimited,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct TryContributeResponse<C> {
    contribution: C,
}

impl<C: Serialize> IntoResponse for TryContributeResponse<C> {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self.contribution)).into_response()
    }
}

pub async fn try_contribute(
    session_id: SessionId,
    Extension(lobby_state): Extension<SharedLobbyState>,
    Extension(storage): Extension<PersistentStorage>,
    Extension(transcript): Extension<SharedTranscript>,
    Extension(options): Extension<crate::Options>,
) -> Result<TryContributeResponse<BatchContribution>, TryContributeError> {
    let res = lobby_state
        .modify_participant(&session_id, |info| {
            let now = Instant::now();
            if !info.is_first_ping_attempt
                && now < info.last_ping_time + options.lobby.min_checkin_delay()
            {
                return Err(TryContributeError::RateLimited);
            }
            info.is_first_ping_attempt = false;
            info.last_ping_time = now;
            Ok(info.token.unique_identifier())
        })
        .await;

    let uid = if let Some(inner) = res {
        inner?
    } else {
        // Session not found. Check if they're the active contributor, and
        // if so, if we can give them back the contribution base they need.
        lobby_state
            .request_contribution_file_again(&session_id)
            .await?;

        let transcript = transcript.read().await;
        return Ok(TryContributeResponse {
            contribution: transcript.contribution(),
        });
    };

    // Attempt to set ourselves as the current contributor in the background,
    // so that request cancelation doesn't interrupt it inbetween the lobby_state
    // and storage calls.
    tokio::spawn(async move {
        lobby_state.enter_lobby(&session_id).await?;

        lobby_state
            .set_current_contributor(&session_id, options.lobby.compute_deadline, storage.clone())
            .await
            .map_err(TryContributeError::from)?;

        storage.insert_contributor(&uid).await?;
        let transcript = transcript.read().await;

        Ok(TryContributeResponse {
            contribution: transcript.contribution(),
        })
    })
    .await
    .unwrap_or_else(|e| Err(TryContributeError::TaskError(e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::v1::lobby::TryContributeError,
        storage::storage_client,
        test_util::{create_test_session_info, test_options},
        tests::test_transcript,
    };
    use std::{sync::Arc, time::Duration};
    use tokio::sync::RwLock;

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn lobby_try_contribute_test() {
        let opts = test_options();
        let lobby_state = SharedLobbyState::new(opts.lobby.clone());
        let transcript = Arc::new(RwLock::new(test_transcript()));
        let db = storage_client(&opts.storage).await.unwrap();

        let session_id = SessionId::new();
        let other_session_id = SessionId::new();

        // no users in lobby
        let unknown_session_response = try_contribute(
            session_id.clone(),
            Extension(lobby_state.clone()),
            Extension(db.clone()),
            Extension(transcript.clone()),
            Extension(opts),
        )
        .await;
        assert!(matches!(
            unknown_session_response,
            Err(TryContributeError::UnknownSessionId)
        ));
        lobby_state
            .insert_session(session_id.clone(), create_test_session_info(100))
            .await
            .unwrap();
        lobby_state
            .insert_session(other_session_id.clone(), create_test_session_info(100))
            .await
            .unwrap();

        // "other participant" is contributing
        try_contribute(
            other_session_id.clone(),
            Extension(lobby_state.clone()),
            Extension(db.clone()),
            Extension(transcript.clone()),
            Extension(test_options()),
        )
        .await
        .unwrap();
        let contribution_in_progress_response = try_contribute(
            session_id.clone(),
            Extension(lobby_state.clone()),
            Extension(db.clone()),
            Extension(transcript.clone()),
            Extension(test_options()),
        )
        .await;

        assert!(matches!(
            contribution_in_progress_response,
            Err(TryContributeError::AnotherContributionInProgress)
        ));

        tokio::time::pause();

        // call the endpoint too soon - rate limited, other participant computing
        tokio::time::advance(Duration::from_secs(5)).await;
        let too_soon_response = try_contribute(
            session_id.clone(),
            Extension(lobby_state.clone()),
            Extension(db.clone()),
            Extension(transcript.clone()),
            Extension(test_options()),
        )
        .await;

        assert!(
            matches!(too_soon_response, Err(TryContributeError::RateLimited),),
            "response expected: Err(TryContributeError::RateLimited) actual: {too_soon_response:?}"
        );

        // "other participant" finished contributing
        lobby_state.clear_current_contributor().await;

        // call the endpoint too soon - rate limited, no one computing
        tokio::time::advance(Duration::from_secs(5)).await;
        let too_soon_response = try_contribute(
            session_id.clone(),
            Extension(lobby_state.clone()),
            Extension(db.clone()),
            Extension(transcript.clone()),
            Extension(test_options()),
        )
        .await;
        assert!(matches!(
            too_soon_response,
            Err(TryContributeError::RateLimited)
        ));

        // wait enough time to be able to contribute
        tokio::time::advance(Duration::from_secs(19)).await;
        // the auto-advance of paused time can expire our contribution unexpectedly
        tokio::time::resume();
        let success_response = try_contribute(
            session_id.clone(),
            Extension(lobby_state.clone()),
            Extension(db.clone()),
            Extension(transcript.clone()),
            Extension(test_options()),
        )
        .await
        .expect("try_contribute that should succeed failed");

        // if a user attempts to try_contribute again they should get rate limited
        let check_again = try_contribute(
            session_id.clone(),
            Extension(lobby_state.clone()),
            Extension(db.clone()),
            Extension(transcript.clone()),
            Extension(test_options()),
        )
        .await;
        assert!(matches!(check_again, Err(TryContributeError::RateLimited)));

        tokio::time::pause();
        tokio::time::advance(test_options().lobby.min_checkin_delay()).await;
        tokio::time::resume();

        // but after waiting a bit they should be able to re-fetch their transcript
        let refetch_transcript = try_contribute(
            session_id.clone(),
            Extension(lobby_state.clone()),
            Extension(db.clone()),
            Extension(transcript.clone()),
            Extension(test_options()),
        )
        .await
        .expect("re-fetching the transcript with try_contribute failed");
        assert_eq!(success_response, refetch_transcript);
    }
}

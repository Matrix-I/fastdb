use db_wire::metadata::{MetadataFailure, MetadataFeature, MetadataSupport};
use std::collections::HashMap;

pub(super) type MetadataStates = HashMap<MetadataFeature, MetadataState>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MetadataState {
    Ready,
    Unavailable(MetadataSupport),
    PermissionDenied(String),
    Failed(String),
}
impl MetadataState {
    pub fn message(&self) -> Option<String> {
        match self {
            Self::Ready => None,
            Self::Unavailable(support) => support.explanation().map(str::to_owned),
            Self::PermissionDenied(error) => Some(format!("Permission denied. {error}")),
            Self::Failed(error) => Some(format!("Metadata load failed. {error}")),
        }
    }
    pub fn from_error(error: &anyhow::Error) -> Self {
        let failure = MetadataFailure::from_error(error);
        if failure.permission_denied {
            Self::PermissionDenied(failure.message)
        } else {
            Self::Failed(failure.message)
        }
    }
}

pub(super) async fn load_optional<T: Default>(
    support: MetadataSupport,
    future: impl std::future::Future<Output = anyhow::Result<T>>,
) -> (T, MetadataState) {
    if support != MetadataSupport::Available {
        return (T::default(), MetadataState::Unavailable(support));
    }
    const OPTIONAL_METADATA_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    match tokio::time::timeout(OPTIONAL_METADATA_TIMEOUT, future).await {
        Ok(Ok(value)) => (value, MetadataState::Ready),
        Ok(Err(error)) => (T::default(), MetadataState::from_error(&error)),
        Err(_) => (
            T::default(),
            MetadataState::Failed("Request timed out; refresh to retry.".into()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{MetadataState, MetadataSupport, load_optional};
    #[test]
    fn optional_failures_are_not_empty_success_and_unavailable_readers_do_not_run() {
        db_wire::get_runtime().block_on(async {
            let (rows, state) = load_optional(MetadataSupport::Available, async {
                Ok(Vec::<String>::new())
            })
            .await;
            assert!(rows.is_empty());
            assert_eq!(state, MetadataState::Ready);
            assert_eq!(state.message(), None);
            let (rows, state) = load_optional(MetadataSupport::Available, async {
                Err::<Vec<String>, _>(anyhow::anyhow!("fixture failure"))
            })
            .await;
            assert!(rows.is_empty());
            assert_eq!(state, MetadataState::Failed("fixture failure".into()));
            assert!(state.message().unwrap().contains("fixture failure"));
            for support in [
                MetadataSupport::NotImplemented,
                MetadataSupport::Unsupported,
            ] {
                let (_, state) = load_optional(support, async {
                    panic!("unavailable reader must not run");
                    #[allow(unreachable_code)]
                    Ok(Vec::<String>::new())
                })
                .await;
                assert_eq!(state, MetadataState::Unavailable(support));
                assert_eq!(state.message().as_deref(), support.explanation());
            }
        });
    }
}

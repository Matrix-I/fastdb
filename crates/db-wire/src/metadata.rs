//! Implemented metadata readers, independent of connection privileges and row counts.
use crate::catalog::CatalogSection;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataFeature {
    Columns,
    Constraints,
    Indexes,
    ForeignKeys,
    TableDetails,
    Catalog(CatalogSection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataSupport {
    Available,
    NotImplemented,
    Unsupported,
}
impl MetadataSupport {
    pub fn explanation(self) -> Option<&'static str> {
        match self {
            Self::Available => None,
            Self::NotImplemented => {
                Some("This metadata reader is not implemented for this provider yet.")
            }
            Self::Unsupported => Some("This metadata feature is not supported by this provider."),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MetadataCapabilities {
    available: &'static [MetadataFeature],
    unsupported: &'static [MetadataFeature],
}
impl Default for MetadataCapabilities {
    fn default() -> Self {
        Self::new(&[MetadataFeature::Columns], &[])
    }
}
impl MetadataCapabilities {
    pub const fn new(
        available: &'static [MetadataFeature],
        unsupported: &'static [MetadataFeature],
    ) -> Self {
        Self {
            available,
            unsupported,
        }
    }
    pub fn support(self, feature: MetadataFeature) -> MetadataSupport {
        if self.available.contains(&feature) {
            MetadataSupport::Available
        } else if self.unsupported.contains(&feature) {
            MetadataSupport::Unsupported
        } else {
            MetadataSupport::NotImplemented
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataFailure {
    pub permission_denied: bool,
    pub message: String,
}
impl MetadataFailure {
    pub fn from_error(error: &anyhow::Error) -> Self {
        let permission_denied = error.chain().any(|cause| {
            if let Some(error) = cause.downcast_ref::<tokio_postgres::Error>() {
                return error.code()
                    == Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE);
            }
            if let Some(error) = cause.downcast_ref::<mysql_async::ServerError>() {
                // Authentication, database/table/column/routine access and administrative privilege denial.
                return matches!(error.code, 1044 | 1045 | 1142 | 1143 | 1227 | 1370 | 1698);
            }
            false
        });
        Self {
            permission_denied,
            message: format!("{error:#}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MetadataFailure;
    #[test]
    fn native_permission_codes_survive_context_without_matching_message_text() {
        let error = anyhow::Error::new(mysql_async::Error::Server(mysql_async::ServerError {
            code: 1142,
            message: "localized server message".into(),
            state: "42000".into(),
        }))
        .context("Loading indexes");
        let result = MetadataFailure::from_error(&error);
        assert!(result.permission_denied);
        assert!(result.message.contains("Loading indexes"));
        let error = anyhow::anyhow!("permission denied while parsing an unrelated string");
        assert!(!MetadataFailure::from_error(&error).permission_denied);
        let error = anyhow::Error::new(mysql_async::Error::Server(mysql_async::ServerError {
            code: 1064,
            message: "syntax error".into(),
            state: "42000".into(),
        }));
        assert!(!MetadataFailure::from_error(&error).permission_denied);
    }
}

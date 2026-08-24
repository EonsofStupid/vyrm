use super::*;

mod collection;
mod points;
mod search;

pub(in crate::engine) use collection::{
    internal_vector_metric, validate_collection_query, vector_collection_error,
};
pub(in crate::engine) use points::public_data_ref;
pub(in crate::engine) use search::internal_vector_filter;

pub(in crate::engine) fn core_vector(error: vyrm_core::Error) -> ServiceError {
    ServiceError::Vector(error.to_string())
}

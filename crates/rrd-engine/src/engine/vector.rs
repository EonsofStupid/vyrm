use super::*;

mod collection;
mod index;
mod points;
mod search;

pub(in crate::engine) use collection::{
    internal_vector_metric, public_vector_collection, validate_collection_query,
    vector_collection_error, vector_matches_collection,
};
pub(in crate::engine) use points::public_data_ref;
pub(in crate::engine) use search::{internal_vector_filter, internal_vector_query};

pub(in crate::engine) fn core_vector(error: rrd_core::Error) -> ServiceError {
    ServiceError::Vector(error.to_string())
}

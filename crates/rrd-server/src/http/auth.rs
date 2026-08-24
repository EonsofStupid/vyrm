use super::*;

pub(super) fn authenticated_session(
    headers: &HeaderMap,
    expected_session: Option<&str>,
) -> std::result::Result<(CorrelationId, CorrelationId), ApiError> {
    let authentication_error = || {
        ApiError::new(
            ErrorCode::Unauthenticated,
            "exactly one valid session and authorization header are required",
            false,
        )
    };
    let sessions = header_values(headers, "X-RRD-Session").map_err(|_| authentication_error())?;
    let authorizations =
        header_values(headers, "Authorization").map_err(|_| authentication_error())?;
    if sessions.len() != 1 || authorizations.len() != 1 {
        return Err(ApiError::new(
            ErrorCode::Unauthenticated,
            "exactly one session and authorization header are required",
            false,
        ));
    }
    if expected_session.is_some_and(|expected| sessions[0] != expected) {
        return Err(ApiError::new(
            ErrorCode::PermissionDenied,
            "session path and header differ",
            false,
        ));
    }
    let token = authorizations[0].strip_prefix("Bearer ").ok_or_else(|| {
        ApiError::new(
            ErrorCode::Unauthenticated,
            "Authorization must use Bearer",
            false,
        )
    })?;
    Ok((parse_correlation(sessions[0])?, parse_correlation(token)?))
}

pub(super) fn api_key_identity(
    headers: &HeaderMap,
) -> std::result::Result<(CanonicalId, String), ApiError> {
    let principals = header_values(headers, "X-RRD-Principal").map_err(|_| {
        ApiError::new(
            ErrorCode::Unauthenticated,
            "exactly one principal and API-key authorization header are required",
            false,
        )
    })?;
    let authorizations = header_values(headers, "Authorization").map_err(|_| {
        ApiError::new(
            ErrorCode::Unauthenticated,
            "exactly one principal and API-key authorization header are required",
            false,
        )
    })?;
    if principals.len() != 1 || authorizations.len() != 1 {
        return Err(ApiError::new(
            ErrorCode::Unauthenticated,
            "exactly one principal and API-key authorization header are required",
            false,
        ));
    }
    let credential = authorizations[0].strip_prefix("ApiKey ").ok_or_else(|| {
        ApiError::new(
            ErrorCode::Unauthenticated,
            "session creation Authorization must use ApiKey",
            false,
        )
    })?;
    if credential.is_empty() || credential.len() > 4_096 {
        return Err(ApiError::new(
            ErrorCode::Unauthenticated,
            "API key is empty or oversized",
            false,
        ));
    }
    Ok((
        CanonicalId::new(principals[0])
            .map_err(|error| ApiError::new(ErrorCode::Unauthenticated, error.to_string(), false))?,
        credential.into(),
    ))
}

pub(super) fn header_values<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
) -> std::result::Result<Vec<&'a str>, ApiError> {
    let values = headers
        .get_all(name)
        .iter()
        .map(|value| {
            value.to_str().map_err(|_| {
                ApiError::new(
                    ErrorCode::InvalidArgument,
                    format!("{name} header is not visible ASCII"),
                    false,
                )
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if values.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidArgument,
            format!("missing {name} header"),
            false,
        ));
    }
    Ok(values)
}

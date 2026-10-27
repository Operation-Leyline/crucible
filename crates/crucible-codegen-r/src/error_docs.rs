use crucible_ir::ir::Service;

pub(crate) fn runtime(service: &Service, source: &str) -> String {
    if service.error_docs_base.is_none() {
        return source.to_owned();
    }
    source.replace(
        "  body <- tryCatch(json_parse(response$body), error = function(e) json_object())",
        "  body <- tryCatch(json_parse(response$body), error = function(e) NULL)\n  empty_error_body <- all(response$body %in% as.raw(c(32, 9, 13, 10)))\n  valid_error_object <- empty_error_body || .crucible_is_object(body)",
    ).replace(
        "  model <- if (is.null(target)) NULL else tryCatch({",
        "  model <- if (is.null(target) || (!valid_error_object && is.null(traits$payload))) NULL else tryCatch({",
    ).replace(
        "value[traits$payload[[1L]]] <- list(.crucible_payload(target, traits, response$body)); value",
        "if (!(identical(traits$payloadKind, 'document') && empty_error_body)) value[traits$payload[[1L]]] <- list(.crucible_payload(target, traits, response$body)); value",
    ).replace(
        "  error <- .crucible_condition(",
        &format!(
            "{}  error <- .crucible_condition(",
            include_str!("../runtime/error_docs.inc")
        ),
    )
}

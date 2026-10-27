  documentation <- traits$documentation
  metadata['doc_url'] <- list(documentation$url)
  if (!is.null(model) && !is.null(documentation$reasonMember)) {
    attribute <- shape$members[[documentation$reasonMember]]$attribute
    reason <- model[[attribute]]
    if (inherits(reason, 'crucible_unset')) reason <- documentation$reasonDefault
    if (is.character(reason) && length(reason) == 1L && as.character(reason) %in% names(documentation$reasonUrls)) {
      metadata$doc_url <- documentation$reasonUrls[[match(as.character(reason), names(documentation$reasonUrls))]]
    }
  }

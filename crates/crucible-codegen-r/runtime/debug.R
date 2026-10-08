# Shared diagnostic formatting never serializes a sensitive value.
.crucible_debug <- function(value, target = attr(value, 'crucible_target'), depth = 0L) {
  if (depth > 64L) return('<nesting limit>')
  if (inherits(value, 'crucible_unset')) return('absent')
  shape <- if (is.null(target)) NULL else .crucible_shape(target)
  if (!is.null(shape) && isTRUE(shape$sensitive)) return('*** Sensitive Data Redacted ***')
  if (is.null(value)) return('null')
  render <- function(v, member) {
    if (isTRUE(member$sensitive)) '*** Sensitive Data Redacted ***' else .crucible_debug(v, member$target, depth + 1L)
  }
  if (!is.null(shape) && shape$kind == 'structure') {
    parts <- character()
    for (name in names(shape$members)) {
      member <- shape$members[[name]]; v <- value[[member$attribute]]
      if (!(member$attribute %in% names(value)) || inherits(v, 'crucible_unset')) next
      parts <- c(parts, paste0(name, '=', render(v, member)))
    }
    return(paste0(shape$name, '(', paste(parts, collapse = ', '), ')'))
  }
  if (!is.null(shape) && shape$kind == 'union') {
    member <- shape$members[[value$member]]
    text <- if (is.null(member)) '*** Sensitive Data Redacted ***' else render(value$value, member)
    return(paste0(shape$name, '(', value$member, '=', text, ')'))
  }
  if (!is.null(shape) && shape$kind == 'list') return(paste0('[', paste(vapply(value, render, '', member = shape$member), collapse = ', '), ']'))
  if (!is.null(shape) && shape$kind == 'map') {
    if (isTRUE(shape$key$sensitive) || isTRUE(.crucible_shape(shape$key$target)$sensitive)) return('*** Sensitive Data Redacted ***')
    return(paste0('{', paste(vapply(names(value), function(key) paste0(key, ': ', render(value[[key]], shape$value)), ''), collapse = ', '), '}'))
  }
  if (inherits(value, 'crucible_timestamp')) return(paste0('Timestamp(', .crucible_epoch_text(value), ')'))
  if (!is.null(shape) && shape$kind %in% c('enum', 'intEnum')) return(paste0(shape$name, '(value=', json_encode(unclass(value)), ')'))
  if (is.list(value)) return(paste0('[', paste(vapply(value, .crucible_debug, '', depth = depth + 1L), collapse = ', '), ']'))
  if (is.raw(value)) return(paste0('[', paste(as.integer(value), collapse = ', '), ']'))
  json_encode(unclass(value))
}
#' Human-readable model representation
#' @param x A generated model, union, enum, or service condition.
#' @param ... Unused formatting arguments.
#' @return A single string; sensitive data is redacted and absent members omitted.
format.crucible_model <- function(x, ...) .crucible_debug(x)
format.crucible_union <- function(x, ...) .crucible_debug(x)
format.crucible_enum <- function(x, ...) .crucible_debug(x, attr(x, 'crucible_target'))
format.crucible_int_enum <- function(x, ...) .crucible_debug(x, attr(x, 'crucible_target'))
format.crucible_error <- function(x, ...) {
  if (!is.null(x$model)) return(.crucible_debug(x$model))
  paste0(class(x)[[1L]], '(*** Sensitive Data Redacted ***)')
}
#' Print a generated diagnostic representation
#' @param x A generated model value or condition.
#' @param ... Unused printing arguments.
#' @return The value invisibly.
print.crucible_model <- function(x, ...) { cat(format(x), '\n', sep = ''); invisible(x) }
print.crucible_union <- print.crucible_model
print.crucible_enum <- print.crucible_model
print.crucible_int_enum <- print.crucible_model
print.crucible_error <- print.crucible_model
